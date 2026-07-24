use std::sync::Arc;

use gsplat_core::{Camera, SceneBuffers, Vec3f};

use super::*;
use crate::plans::{
    FrameIdentity, GpuCapabilityReceipt, GpuOwnerToken, IndirectCountSemantics, OrderLane, PlanId,
    PlanSetError, TestGpuAdmissionMode, WorkUnavailable,
};
use crate::renderer::frame::Viewport;
use crate::renderer::gpu_prepare::GpuPreparationError;
use crate::renderer::{FrameExecutionError, PreparedRuntimeSlot, execute_frame_gpu};
use crate::scene::{ResidentSceneCpu, SceneRuntime};

fn capability(source_count: u32, sh_degree: u8) -> GpuCapabilityReceipt {
    GpuCapabilityReceipt {
        source_count,
        capacity: source_count,
        resident_count: source_count,
        addressable_count: source_count,
        sh_degree,
        scene_generation: 1,
        contract_generation: 1,
        plan_set_generation: 2,
    }
}

fn source(depths: &[f32], sh_degree: u8) -> SceneBuffers {
    let coefficient_count = match sh_degree {
        0 => 0,
        1 => 9,
        2 => 24,
        3 => 45,
        _ => unreachable!("Exact tests support only SH0-SH3"),
    };
    SceneBuffers {
        positions: depths
            .iter()
            .copied()
            .enumerate()
            .map(|(index, depth)| Vec3f::new(index as f32 * 0.0001, 0.0, depth))
            .collect(),
        opacity: vec![0.0; depths.len()],
        scale_xyz: vec![[-3.0; 3]; depths.len()],
        rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; depths.len()],
        color_dc: (0..depths.len())
            .map(|index| {
                let value = index as f32 / depths.len().max(1) as f32;
                [value, value * 0.5, value * 0.25]
            })
            .collect(),
        sh_degree,
        sh_rest: (coefficient_count != 0).then(|| vec![0.001; depths.len() * coefficient_count]),
    }
}

fn runtime(depths: &[f32], sh_degree: u8) -> SceneRuntime {
    let resident =
        ResidentSceneCpu::encode_owned(source(depths, sh_degree)).expect("resident scene");
    SceneRuntime::prepare(resident).expect("scene runtime")
}

fn camera() -> Camera {
    let mut camera = Camera::default();
    camera.intrinsics.near_plane = 1.0;
    camera.intrinsics.far_plane = 3.0;
    camera
}

fn adversarial_depths(count: usize) -> Vec<f32> {
    let below_near = f32::from_bits(1.0_f32.to_bits() - 1);
    let above_far = f32::from_bits(3.0_f32.to_bits() + 1);
    (0..count)
        .map(|index| match index % 9 {
            0 => 1.0,
            1 => 3.0,
            2 | 3 => 2.0,
            4 => below_near,
            5 => above_far,
            6 | 7 => 2.5,
            _ => 1.5,
        })
        .collect()
}

fn portable_limits() -> wgpu::Limits {
    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_storage_buffers_per_shader_stage = 8;
    limits.max_storage_buffer_binding_size = 128 << 20;
    limits.max_buffer_size = 128 << 20;
    limits
}

async fn request_device() -> Option<(
    wgpu::Instance,
    wgpu::Adapter,
    wgpu::AdapterInfo,
    Arc<wgpu::Device>,
    Arc<wgpu::Queue>,
)> {
    // Fixture only. GpuPreprojectPlan consumes the renderer's existing owner
    // and never requests an adapter or device in production.
    let required = std::env::var_os("GSPLAT_REQUIRE_GPU_PREPROJECT_PLAN").is_some();
    let require_metal = std::env::var_os("GSPLAT_REQUIRE_METAL_GPU_PREPROJECT_PLAN").is_some();
    let instance = wgpu::Instance::default();
    let adapter = match instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
    {
        Ok(adapter) => adapter,
        Err(error) if required || require_metal => {
            panic!("required GPU Preproject plan adapter unavailable: {error}")
        }
        Err(error) => {
            eprintln!("skipping GPU Preproject plan test; adapter unavailable: {error}");
            return None;
        }
    };
    let info = adapter.get_info();
    if require_metal {
        assert_eq!(info.backend, wgpu::Backend::Metal, "Metal adapter required");
    }
    let limits = portable_limits();
    if !limits.check_limits(&adapter.limits()) {
        if required || require_metal {
            panic!("required GPU Preproject plan limits are unavailable: {limits:?}");
        }
        eprintln!("skipping GPU Preproject plan test; portable limits unavailable");
        return None;
    }
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("exact-gpu-preproject-plan-test-device"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    };
    match adapter.request_device(&descriptor).await {
        Ok((device, queue)) => Some((instance, adapter, info, Arc::new(device), Arc::new(queue))),
        Err(error) if required || require_metal => {
            panic!("required GPU Preproject plan device unavailable: {error}")
        }
        Err(error) => {
            eprintln!("skipping GPU Preproject plan test; device unavailable: {error}");
            None
        }
    }
}

#[test]
fn admission_rejects_incomplete_counts_and_sh_downgrade() {
    let mut mismatched = capability(1, 3);
    mismatched.addressable_count = 0;
    assert_eq!(
        GpuPreprojectPlan::prepare(mismatched).map(|_| ()),
        Err(GpuPreprojectError::AdmissionContractMismatch {
            component: "source/capacity/resident/addressable count"
        })
    );

    let mut invalid_sh = capability(1, 3);
    invalid_sh.sh_degree = 4;
    assert_eq!(
        GpuPreprojectPlan::prepare(invalid_sh).map(|_| ()),
        Err(GpuPreprojectError::AdmissionContractMismatch {
            component: "SH degree"
        })
    );
}

#[test]
fn frame_preflight_guards_camera_viewport_owner_graph_counts_sh_and_generations() {
    let scene = runtime(&[2.0], 0);
    let plan = GpuPreprojectPlan::prepare(capability(1, 0)).expect("plan");
    let base = GpuPreprojectGuard {
        frame: FrameIdentity::new(1, 1, 1, 1, 2),
        source_count: 1,
        sh_degree: 0,
        camera: camera(),
        viewport_width: 64,
        viewport_height: 64,
    };

    assert_eq!(
        plan.validate_frame(
            &scene,
            GpuPreprojectGuard {
                source_count: 0,
                ..base
            }
        ),
        Err(GpuPreprojectError::FrameContractMismatch {
            component: "source count"
        })
    );
    let sh_plan = GpuPreprojectPlan::prepare(capability(1, 1)).expect("SH1 plan");
    assert_eq!(
        sh_plan.validate_frame(&scene, base),
        Err(GpuPreprojectError::FrameContractMismatch {
            component: "SH degree"
        })
    );

    let mut invalid_camera = camera();
    invalid_camera.intrinsics.near_plane = 4.0;
    invalid_camera.intrinsics.far_plane = 1.0;
    assert_eq!(
        plan.validate_frame(
            &scene,
            GpuPreprojectGuard {
                camera: invalid_camera,
                ..base
            }
        ),
        Err(GpuPreprojectError::FrameContractMismatch {
            component: "camera"
        })
    );
    assert_eq!(
        plan.validate_frame(
            &scene,
            GpuPreprojectGuard {
                viewport_width: 0,
                ..base
            }
        ),
        Err(GpuPreprojectError::FrameContractMismatch {
            component: "viewport"
        })
    );
    for stale in [
        FrameIdentity::new(2, 1, 1, 1, 2),
        FrameIdentity::new(1, 1, 1, 2, 2),
        FrameIdentity::new(1, 1, 1, 1, 3),
    ] {
        assert_eq!(
            plan.validate_frame(
                &scene,
                GpuPreprojectGuard {
                    frame: stale,
                    ..base
                }
            ),
            Err(GpuPreprojectError::FrameContractMismatch {
                component: "scene/contract/plan-set generation"
            })
        );
    }
    assert_eq!(
        plan.validate_frame(&scene, base),
        Err(GpuPreprojectError::FrameContractMismatch {
            component: "complete Preproject graph"
        })
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_gpu_preproject_is_exact_current_indirect_owner_bound_and_discard_safe() {
    pollster::block_on(async {
        let Some((_instance, _adapter, info, device, queue)) = request_device().await else {
            return;
        };
        eprintln!(
            "EXACT_GPU_PREPROJECT_PLAN adapter={} backend={:?} device_type={:?} driver={} driver_info={}",
            info.name, info.backend, info.device_type, info.driver, info.driver_info
        );

        for (count, sh_degree) in [(0, 0), (1, 1), (127, 2), (128, 3), (129, 0), (1_025, 3)] {
            let depths = adversarial_depths(count);
            let resident =
                ResidentSceneCpu::encode_owned(source(&depths, sh_degree)).expect("scene");
            let mut slot = PreparedRuntimeSlot::prepare(resident).expect("CPU fallback");
            slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
            let receipt = slot
                .prepare_gpu(&device, &queue)
                .await
                .expect("atomic PostSort plus Preproject admission");
            assert_eq!(slot.fallback(), PlanId::CpuPostSort);
            assert_eq!(
                slot.eligible(),
                &[
                    PlanId::CpuPostSort,
                    PlanId::GpuPostSort,
                    PlanId::GpuPreproject
                ]
            );
            assert!(receipt.preproject_compute());
            assert_eq!(receipt.source_count(), count as u32);
            assert_eq!(receipt.capacity(), count as u32);
            assert_eq!(receipt.resident_count(), count as u32);
            assert_eq!(receipt.addressable_count(), count as u32);
            assert_eq!(receipt.sh_degree(), sh_degree);

            let viewport = Viewport::new(127, 65).expect("viewport");
            let before_wrong_owner = slot.frame_state();
            let wrong_queue = Arc::new(queue.as_ref().clone());
            let mut wrong_owner_encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("wrong-owner-gpu-preproject-plan-test-encoder"),
                });
            assert!(matches!(
                execute_frame_gpu(
                    &mut slot,
                    PlanId::GpuPreproject,
                    &camera(),
                    viewport,
                    &wrong_queue,
                    &mut wrong_owner_encoder,
                ),
                Err(FrameExecutionError::GpuPreparation(
                    GpuPreparationError::ExecutionOwnerMismatch
                ))
            ));
            drop(wrong_owner_encoder.finish());
            assert_eq!(slot.frame_state(), before_wrong_owner);
            assert_eq!(slot.scene().gpu_preproject_encode_count(), Some(0));

            let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mut discarded = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("discarded-gpu-preproject-plan-test-encoder"),
            });
            let work = execute_frame_gpu(
                &mut slot,
                PlanId::GpuPreproject,
                &camera(),
                viewport,
                &queue,
                &mut discarded,
            )
            .expect("GPU Preproject projected work");
            assert_eq!(work.plan_id(), PlanId::GpuPreproject);
            assert_eq!(work.order_lane(), OrderLane::Gpu);
            assert_eq!(work.source_count(), count as u32);
            assert_eq!(work.visible_count(), Err(WorkUnavailable::VisibleCount));
            assert_eq!(
                work.contributor_count(),
                Err(WorkUnavailable::ContributorCount)
            );
            assert_eq!(work.draw_count(), Err(WorkUnavailable::DrawCount));
            assert_eq!(work.cpu_order_ids(), Err(WorkUnavailable::CpuOrderIds));
            let first_generation = work.order_generation();
            let projected = work
                .gpu_preproject()
                .expect("canonical GPU Preproject work");
            assert_eq!(projected.frame_identity(), work.frame_identity());
            assert_eq!(projected.camera(), camera());
            assert_eq!(projected.viewport(), (127, 65));
            assert_eq!(projected.source_count(), count as u32);
            assert_eq!(projected.sh_degree(), sh_degree);
            assert_eq!(projected.receipt(), receipt);
            assert_eq!(
                projected.count_semantics(),
                IndirectCountSemantics::DrawEqualsContributor
            );
            let accepted_owner = projected.owner.clone();
            assert!(projected.same_owner(&accepted_owner));
            assert!(!projected.same_owner(&GpuOwnerToken::fresh()));
            assert_eq!(projected.indirect_args().size(), 16);
            assert!(projected.ordered_source_ids().size() >= (count.max(1) * 4) as u64);
            assert!(projected.projected_center_alpha_key().size() >= 16);
            assert!(projected.projected_axes().size() >= 16);
            assert!(projected.resolved_color().size() >= 4);
            assert_eq!(projected.candidate_count().offset() % 4, 0);
            assert_eq!(projected.contributor_count().offset() % 4, 0);
            assert!(
                projected.candidate_count().offset() + 4
                    <= projected.candidate_count().buffer().size()
            );
            assert!(
                projected.contributor_count().offset() + 4
                    <= projected.contributor_count().buffer().size()
            );
            drop(work);
            drop(discarded.finish());
            assert_eq!(slot.scene().gpu_color_encode_count(), Some(1));
            assert_eq!(slot.scene().gpu_preproject_encode_count(), Some(1));

            let mut retry = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("retried-gpu-preproject-plan-test-encoder"),
            });
            let retried = execute_frame_gpu(
                &mut slot,
                PlanId::GpuPreproject,
                &camera(),
                viewport,
                &queue,
                &mut retry,
            )
            .expect("discarded encoder requires complete Preproject retry");
            assert_eq!(retried.order_generation(), first_generation + 1);
            assert_eq!(
                retried
                    .gpu_preproject()
                    .expect("retried Preproject work")
                    .count_semantics(),
                IndirectCountSemantics::DrawEqualsContributor
            );
            drop(retried);
            drop(retry.finish());
            assert_eq!(slot.scene().gpu_color_encode_count(), Some(2));
            assert_eq!(slot.scene().gpu_preproject_encode_count(), Some(2));

            if count == 1_025 {
                let before_frame = slot.frame_state();
                let before_preproject_encodes = slot.scene().gpu_preproject_encode_count();
                let mut invalid_cameras = [camera(), camera(), camera()];
                invalid_cameras[0].intrinsics.near_plane = 4.0;
                invalid_cameras[0].intrinsics.far_plane = 1.0;
                invalid_cameras[1].intrinsics.vertical_fov_radians = 0.0;
                invalid_cameras[2].pose.rotation_xyzw = [0.0; 4];
                for invalid_camera in invalid_cameras {
                    let mut invalid_encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("invalid-camera-gpu-preproject-plan-test-encoder"),
                        });
                    assert!(matches!(
                        execute_frame_gpu(
                            &mut slot,
                            PlanId::GpuPreproject,
                            &invalid_camera,
                            viewport,
                            &queue,
                            &mut invalid_encoder,
                        ),
                        Err(FrameExecutionError::PlanSet(PlanSetError::GpuPreproject(
                            GpuPreprojectError::FrameContractMismatch {
                                component: "camera"
                            }
                        )))
                    ));
                    drop(invalid_encoder.finish());
                    assert_eq!(slot.frame_state(), before_frame);
                    assert_eq!(slot.fallback(), PlanId::CpuPostSort);
                    assert_eq!(
                        slot.scene().gpu_preproject_encode_count(),
                        before_preproject_encodes
                    );
                }
            }
            assert!(
                validation.pop().await.is_none(),
                "GPU Preproject plan encoding must be validation-clean"
            );
        }
    });
}
