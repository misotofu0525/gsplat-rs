#[cfg(not(target_os = "macos"))]
fn main() {
    println!(
        "M7D_SURFACE_GEOMETRY_ENTRY=SKIPPED target={} reason=requires-macos-metal-window-surface",
        std::env::consts::OS
    );
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;

    use gsplat_core::{Camera, ErrorCode, FrameStats, RendererConfig, SceneBuffers, Vec3f};
    use gsplat_render_wgpu::{
        GeometryPath, Renderer, RendererError, SurfaceCurrentStatsSubmission,
        SurfaceGpuOrderProducer, SurfaceOrderBackend, SurfacePresenter, SurfacePresenterError,
        SurfaceProjectedDrawPolicy, SurfaceRasterExecutionPlan, SurfaceRenderSession,
    };
    use winit::{
        dpi::PhysicalSize,
        event_loop::EventLoop,
        window::{Window, WindowAttributes},
    };

    const TEST_WIDTH: u32 = 64;
    const TEST_HEIGHT: u32 = 64;

    enum HarnessError {
        Skipped(String),
        Failed(String),
    }

    #[derive(Debug, PartialEq)]
    struct PresenterSnapshot {
        geometry_path: GeometryPath,
        surface_size: (u32, u32),
        addressable_splat_count: usize,
        instance_count: u32,
    }

    #[derive(Debug, PartialEq)]
    struct SessionSnapshot {
        geometry_path: GeometryPath,
        renderer_geometry_path: GeometryPath,
        scene_len: Option<usize>,
        scene_allocation: Option<usize>,
        resident_allocation: Option<usize>,
        positions_allocation: Option<usize>,
        sorted_indices: Vec<u32>,
        last_stats: FrameStats,
        order_backend: SurfaceOrderBackend,
        projected_draw_policy: SurfaceProjectedDrawPolicy,
        gpu_order_producer: SurfaceGpuOrderProducer,
        raster_execution_plan: SurfaceRasterExecutionPlan,
        current_stats_submission: SurfaceCurrentStatsSubmission,
        surface_size: (u32, u32),
    }

    pub(super) fn main() {
        match run() {
            Ok(adapter) => println!(
                "M7D_SURFACE_GEOMETRY_ENTRY=PASS backend=Metal adapter={adapter:?} public_presenter=true public_session=true standalone_packed_rejected=true product_packed_host=true"
            ),
            Err(HarnessError::Skipped(reason)) => {
                println!("M7D_SURFACE_GEOMETRY_ENTRY=SKIPPED target=macos reason={reason}")
            }
            Err(HarnessError::Failed(reason)) => {
                panic!("M7d public Surface geometry entry regression failed: {reason}")
            }
        }
    }

    fn run() -> Result<String, HarnessError> {
        let adapter = require_metal_adapter()?;
        let event_loop = EventLoop::new().map_err(|error| {
            HarnessError::Skipped(format!("window-event-loop-unavailable:{error}"))
        })?;

        exercise_presenter_entries(&event_loop)?;
        exercise_session_entries(&event_loop)?;
        exercise_exact_packed_host_entry(&event_loop)?;
        Ok(adapter)
    }

    fn require_metal_adapter() -> Result<String, HarnessError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|error| HarnessError::Skipped(format!("metal-adapter-unavailable:{error}")))?;
        let info = adapter.get_info();
        if info.backend != wgpu::Backend::Metal {
            return Err(HarnessError::Failed(format!(
                "Metal-only probe selected unexpected backend {:?}",
                info.backend
            )));
        }
        Ok(info.name)
    }

    fn exercise_presenter_entries(event_loop: &EventLoop<()>) -> Result<(), HarnessError> {
        let direct_renderer = loaded_renderer(GeometryPath::SortedIndexDirect);
        let packed_renderer = loaded_renderer(GeometryPath::PackedAtlas);
        let paged_renderer = loaded_renderer(GeometryPath::PagedActiveAtlas);
        let empty_packed_renderer = empty_renderer(GeometryPath::PackedAtlas);

        let direct_window = hidden_window(event_loop, "M7d presenter Direct")?;
        let mut direct = presenter(direct_window, &direct_renderer)?;
        let direct_before = presenter_snapshot(&direct);

        direct
            .set_geometry_path(GeometryPath::SortedIndexDirect, &direct_renderer)
            .map_err(failed("public presenter same-path Direct"))?;
        assert_eq!(presenter_snapshot(&direct), direct_before);

        assert_presenter_switch_unsupported(
            direct.set_geometry_path(GeometryPath::PackedAtlas, &packed_renderer),
            "public presenter Direct -> Packed",
        );
        assert_eq!(presenter_snapshot(&direct), direct_before);
        assert_presenter_switch_unsupported(
            direct.set_geometry_path(GeometryPath::PackedAtlas, &empty_packed_renderer),
            "public presenter Direct -> Packed must reject before reading or preparing the target source",
        );
        assert_eq!(presenter_snapshot(&direct), direct_before);

        direct
            .set_geometry_path(GeometryPath::PagedActiveAtlas, &paged_renderer)
            .map_err(failed("public presenter Direct -> Paged"))?;
        assert_eq!(direct.geometry_path(), GeometryPath::PagedActiveAtlas);
        direct
            .set_geometry_path(GeometryPath::SortedIndexDirect, &direct_renderer)
            .map_err(failed("public presenter Paged -> Direct"))?;
        assert_eq!(direct.geometry_path(), GeometryPath::SortedIndexDirect);

        let paged_window = hidden_window(event_loop, "M7 standalone presenter Paged")?;
        let paged = presenter(paged_window, &paged_renderer)?;
        assert_eq!(paged.geometry_path(), GeometryPath::PagedActiveAtlas);

        let packed_window = hidden_window(event_loop, "M7 standalone presenter Packed")?;
        assert_standalone_packed_rejected(
            pollster::block_on(SurfacePresenter::from_window(
                packed_window.clone(),
                0,
                0,
                &packed_renderer,
            )),
            "standalone Packed admission must precede Surface size and graph preparation",
        );
        assert_standalone_packed_rejected(
            pollster::block_on(SurfacePresenter::from_window(
                packed_window,
                TEST_WIDTH,
                TEST_HEIGHT,
                &packed_renderer,
            )),
            "standalone Packed admission must be repeatable",
        );
        Ok(())
    }

    fn exercise_session_entries(event_loop: &EventLoop<()>) -> Result<(), HarnessError> {
        let direct_renderer = loaded_renderer(GeometryPath::SortedIndexDirect);
        let direct_window = hidden_window(event_loop, "M7d session Direct")?;
        let direct_presenter = presenter(direct_window, &direct_renderer)?;
        let mut direct =
            SurfaceRenderSession::new(direct_renderer, direct_presenter, Camera::default())
                .map_err(failed("public Direct session construction"))?;
        let direct_before = session_snapshot(&direct);

        direct
            .set_geometry_path(GeometryPath::SortedIndexDirect)
            .map_err(failed("public session same-path Direct"))?;
        assert_eq!(session_snapshot(&direct), direct_before);

        assert_session_switch_unsupported(
            direct.set_geometry_path(GeometryPath::PackedAtlas),
            "public session Direct -> Packed",
        );
        assert_eq!(
            session_snapshot(&direct),
            direct_before,
            "rejected public session Direct -> Packed must not prepare or publish renderer, presenter, plan, stats, or policy state"
        );

        direct
            .set_geometry_path(GeometryPath::PagedActiveAtlas)
            .map_err(failed("public session Direct -> Paged"))?;
        assert_eq!(direct.geometry_path(), GeometryPath::PagedActiveAtlas);
        direct
            .set_geometry_path(GeometryPath::SortedIndexDirect)
            .map_err(failed("public session Paged -> Direct"))?;
        assert_eq!(direct.geometry_path(), GeometryPath::SortedIndexDirect);

        Ok(())
    }

    fn exercise_exact_packed_host_entry(event_loop: &EventLoop<()>) -> Result<(), HarnessError> {
        let renderer = loaded_renderer(GeometryPath::PackedAtlas);
        let window = hidden_window(event_loop, "M7h-1 Exact Packed host")?;
        let mut session = pollster::block_on(SurfaceRenderSession::from_window(
            renderer,
            window,
            TEST_WIDTH,
            TEST_HEIGHT,
            Camera::default(),
        ))
        .map_err(failed("Exact Packed host session construction"))?;

        assert_eq!(session.geometry_path(), GeometryPath::PackedAtlas);
        assert_eq!(session.addressable_splat_count(), 3);
        assert_eq!(
            session.raster_execution_plan(),
            SurfaceRasterExecutionPlan::ProjectedQuadsExact
        );
        assert_eq!(session.internal_render_size(), (TEST_WIDTH, TEST_HEIGHT));
        session
            .resize(TEST_WIDTH, TEST_HEIGHT)
            .map_err(failed("Exact Packed host idempotent sync resize"))?;
        let producer = session.gpu_order_producer();
        pollster::block_on(session.prepare_gpu_order_producer(producer))
            .map_err(failed("Exact Packed host producer preparation"))?;
        session
            .set_gpu_order_producer(producer)
            .map_err(failed("Exact Packed host producer setter"))?;
        session
            .set_gpu_producer_measurement_enabled(false)
            .map_err(failed("Exact Packed host producer measurement setter"))?;
        session
            .set_projected_draw_policy(session.projected_draw_policy())
            .map_err(failed("Exact Packed host projected policy setter"))?;
        session
            .set_raster_execution_plan(SurfaceRasterExecutionPlan::ProjectedQuadsExact)
            .map_err(failed("Exact Packed host raster setter"))?;
        session
            .set_geometry_path(GeometryPath::PackedAtlas)
            .map_err(failed("Exact Packed host geometry setter"))?;
        session.poll_order_measurement_receipts();
        session.set_frame_latency(2);
        let frame = session
            .render_frame()
            .map_err(failed("Exact Packed host session render"))?;
        assert!(frame.frame_presented);
        assert_eq!(frame.stats.visible_count, 3);
        assert_eq!(frame.stats.drawn_count, 3);
        Ok(())
    }

    fn loaded_renderer(path: GeometryPath) -> Renderer {
        let mut renderer = empty_renderer(path);
        renderer
            .load_scene(test_scene())
            .expect("valid M7d Surface scene");
        renderer
    }

    fn empty_renderer(path: GeometryPath) -> Renderer {
        let mut renderer = Renderer::with_config_for_surface(RendererConfig {
            width: TEST_WIDTH,
            height: TEST_HEIGHT,
            ..RendererConfig::default()
        })
        .expect("valid M7d Surface renderer config");
        renderer.set_geometry_path(path);
        renderer
    }

    fn test_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![
                Vec3f::new(-0.1, 0.0, 1.0),
                Vec3f::new(0.1, 0.0, 1.2),
                Vec3f::new(0.0, 0.1, 1.4),
            ],
            opacity: vec![1.0; 3],
            scale_xyz: vec![[-1.0; 3]; 3],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 3],
            color_dc: vec![[0.1, 0.2, 0.3]; 3],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[allow(deprecated)]
    fn hidden_window(event_loop: &EventLoop<()>, title: &str) -> Result<Arc<Window>, HarnessError> {
        event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title(title)
                    .with_visible(false)
                    .with_inner_size(PhysicalSize::new(TEST_WIDTH, TEST_HEIGHT)),
            )
            .map(Arc::new)
            .map_err(|error| HarnessError::Skipped(format!("window-target-unavailable:{error}")))
    }

    fn presenter(
        window: Arc<Window>,
        renderer: &Renderer,
    ) -> Result<SurfacePresenter, HarnessError> {
        pollster::block_on(SurfacePresenter::from_window(
            window,
            TEST_WIDTH,
            TEST_HEIGHT,
            renderer,
        ))
        .map_err(|error| match error {
            SurfacePresenterError::SurfaceCreation
            | SurfacePresenterError::NoAdapter
            | SurfacePresenterError::NoSurfaceFormat => {
                HarnessError::Skipped(format!("window-surface-unavailable:{error}"))
            }
            other => HarnessError::Failed(format!("SurfacePresenter::from_window failed: {other}")),
        })
    }

    fn presenter_snapshot(presenter: &SurfacePresenter) -> PresenterSnapshot {
        PresenterSnapshot {
            geometry_path: presenter.geometry_path(),
            surface_size: presenter.surface_size(),
            addressable_splat_count: presenter.addressable_splat_count(),
            instance_count: presenter.instance_count(),
        }
    }

    fn session_snapshot(session: &SurfaceRenderSession) -> SessionSnapshot {
        let renderer = session.renderer();
        SessionSnapshot {
            geometry_path: session.geometry_path(),
            renderer_geometry_path: renderer.geometry_path(),
            scene_len: renderer.scene_len(),
            scene_allocation: renderer
                .scene()
                .map(|scene| std::ptr::from_ref(scene) as usize),
            resident_allocation: renderer
                .resident_scene()
                .map(|scene| std::ptr::from_ref(scene) as usize),
            positions_allocation: renderer
                .positions()
                .map(|positions| positions.as_ptr() as usize),
            sorted_indices: renderer.current_sorted_indices().to_vec(),
            last_stats: session.last_stats(),
            order_backend: session.order_backend(),
            projected_draw_policy: session.projected_draw_policy(),
            gpu_order_producer: session.gpu_order_producer(),
            raster_execution_plan: session.raster_execution_plan(),
            current_stats_submission: session.current_stats_submission(),
            surface_size: session.surface_size(),
        }
    }

    fn assert_presenter_switch_unsupported(
        result: Result<(), SurfacePresenterError>,
        context: &str,
    ) {
        assert!(
            matches!(
                result,
                Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported)
            ),
            "{context} returned {result:?}"
        );
    }

    fn assert_standalone_packed_rejected(
        result: Result<SurfacePresenter, SurfacePresenterError>,
        context: &str,
    ) {
        let error = match result {
            Err(error @ SurfacePresenterError::StandalonePackedPresenterUnsupported) => error,
            Err(other) => panic!("{context} returned the wrong error: {other:?}"),
            Ok(_) => panic!("{context} unexpectedly constructed a legacy Packed presenter"),
        };
        assert_eq!(error.code(), ErrorCode::Unsupported, "{context}");
        assert_eq!(
            error.to_string(),
            "standalone Packed surface presenters are unsupported; construct Packed through SurfaceRenderSession::from_* so the session owns the Exact surface host",
            "{context}"
        );
    }

    fn assert_session_switch_unsupported(result: Result<(), RendererError>, context: &str) {
        assert!(
            matches!(
                result,
                Err(RendererError::SurfacePresenter(
                    SurfacePresenterError::SurfaceGeometrySwitchUnsupported
                ))
            ),
            "{context} returned {result:?}"
        );
    }

    fn failed<T: std::fmt::Display>(context: &'static str) -> impl FnOnce(T) -> HarnessError {
        move |error| HarnessError::Failed(format!("{context}: {error}"))
    }
}

#[cfg(target_os = "macos")]
fn main() {
    macos::main();
}
