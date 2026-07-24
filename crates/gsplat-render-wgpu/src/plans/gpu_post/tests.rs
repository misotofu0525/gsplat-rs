use std::sync::Arc;

use gsplat_core::{Camera, SceneBuffers, Vec3f};

use super::*;
use crate::plans::{
    FrameIdentity, GpuCapabilityReceipt, IndirectCountSemantics, OrderLane, PlanId, PlanSetError,
    TestGpuAdmissionMode, WorkUnavailable,
};
use crate::renderer::frame::Viewport;
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
    // Test fixture only. Production GpuPostSortPlan receives the renderer's
    // E8a-owned token/device queue and never requests an adapter or device.
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
        Err(error) => {
            eprintln!("skipping GPU PostSort plan test; adapter unavailable: {error}");
            return None;
        }
    };
    let limits = portable_limits();
    if !limits.check_limits(&adapter.limits()) {
        eprintln!("skipping GPU PostSort plan test; portable limits unavailable");
        return None;
    }
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("exact-gpu-post-plan-test-device"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    };
    let info = adapter.get_info();
    match adapter.request_device(&descriptor).await {
        Ok((device, queue)) => Some((instance, adapter, info, Arc::new(device), Arc::new(queue))),
        Err(error) => {
            eprintln!("skipping GPU PostSort plan test; device unavailable: {error}");
            None
        }
    }
}

#[test]
fn admission_rejects_incomplete_counts_and_sh_downgrade() {
    let mut mismatched = capability(1, 3);
    mismatched.addressable_count = 0;
    assert_eq!(
        GpuPostSortPlan::prepare(mismatched).map(|_| ()),
        Err(GpuPostSortError::AdmissionContractMismatch {
            component: "source/capacity/resident/addressable count"
        })
    );

    let mut invalid_sh = capability(1, 3);
    invalid_sh.sh_degree = 4;
    assert_eq!(
        GpuPostSortPlan::prepare(invalid_sh).map(|_| ()),
        Err(GpuPostSortError::AdmissionContractMismatch {
            component: "SH degree"
        })
    );
}

#[test]
fn frame_preflight_guards_count_sh_generations_and_gpu_graph() {
    let scene = runtime(&[2.0], 0);
    let plan = GpuPostSortPlan::prepare(capability(1, 0)).expect("plan");
    let base = GpuPostSortGuard {
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
            GpuPostSortGuard {
                source_count: 0,
                ..base
            }
        ),
        Err(GpuPostSortError::FrameContractMismatch {
            component: "source count"
        })
    );

    let sh_plan = GpuPostSortPlan::prepare(capability(1, 1)).expect("SH1 plan");
    assert_eq!(
        sh_plan.validate_frame(&scene, base),
        Err(GpuPostSortError::FrameContractMismatch {
            component: "SH degree"
        })
    );

    assert_eq!(
        plan.validate_frame(
            &scene,
            GpuPostSortGuard {
                frame: FrameIdentity::new(2, 1, 1, 1, 2),
                ..base
            }
        ),
        Err(GpuPostSortError::FrameContractMismatch {
            component: "scene/contract/plan-set generation"
        })
    );
    assert_eq!(
        plan.validate_frame(&scene, base),
        Err(GpuPostSortError::FrameContractMismatch {
            component: "device-owned scene graph"
        })
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_gpu_post_is_exact_current_indirect_and_discard_safe() {
    pollster::block_on(async {
        let Some((_instance, _adapter, info, device, queue)) = request_device().await else {
            return;
        };
        eprintln!(
            "EXACT_GPU_POST_PLAN adapter={} backend={:?}",
            info.name, info.backend
        );
        #[cfg(target_os = "macos")]
        assert_eq!(info.backend, wgpu::Backend::Metal, "Metal adapter required");

        for (count, sh_degree) in [(0, 0), (1, 0), (1, 1), (1, 2), (257, 3)] {
            let depths = adversarial_depths(count);
            let resident =
                ResidentSceneCpu::encode_owned(source(&depths, sh_degree)).expect("scene");
            let mut slot = PreparedRuntimeSlot::prepare(resident).expect("CPU fallback");
            slot.set_test_gpu_admission_mode(TestGpuAdmissionMode::Concrete);
            let receipt = slot
                .prepare_gpu(&device, &queue, wgpu::TextureFormat::Rgba8Unorm)
                .await
                .expect("atomic concrete GPU PostSort admission");
            assert_eq!(slot.fallback(), PlanId::CpuPostSort);
            assert_eq!(slot.eligible(), &[PlanId::CpuPostSort, PlanId::GpuPostSort]);
            assert_eq!(receipt.source_count(), count as u32);
            assert_eq!(receipt.capacity(), count as u32);
            assert_eq!(receipt.resident_count(), count as u32);
            assert_eq!(receipt.addressable_count(), count as u32);
            assert_eq!(receipt.sh_degree(), sh_degree);

            let viewport = Viewport::new(96, 64).expect("viewport");
            let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mut discarded = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("discarded-gpu-post-plan-test-encoder"),
            });
            let work = execute_frame_gpu(
                &mut slot,
                PlanId::GpuPostSort,
                &camera(),
                viewport,
                &queue,
                &mut discarded,
            )
            .expect("GPU PostSort projected work");
            assert_eq!(work.plan_id(), PlanId::GpuPostSort);
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
            let projected = work.gpu_post().expect("rank-indexed GPU work");
            assert_eq!(projected.frame_identity(), work.frame_identity());
            assert_eq!(projected.camera(), camera());
            assert_eq!(projected.viewport(), (96, 64));
            assert_eq!(projected.source_count(), count as u32);
            assert_eq!(projected.sh_degree(), sh_degree);
            assert_eq!(projected.receipt(), receipt);
            assert_eq!(
                projected.count_semantics(),
                IndirectCountSemantics::DrawEqualsVisible
            );
            let accepted_owner = projected.owner.clone();
            assert!(projected.same_owner(&accepted_owner));
            assert!(!projected.same_owner(&GpuOwnerToken::fresh()));
            assert_eq!(projected.indirect_args().size(), 16);
            assert!(projected.ordered_source_ids().size() >= (count.max(1) * 4) as u64);
            assert!(projected.projected_center_source().size() >= 16);
            assert!(projected.projected_axes().size() >= 16);
            assert!(projected.resolved_color().size() >= 4);
            drop(work);
            drop(discarded.finish());
            assert_eq!(slot.scene().gpu_color_encode_count(), Some(1));

            let mut retry = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("retried-gpu-post-plan-test-encoder"),
            });
            let retried = execute_frame_gpu(
                &mut slot,
                PlanId::GpuPostSort,
                &camera(),
                viewport,
                &queue,
                &mut retry,
            )
            .expect("same guard must re-encode after discarded encoder");
            let retried_generation = retried.order_generation();
            assert_eq!(retried_generation, first_generation + 1);
            assert_eq!(
                retried
                    .gpu_post()
                    .expect("retried GPU work")
                    .count_semantics(),
                IndirectCountSemantics::DrawEqualsVisible
            );
            drop(retried);
            drop(retry.finish());
            assert_eq!(slot.scene().gpu_color_encode_count(), Some(2));

            if count == 257 {
                let before_frame = slot.frame_state();
                let before_color_encodes = slot.scene().gpu_color_encode_count();
                let mut invalid_cameras = [camera(), camera(), camera()];
                invalid_cameras[0].intrinsics.near_plane = 4.0;
                invalid_cameras[0].intrinsics.far_plane = 1.0;
                invalid_cameras[1].intrinsics.vertical_fov_radians = 0.0;
                invalid_cameras[2].pose.rotation_xyzw = [0.0; 4];
                for invalid_camera in invalid_cameras {
                    let mut invalid_encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("invalid-camera-gpu-post-plan-test-encoder"),
                        });
                    assert!(matches!(
                        execute_frame_gpu(
                            &mut slot,
                            PlanId::GpuPostSort,
                            &invalid_camera,
                            viewport,
                            &queue,
                            &mut invalid_encoder,
                        ),
                        Err(FrameExecutionError::PlanSet(PlanSetError::GpuPostSort(
                            GpuPostSortError::FrameContractMismatch {
                                component: "camera"
                            }
                        )))
                    ));
                    drop(invalid_encoder.finish());
                    assert_eq!(slot.frame_state(), before_frame);
                    assert_eq!(slot.fallback(), PlanId::CpuPostSort);
                    assert_eq!(slot.eligible(), &[PlanId::CpuPostSort, PlanId::GpuPostSort]);
                    assert_eq!(slot.scene().gpu_color_encode_count(), before_color_encodes);
                }

                let mut after_invalid =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("post-invalid-camera-gpu-post-plan-test-encoder"),
                    });
                let recovered = execute_frame_gpu(
                    &mut slot,
                    PlanId::GpuPostSort,
                    &camera(),
                    viewport,
                    &queue,
                    &mut after_invalid,
                )
                .expect("valid retry after invalid cameras");
                assert_eq!(recovered.order_generation(), retried_generation + 1);
                drop(recovered);
                drop(after_invalid.finish());
                assert_eq!(slot.scene().gpu_color_encode_count(), Some(3));
            }
            assert!(
                validation.pop().await.is_none(),
                "GPU PostSort encoding must be validation-clean"
            );
        }
    });
}
