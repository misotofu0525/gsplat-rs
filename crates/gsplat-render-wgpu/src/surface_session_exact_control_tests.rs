//! Exact native Surface control-boundary regressions.
//!
//! The fixture exercises the same crate-private exact branches used by the
//! public session APIs, so production keeps one concrete `SurfacePresenter`
//! type and these tests need neither a fake swapchain nor a test-only wrapper.

use std::sync::Arc;

use gsplat_core::{Camera, RendererConfig, SceneBuffers, Vec3f};

use super::{SurfaceGpuOrderProducer, SurfaceOrderBackend, SurfaceProjectedDrawPolicy};
use crate::cpu_order::DepthKeyPrecision;
use crate::plans::{PlanId, TestGpuAdmissionMode};
use crate::renderer::{ExactPlanPolicy, PreparedRuntimeSlot, execute_frame};
use crate::surface::{
    ExactSurfacePlanState, commit_exact_plan_state, exact_gpu_plan_for_producer,
    prepare_exact_gpu_order, prepare_exact_gpu_order_producer,
};
use crate::{GeometryPath, Renderer, RendererError, ResidentSceneCpu, SurfacePresenterError};

fn exact_control_scene() -> ResidentSceneCpu {
    ResidentSceneCpu::encode_owned(SceneBuffers {
        positions: vec![Vec3f::new(0.0, 0.0, 1.0)],
        opacity: vec![1.0],
        scale_xyz: vec![[-1.0; 3]],
        rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
        color_dc: vec![[0.1, 0.2, 0.3]],
        sh_degree: 0,
        sh_rest: None,
    })
    .expect("Exact Surface control fixture")
}

fn depth_precision_scene() -> ResidentSceneCpu {
    ResidentSceneCpu::encode_owned(SceneBuffers {
        positions: vec![
            Vec3f::new(0.0, 0.0, f32::from_bits(1.0_f32.to_bits() + 1)),
            Vec3f::new(0.0, 0.0, f32::from_bits(1.0_f32.to_bits() + 255)),
        ],
        opacity: vec![1.0; 2],
        scale_xyz: vec![[-1.0; 3]; 2],
        rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 2],
        color_dc: vec![[0.1, 0.2, 0.3]; 2],
        sh_degree: 0,
        sh_rest: None,
    })
    .expect("depth precision fixture")
}

fn exact_control_limits() -> wgpu::Limits {
    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_storage_buffers_per_shader_stage =
        crate::resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS;
    limits.max_storage_buffer_binding_size = 128 << 20;
    limits.max_buffer_size = 128 << 20;
    limits
}

async fn exact_control_device() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    #[cfg(target_os = "macos")]
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..Default::default()
    });
    #[cfg(not(target_os = "macos"))]
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
        #[cfg(target_os = "macos")]
        Err(error) => panic!("required Exact Surface Metal adapter unavailable: {error}"),
        #[cfg(not(target_os = "macos"))]
        Err(error) => {
            eprintln!("skipping optional Exact Surface control test: {error}");
            return None;
        }
    };
    #[cfg(target_os = "macos")]
    assert_eq!(adapter.get_info().backend, wgpu::Backend::Metal);
    let limits = exact_control_limits();
    if !limits.check_limits(&adapter.limits()) {
        #[cfg(target_os = "macos")]
        panic!("required Exact Surface Metal limits unavailable: {limits:?}");
        #[cfg(not(target_os = "macos"))]
        return None;
    }
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("exact-surface-control-test-device"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    };
    match adapter.request_device(&descriptor).await {
        Ok((device, queue)) => Some((Arc::new(device), Arc::new(queue))),
        #[cfg(target_os = "macos")]
        Err(error) => panic!("required Exact Surface Metal device unavailable: {error}"),
        #[cfg(not(target_os = "macos"))]
        Err(error) => {
            eprintln!("skipping optional Exact Surface control test: {error}");
            None
        }
    }
}

async fn exact_control_renderer(
    indirect_execution_supported: bool,
) -> Option<(Renderer, Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    let (device, queue) = exact_control_device().await?;
    let mut renderer = Renderer::with_config_for_surface(RendererConfig {
        width: 64,
        height: 64,
        ..RendererConfig::default()
    })
    .expect("Surface-only renderer");
    renderer.set_geometry_path(GeometryPath::PackedAtlas);
    renderer
        .load_resident_scene(exact_control_scene())
        .expect("retained Exact Surface source");
    let source = renderer
        .resident_scene()
        .expect("retained Exact Surface source");
    let mut candidate = PreparedRuntimeSlot::prepare_surface_candidate(source)
        .expect("CPU Exact Surface candidate");
    if indirect_execution_supported {
        candidate.set_test_gpu_admission_mode(TestGpuAdmissionMode::ConcreteAll);
    }
    candidate
        .prepare_gpu_from_source(
            source,
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            indirect_execution_supported,
        )
        .await
        .expect("complete Exact Surface candidate");
    candidate.seed_surface_frame_baseline(
        Camera::default(),
        crate::renderer::frame::Viewport::new(64, 64).expect("test viewport"),
    );
    renderer
        .publish_surface_exact_candidate(candidate)
        .expect("publish Exact Surface test runtime");
    Some((renderer, device, queue))
}

#[test]
fn surface_cpu_plan_carries_candidate_precision_without_changing_exact_default() {
    let viewport = crate::renderer::frame::Viewport::new(64, 64).expect("viewport");
    let camera = Camera::default();
    let mut exact = PreparedRuntimeSlot::prepare(depth_precision_scene()).expect("Exact runtime");
    assert_eq!(
        exact.depth_key_precision_for_test(),
        DepthKeyPrecision::ExactFull32
    );
    let exact_ids = execute_frame(&mut exact, PlanId::CpuPostSort, &camera, viewport)
        .expect("Exact CPU frame")
        .cpu_order_ids()
        .expect("Exact CPU IDs")
        .to_vec();

    let mut candidate = PreparedRuntimeSlot::prepare_with_depth_key_precision(
        depth_precision_scene(),
        DepthKeyPrecision::CandidateStable24,
    )
    .expect("Candidate runtime");
    let candidate_ids = execute_frame(&mut candidate, PlanId::CpuPostSort, &camera, viewport)
        .expect("Candidate CPU frame")
        .cpu_order_ids()
        .expect("Candidate CPU IDs")
        .to_vec();

    assert_eq!(exact_ids, [1, 0]);
    assert_eq!(candidate_ids, [0, 1]);
    assert_eq!(exact_ids.len(), candidate_ids.len());
    assert_eq!(candidate.scene().source_count(), 2);

    candidate
        .replace(depth_precision_scene())
        .expect("Candidate replacement");
    assert_eq!(
        candidate.depth_key_precision_for_test(),
        DepthKeyPrecision::CandidateStable24
    );
}

#[test]
fn surface_gpu_candidate_carries_the_same_candidate_precision_to_resident_order() {
    pollster::block_on(async {
        let Some((device, queue)) = exact_control_device().await else {
            return;
        };
        let source = depth_precision_scene();
        let candidate =
            PreparedRuntimeSlot::prepare_complete_surface_gpu_candidate_with_depth_key_precision(
                &source,
                &device,
                &queue,
                wgpu::TextureFormat::Rgba8Unorm,
                true,
                DepthKeyPrecision::CandidateStable24,
            )
            .await
            .expect("Candidate Surface GPU runtime");

        assert_eq!(
            candidate.depth_key_precision_for_test(),
            DepthKeyPrecision::CandidateStable24
        );
        assert_eq!(
            candidate.gpu_depth_key_precision_for_test(),
            Some(DepthKeyPrecision::CandidateStable24)
        );
    });
}

#[derive(Debug, PartialEq)]
struct GpuPreparationSnapshot {
    source_count: u32,
    capacity: u32,
    resident_count: u32,
    addressable_count: u32,
    sh_degree: u8,
    preproject_compute: bool,
    scene_generation: u64,
    contract_generation: u64,
    plan_set_generation: u64,
}

#[derive(Debug, PartialEq)]
struct ExactControlSnapshot {
    geometry_path: GeometryPath,
    scene_len: Option<usize>,
    positions_allocation: Option<usize>,
    runtime_allocation: usize,
    runtime_frame: crate::renderer::frame::FrameState,
    runtime_policy: ExactPlanPolicy,
    eligible: Vec<PlanId>,
    fallback: PlanId,
    gpu_preparation: Option<GpuPreparationSnapshot>,
    last_published_plan: Option<PlanId>,
    last_cpu_order: Option<Vec<u32>>,
    cpu_order_generation: Option<u64>,
}

#[derive(Debug, PartialEq)]
struct ExactSessionBoundarySnapshot {
    renderer: ExactControlSnapshot,
    plan_receipt: Option<ExactSurfacePlanState>,
    order_backend: SurfaceOrderBackend,
    projected_draw_policy: SurfaceProjectedDrawPolicy,
    projected_draw_policy_requested: SurfaceProjectedDrawPolicy,
}

struct ExactSessionBoundary {
    renderer: Renderer,
    plan_receipt: Option<ExactSurfacePlanState>,
    order_backend: SurfaceOrderBackend,
    projected_draw_policy: SurfaceProjectedDrawPolicy,
    projected_draw_policy_requested: SurfaceProjectedDrawPolicy,
}

impl ExactSessionBoundary {
    fn new(renderer: Renderer) -> Self {
        let state = ExactSurfacePlanState::from_policy(
            renderer
                .exact_surface_policy()
                .expect("Exact Surface policy"),
        );
        Self {
            renderer,
            plan_receipt: Some(state),
            order_backend: state.order_backend(),
            projected_draw_policy: state.projected_policy(),
            projected_draw_policy_requested: SurfaceProjectedDrawPolicy::Adaptive,
        }
    }

    fn current_state(&self) -> ExactSurfacePlanState {
        let state = ExactSurfacePlanState::from_policy(
            self.renderer
                .exact_surface_policy()
                .expect("Exact Surface policy"),
        );
        assert_eq!(self.plan_receipt, Some(state));
        state
    }

    fn prepare_gpu_order_producer(
        &self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), RendererError> {
        prepare_exact_gpu_order_producer(&self.renderer, producer)
    }

    fn prepare_gpu_order(&self) -> Result<(), RendererError> {
        prepare_exact_gpu_order(&self.renderer, self.current_state())
    }

    fn set_order_backend(&mut self, backend: SurfaceOrderBackend) -> Result<(), RendererError> {
        let next = self.current_state().with_order_backend(backend);
        self.commit(next)
    }

    fn set_projected_draw_policy(
        &mut self,
        policy: SurfaceProjectedDrawPolicy,
    ) -> Result<(), RendererError> {
        let next = self.current_state().with_projected_policy(policy)?;
        self.commit(next)?;
        self.projected_draw_policy_requested = policy;
        Ok(())
    }

    fn set_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), RendererError> {
        let next = self.current_state().with_producer(producer)?;
        self.commit(next)
    }

    fn set_geometry_path(&mut self, path: GeometryPath) -> Result<(), RendererError> {
        let next = self.current_state().with_geometry(path)?;
        self.commit(next)
    }

    fn commit(&mut self, next: ExactSurfacePlanState) -> Result<(), RendererError> {
        commit_exact_plan_state(
            &mut self.renderer,
            &mut self.plan_receipt,
            &mut self.order_backend,
            &mut self.projected_draw_policy,
            next,
        )
    }

    fn snapshot(&self) -> ExactSessionBoundarySnapshot {
        ExactSessionBoundarySnapshot {
            renderer: exact_control_snapshot(&self.renderer),
            plan_receipt: self.plan_receipt,
            order_backend: self.order_backend,
            projected_draw_policy: self.projected_draw_policy,
            projected_draw_policy_requested: self.projected_draw_policy_requested,
        }
    }
}

fn exact_control_snapshot(renderer: &Renderer) -> ExactControlSnapshot {
    let runtime = renderer
        .exact_offscreen_runtime
        .as_ref()
        .expect("Exact Surface runtime");
    ExactControlSnapshot {
        geometry_path: renderer.geometry_path(),
        scene_len: renderer.scene_len(),
        positions_allocation: renderer
            .positions()
            .map(|positions| positions.as_ptr() as usize),
        runtime_allocation: std::ptr::from_ref(runtime) as usize,
        runtime_frame: runtime.frame_state(),
        runtime_policy: runtime.active_policy(),
        eligible: runtime.eligible().to_vec(),
        fallback: runtime.fallback(),
        gpu_preparation: runtime
            .gpu_preparation()
            .map(|receipt| GpuPreparationSnapshot {
                source_count: receipt.source_count(),
                capacity: receipt.capacity(),
                resident_count: receipt.resident_count(),
                addressable_count: receipt.addressable_count(),
                sh_degree: receipt.sh_degree(),
                preproject_compute: receipt.preproject_compute(),
                scene_generation: receipt.scene_generation(),
                contract_generation: receipt.contract_generation(),
                plan_set_generation: receipt.plan_set_generation(),
            }),
        last_published_plan: runtime.last_published_plan(),
        last_cpu_order: runtime.last_usable_cpu_order().map(<[u32]>::to_vec),
        cpu_order_generation: runtime.current_cpu_order_generation(),
    }
}

#[test]
fn packed_exact_surface_geometry_switch_rejects_atomically_and_same_path_is_idempotent() {
    pollster::block_on(async {
        let Some((renderer, _device, _queue)) = exact_control_renderer(false).await else {
            return;
        };
        let mut session = ExactSessionBoundary::new(renderer);
        let before = session.snapshot();

        session
            .set_geometry_path(GeometryPath::PackedAtlas)
            .expect("same-path Packed setter is idempotent");
        assert_eq!(session.snapshot(), before);

        let result = session.set_geometry_path(GeometryPath::SortedIndexDirect);
        assert!(matches!(
            result,
            Err(RendererError::SurfacePresenter(
                SurfacePresenterError::SurfaceGeometrySwitchUnsupported
            ))
        ));
        assert_eq!(
            session.snapshot(),
            before,
            "rejected Packed -> Direct retains scene allocation, GPU preparation, current plan, and published frame state"
        );
    });
}

fn assert_gpu_order_unsupported(result: Result<(), RendererError>) {
    assert!(matches!(
        result,
        Err(RendererError::SurfacePresenter(
            SurfacePresenterError::GpuOrderUnsupported
        ))
    ));
}

#[test]
fn cpu_only_exact_surface_prepare_boundary_fails_closed_without_mutation() {
    pollster::block_on(async {
        let Some((renderer, _device, _queue)) = exact_control_renderer(false).await else {
            return;
        };
        let session = ExactSessionBoundary::new(renderer);
        let before = session.snapshot();
        assert_eq!(before.renderer.eligible, [PlanId::CpuPostSort]);

        // prepare_gpu_order_producer(PostSort) and the default
        // prepare_gpu_order() both request the same canonical GPU PostSort
        // plan. Neither may report a CPU-only Exact runtime as GPU-ready.
        assert_eq!(
            exact_gpu_plan_for_producer(ExactSurfacePlanState::CpuPostSort.producer()),
            PlanId::GpuPostSort
        );
        assert_gpu_order_unsupported(
            session.prepare_gpu_order_producer(SurfaceGpuOrderProducer::PostSort),
        );
        assert_eq!(session.snapshot(), before);

        assert_gpu_order_unsupported(session.prepare_gpu_order());
        assert_eq!(session.snapshot(), before);

        assert_gpu_order_unsupported(
            session.prepare_gpu_order_producer(SurfaceGpuOrderProducer::Preproject),
        );
        assert_eq!(session.snapshot(), before);
    });
}

#[test]
fn cpu_only_exact_surface_forced_gpu_rejects_transactionally_and_adaptive_never_probes() {
    pollster::block_on(async {
        let Some((renderer, _device, _queue)) = exact_control_renderer(false).await else {
            return;
        };
        let mut session = ExactSessionBoundary::new(renderer);
        let before = session.snapshot();

        assert_gpu_order_unsupported(session.set_order_backend(SurfaceOrderBackend::Gpu));
        assert_eq!(session.snapshot(), before);
        assert_gpu_order_unsupported(
            session
                .renderer
                .set_exact_surface_policy(ExactPlanPolicy::Forced(PlanId::GpuPreproject)),
        );
        assert_eq!(session.snapshot(), before);
        assert!(
            ExactSurfacePlanState::CpuPostSort
                .with_producer(SurfaceGpuOrderProducer::Preproject)
                .is_err()
        );
        assert!(
            ExactSurfacePlanState::CpuPostSort
                .with_projected_policy(SurfaceProjectedDrawPolicy::Compact)
                .is_err()
        );

        session
            .set_order_backend(SurfaceOrderBackend::Adaptive)
            .expect("single-plan Exact Adaptive remains supported");
        assert_eq!(
            session.renderer.exact_surface_adaptive_state(),
            Some(crate::renderer::ExactAdaptivePolicyState::CpuStable)
        );
        let runtime = session
            .renderer
            .exact_offscreen_runtime
            .as_mut()
            .expect("Exact Surface runtime");
        let probe_generation = runtime.adaptive_probe_generation_for_test();
        for _ in 0..256 {
            assert_eq!(
                runtime
                    .complete_adaptive_cpu_frame_for_test()
                    .expect("single eligible plan cannot exhaust probing"),
                (PlanId::CpuPostSort, false),
            );
        }
        assert_eq!(
            runtime.adaptive_probe_generation_for_test(),
            probe_generation,
            "a one-plan controller never starts a GPU probe"
        );
        assert_eq!(runtime.eligible(), [PlanId::CpuPostSort]);

        let after = session.snapshot();
        assert_eq!(after.renderer.runtime_frame, before.renderer.runtime_frame);
        assert_eq!(
            after.renderer.gpu_preparation,
            before.renderer.gpu_preparation
        );
        assert_eq!(
            after.renderer.last_published_plan,
            before.renderer.last_published_plan
        );
        assert_eq!(
            after.renderer.last_cpu_order,
            before.renderer.last_cpu_order
        );
        assert_eq!(
            after.renderer.cpu_order_generation,
            before.renderer.cpu_order_generation
        );
    });
}

#[test]
fn capable_exact_surface_prepare_boundary_retains_all_three_plans_and_setters() {
    pollster::block_on(async {
        let Some((renderer, _device, _queue)) = exact_control_renderer(true).await else {
            return;
        };
        let mut session = ExactSessionBoundary::new(renderer);
        let before = session.snapshot();
        assert_eq!(
            before.renderer.eligible,
            [
                PlanId::CpuPostSort,
                PlanId::GpuPostSort,
                PlanId::GpuPreproject,
            ]
        );

        session
            .prepare_gpu_order()
            .expect("capable PostSort plan was prepared at construction");
        session
            .prepare_gpu_order_producer(SurfaceGpuOrderProducer::Preproject)
            .expect("capable Preproject plan was prepared at construction");
        assert_eq!(session.snapshot(), before);

        session
            .set_order_backend(SurfaceOrderBackend::Gpu)
            .expect("force capable GPU PostSort");
        session
            .set_projected_draw_policy(SurfaceProjectedDrawPolicy::Adaptive)
            .expect("retain configured Adaptive projected policy");
        let adaptive_requested = session.snapshot();
        assert_eq!(
            adaptive_requested.plan_receipt,
            Some(ExactSurfacePlanState::GpuPostSort)
        );
        assert_eq!(
            adaptive_requested.projected_draw_policy,
            SurfaceProjectedDrawPolicy::Candidate,
            "the forced GPU plan still executes Candidate"
        );
        assert_eq!(
            adaptive_requested.projected_draw_policy_requested,
            SurfaceProjectedDrawPolicy::Adaptive,
            "the public receipt preserves the configured policy"
        );
        session
            .set_gpu_order_producer(SurfaceGpuOrderProducer::Preproject)
            .expect("force capable GPU Preproject");
        session
            .set_projected_draw_policy(SurfaceProjectedDrawPolicy::Candidate)
            .expect("return capable GPU PostSort");
        session
            .set_order_backend(SurfaceOrderBackend::Adaptive)
            .expect("capable whole-plan Adaptive");
        assert_eq!(
            ExactSurfacePlanState::GpuPostSort
                .with_projected_policy(SurfaceProjectedDrawPolicy::Compact)
                .expect("canonical capable transition"),
            ExactSurfacePlanState::GpuPreproject
        );
        assert_eq!(
            ExactSurfacePlanState::GpuPreproject.with_order_backend(SurfaceOrderBackend::Gpu),
            ExactSurfacePlanState::GpuPreproject
        );
        assert_eq!(
            session.snapshot().renderer.eligible,
            before.renderer.eligible,
            "policy changes never rebuild or narrow the admitted plan set"
        );
    });
}
