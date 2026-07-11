mod artifact;

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use artifact::{
    ArtifactContext, Display, Environment, FileIdentity, FrameSample, Renderer as ArtifactRenderer,
    ResourcePreflight, ResourceRequirement,
};
use gsplat_core::{Camera, FrameStats, RenderMode, RendererConfig, SceneBuffers, Vec3f};
use gsplat_io_ply::load_ply;
use gsplat_render_wgpu::{GeometryPath, Renderer};

fn main() {
    if let Err(err) = run() {
        eprintln!("bench-runner failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = BenchConfig::parse(env::args().skip(1).collect())?;
    if config.analysis.is_none() && config.iterations == 0 {
        return Err("iterations must be > 0".to_owned());
    }

    let dataset_path = Path::new(&config.dataset_path);
    let dataset_identity = config
        .artifact_dir
        .as_ref()
        .map(|_| artifact::file_identity(dataset_path))
        .transpose()?;
    let loaded = load_ply(dataset_path).map_err(|err| err.to_string())?;
    if let Some(expected) = dataset_identity.as_ref() {
        let actual = artifact::file_identity(dataset_path)?;
        if &actual != expected {
            return Err("dataset changed while it was being loaded".to_owned());
        }
    }
    if let Some(analysis) = config.analysis {
        return run_spatial_analysis(&loaded.scene, &config.dataset_path, analysis);
    }

    let splat_count = loaded.scene.len();
    let sh_degree = loaded.scene.sh_degree;
    let mut renderer = Renderer::new(RenderMode::SortedAlpha).map_err(|err| err.to_string())?;
    renderer.set_geometry_path(config.geometry_path);
    renderer
        .load_scene(loaded.scene)
        .map_err(|err| err.to_string())?;
    print_gpu_metadata(&renderer);
    print_direct_scene_preflight(&renderer)?;
    println!(
        "offscreen_geometry_pipeline={}",
        geometry_path_label(config.geometry_path)
    );

    let camera = Camera::default();

    if let Some(seconds) = config.stability_seconds {
        run_stability_mode(
            &mut renderer,
            &camera,
            &config.dataset_path,
            seconds,
            config.rss_growth_limit_kib,
        )
    } else {
        run_iteration_mode(
            &mut renderer,
            &camera,
            &config,
            splat_count,
            sh_degree,
            dataset_identity,
        )
    }
}

fn run_iteration_mode(
    renderer: &mut Renderer,
    camera: &Camera,
    config: &BenchConfig,
    splat_count: usize,
    sh_degree: u8,
    dataset_identity: Option<FileIdentity>,
) -> Result<(), String> {
    let run_id = match config.run_id.clone() {
        Some(run_id) => run_id,
        None => artifact::default_run_id()?,
    };
    let started_at_utc = artifact::utc_now()?;
    let mut sum = FrameStats::zero();
    let mut gpu_wait_ms = 0.0_f32;
    let mut gpu_complete_frame_ms = 0.0_f32;
    let mut raw_frames = Vec::with_capacity(config.iterations);

    for _ in 0..config.warmup_iterations {
        renderer
            .render_frame(camera)
            .map_err(|err| err.to_string())?;
        renderer.wait_for_gpu().map_err(|err| err.to_string())?;
    }

    let measurement_started_at_utc = artifact::utc_now()?;
    let measured_start = Instant::now();
    for _ in 0..config.iterations {
        let frame_start = Instant::now();
        let stats = renderer
            .render_frame(camera)
            .map_err(|err| err.to_string())?;
        let submit_elapsed_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        renderer.wait_for_gpu().map_err(|err| err.to_string())?;
        let complete_elapsed_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        sum.frame_ms += stats.frame_ms;
        sum.preprocess_ms += stats.preprocess_ms;
        sum.sort_ms += stats.sort_ms;
        sum.raster_ms += stats.raster_ms;
        sum.visible_count += stats.visible_count;
        sum.drawn_count += stats.drawn_count;
        gpu_wait_ms += (complete_elapsed_ms - submit_elapsed_ms).max(0.0);
        gpu_complete_frame_ms += complete_elapsed_ms;
        raw_frames.push(RawFrameSample {
            elapsed_ns: u64::try_from(measured_start.elapsed().as_nanos()).unwrap_or(u64::MAX),
            call_ms: f64::from(submit_elapsed_ms),
            frame_wall_ms: f64::from(complete_elapsed_ms),
            preprocess_ms: f64::from(stats.preprocess_ms),
            sort_ms: f64::from(stats.sort_ms),
            geometry_submit_ms: f64::from(stats.raster_ms),
            gpu_wait_ms: Some(f64::from(
                (complete_elapsed_ms - submit_elapsed_ms).max(0.0),
            )),
            gpu_complete_ms: Some(f64::from(complete_elapsed_ms)),
            visible: u64::from(stats.visible_count),
            drawn: u64::from(stats.drawn_count),
            sort_refreshed: None,
        });
    }
    let frames = raw_frames
        .into_iter()
        .enumerate()
        .map(|(frame_index, frame)| frame.into_artifact(&run_id, frame_index as u64))
        .collect::<Vec<_>>();
    let measurement_ended_at_utc = artifact::utc_now()?;

    let n = config.iterations as f32;
    let avg_gpu_complete_frame_ms = gpu_complete_frame_ms / n;
    let summary = artifact::summarize(
        &run_id,
        config.warmup_iterations,
        config.frame_budget_ms,
        &frames,
    )?;
    println!("bench-runner complete");
    println!("mode=iterations");
    println!("dataset={}", config.dataset_path);
    println!("iterations={}", config.iterations);
    println!("warmup_iterations={}", config.warmup_iterations);
    println!("benchmark_schema={}", artifact::SCHEMA);
    println!("benchmark_run_id={run_id}");
    println!("avg_submit_frame_ms={:.4}", sum.frame_ms / n);
    println!("avg_cpu_preprocess_ms={:.4}", sum.preprocess_ms / n);
    println!("avg_cpu_sort_ms={:.4}", sum.sort_ms / n);
    println!(
        "avg_geometry_encode_submit_cpu_wall_ms={:.4}",
        sum.raster_ms / n
    );
    println!("avg_gpu_wait_ms={:.4}", gpu_wait_ms / n);
    println!("avg_gpu_complete_frame_ms={avg_gpu_complete_frame_ms:.4}");
    println!("avg_visible_count={:.2}", sum.visible_count as f32 / n);
    println!("avg_drawn_count={:.2}", sum.drawn_count as f32 / n);
    print_distributions(&summary);

    if let Some(directory) = config.artifact_dir.as_deref() {
        let context = artifact_context(
            renderer,
            config,
            ArtifactContextInput {
                run_id: &run_id,
                started_at_utc: &started_at_utc,
                measurement_started_at_utc: &measurement_started_at_utc,
                measurement_ended_at_utc: &measurement_ended_at_utc,
                splat_count,
                sh_degree,
                dataset_identity: dataset_identity
                    .clone()
                    .ok_or("artifact dataset identity is unavailable")?,
            },
        )?;
        artifact::write_artifacts(directory, context, config.warmup_iterations, &frames)?;
        println!("benchmark_artifact_dir={}", directory.display());
    }

    if let Some(limit_ms) = config.max_avg_gpu_complete_ms {
        println!("max_avg_gpu_complete_ms={limit_ms:.4}");
        if avg_gpu_complete_frame_ms > limit_ms {
            return Err(format!(
                "average GPU-complete frame time exceeded limit: actual={avg_gpu_complete_frame_ms:.4}ms limit={limit_ms:.4}ms"
            ));
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct RawFrameSample {
    elapsed_ns: u64,
    call_ms: f64,
    frame_wall_ms: f64,
    preprocess_ms: f64,
    sort_ms: f64,
    geometry_submit_ms: f64,
    gpu_wait_ms: Option<f64>,
    gpu_complete_ms: Option<f64>,
    visible: u64,
    drawn: u64,
    sort_refreshed: Option<bool>,
}

impl RawFrameSample {
    fn into_artifact(self, run_id: &str, frame_index: u64) -> FrameSample {
        FrameSample {
            schema: artifact::SCHEMA,
            record_type: "frame",
            run_id: run_id.to_owned(),
            frame_index,
            elapsed_ns: self.elapsed_ns,
            call_ms: self.call_ms,
            frame_wall_ms: self.frame_wall_ms,
            preprocess_ms: self.preprocess_ms,
            sort_ms: self.sort_ms,
            geometry_submit_ms: self.geometry_submit_ms,
            gpu_wait_ms: self.gpu_wait_ms,
            gpu_complete_ms: self.gpu_complete_ms,
            visible: self.visible,
            drawn: self.drawn,
            sort_refreshed: self.sort_refreshed,
        }
    }
}

fn run_stability_mode(
    renderer: &mut Renderer,
    camera: &Camera,
    dataset_path: &str,
    stability_seconds: u64,
    rss_growth_limit_kib: u64,
) -> Result<(), String> {
    if stability_seconds == 0 {
        return Err("--stability-seconds must be > 0".to_owned());
    }

    let start = Instant::now();
    let deadline = start + Duration::from_secs(stability_seconds);

    let mut frame_count: u64 = 0;
    let mut min_rss = process_rss_kib();
    let mut max_rss = min_rss;

    while Instant::now() < deadline {
        renderer
            .render_frame(camera)
            .map_err(|err| err.to_string())?;
        renderer.wait_for_gpu().map_err(|err| err.to_string())?;
        frame_count += 1;

        if frame_count.is_multiple_of(60) {
            let rss = process_rss_kib();
            min_rss = merge_min(min_rss, rss);
            max_rss = merge_max(max_rss, rss);
        }
    }

    let elapsed = start.elapsed().as_secs_f32();
    let fps = if elapsed > 0.0 {
        frame_count as f32 / elapsed
    } else {
        0.0
    };

    println!("bench-runner complete");
    println!("mode=stability");
    println!("dataset={dataset_path}");
    println!("stability_seconds={stability_seconds}");
    println!("frames={frame_count}");
    println!("avg_fps={fps:.2}");

    if let (Some(min_kib), Some(max_kib)) = (min_rss, max_rss) {
        let growth = max_kib.saturating_sub(min_kib);
        println!("rss_min_kib={min_kib}");
        println!("rss_max_kib={max_kib}");
        println!("rss_growth_kib={growth}");
        println!("rss_growth_limit_kib={rss_growth_limit_kib}");

        if growth > rss_growth_limit_kib {
            return Err(format!(
                "rss growth exceeded limit: growth={growth}KiB limit={rss_growth_limit_kib}KiB"
            ));
        }
    } else {
        println!("rss_measurement=unavailable");
    }

    Ok(())
}

fn print_gpu_metadata(renderer: &Renderer) {
    let Some(info) = renderer.gpu_adapter_info() else {
        println!("gpu_adapter=unavailable");
        return;
    };

    println!("gpu_adapter_name={}", single_line(&info.name));
    println!("gpu_backend={:?}", info.backend);
    println!("gpu_device_type={:?}", info.device_type);
    println!("gpu_vendor_id={}", info.vendor);
    println!("gpu_device_id={}", info.device);
    println!("gpu_driver={}", single_line(&info.driver));
    println!("gpu_driver_info={}", single_line(&info.driver_info));
}

fn print_direct_scene_preflight(renderer: &Renderer) -> Result<(), String> {
    let report = renderer
        .current_direct_scene_preflight()
        .map_err(|error| error.to_string())?;
    println!("direct_preflight_path={:?}", report.path);
    println!("direct_preflight_splats={}", report.splat_count);
    println!("direct_preflight_sh_degree={}", report.sh_degree);
    println!(
        "direct_preflight_storage_binding_limit_bytes={}",
        report.effective_storage_binding_limit
    );
    println!(
        "direct_preflight_max_buffer_size_bytes={}",
        report.effective_max_buffer_size
    );
    println!(
        "direct_preflight_limiting_resource={:?}",
        report.limiting_resource
    );
    println!(
        "direct_preflight_max_direct_splats={}",
        report.max_direct_splats
    );
    for requirement in report.requirements {
        println!(
            "direct_preflight_resource={:?} required_bytes={} limit_bytes={} fits={}",
            requirement.resource,
            requirement.required_bytes,
            requirement.limit_bytes,
            requirement.fits
        );
    }
    Ok(())
}

fn print_distributions(summary: &artifact::Summary) {
    for (name, distribution) in [
        ("call_ms", summary.distributions.call_ms.as_ref()),
        (
            "frame_wall_ms",
            summary.distributions.frame_wall_ms.as_ref(),
        ),
        (
            "preprocess_ms",
            summary.distributions.preprocess_ms.as_ref(),
        ),
        ("sort_ms", summary.distributions.sort_ms.as_ref()),
        (
            "geometry_submit_ms",
            summary.distributions.geometry_submit_ms.as_ref(),
        ),
        ("gpu_wait_ms", summary.distributions.gpu_wait_ms.as_ref()),
        (
            "gpu_complete_ms",
            summary.distributions.gpu_complete_ms.as_ref(),
        ),
    ] {
        if let Some(distribution) = distribution {
            println!(
                "distribution_{name}=count:{} mean:{:.4} p50:{:.4} p90:{:.4} p95:{:.4} p99:{:.4} max:{:.4}",
                distribution.count,
                distribution.mean,
                distribution.p50,
                distribution.p90,
                distribution.p95,
                distribution.p99,
                distribution.max
            );
        } else {
            println!("distribution_{name}=unavailable");
        }
    }
    println!("frame_budget_ms={:.4}", summary.frame_budget_ms);
    println!("missed_frame_count={}", summary.missed_frame_count);
}

struct ArtifactContextInput<'a> {
    run_id: &'a str,
    started_at_utc: &'a str,
    measurement_started_at_utc: &'a str,
    measurement_ended_at_utc: &'a str,
    splat_count: usize,
    sh_degree: u8,
    dataset_identity: FileIdentity,
}

fn artifact_context(
    renderer: &Renderer,
    config: &BenchConfig,
    input: ArtifactContextInput<'_>,
) -> Result<ArtifactContext, String> {
    let render_config = RendererConfig::default();
    let dataset = artifact::dataset_with_identity(
        Path::new(&config.dataset_path),
        input.splat_count,
        input.sh_degree,
        input.dataset_identity,
    )?;
    let trace = artifact::trace(
        "static-default-camera-v1",
        b"gsplat-camera-trace/v1\nmode=static\ncamera=Camera::default\n",
    );
    let info = renderer.gpu_adapter_info();
    let preflight = renderer
        .current_direct_scene_preflight()
        .map_err(|error| error.to_string())?;
    let backend = info
        .map(|value| format!("{:?}", value.backend))
        .unwrap_or_else(|| "unavailable".to_owned());
    let adapter = info.map(|value| single_line(&value.name));
    let adapter_device_type = info.map(|value| format!("{:?}", value.device_type));
    let device = host_model();
    let driver = info
        .map(|value| single_line(&value.driver))
        .filter(|value| !value.is_empty());
    let mut unavailable_fields = vec![
        "environment.browser".to_owned(),
        "frames[*].sort_refreshed".to_owned(),
    ];
    if adapter.is_none() {
        unavailable_fields.push("environment.adapter".to_owned());
    }
    if device.is_none() {
        unavailable_fields.push("environment.device".to_owned());
    }
    if adapter_device_type.is_none() {
        unavailable_fields.push("environment.adapter_device_type".to_owned());
    }
    if driver.is_none() {
        unavailable_fields.push("environment.driver".to_owned());
    }

    Ok(ArtifactContext {
        run_id: input.run_id.to_owned(),
        series_id: config.series_id.clone(),
        started_at_utc: input.started_at_utc.to_owned(),
        measurement_started_at_utc: input.measurement_started_at_utc.to_owned(),
        measurement_ended_at_utc: input.measurement_ended_at_utc.to_owned(),
        build: artifact::repository_build()?,
        dataset,
        trace,
        renderer: ArtifactRenderer {
            implementation: "gsplat-rs".to_owned(),
            path: geometry_path_label(config.geometry_path).to_owned(),
            backend,
            sort_policy: "cpu_every_frame".to_owned(),
            resource_preflight: Some(ResourcePreflight {
                path: format!("{:?}", preflight.path),
                splat_count: preflight.splat_count,
                sh_degree: preflight.sh_degree,
                storage_binding_limit_bytes: preflight.effective_storage_binding_limit,
                max_buffer_size_bytes: preflight.effective_max_buffer_size,
                limiting_resource: format!("{:?}", preflight.limiting_resource),
                max_direct_splats: preflight.max_direct_splats,
                remediation: format!("{:?}", preflight.remediation),
                requirements: preflight
                    .requirements
                    .into_iter()
                    .map(|requirement| ResourceRequirement {
                        resource: format!("{:?}", requirement.resource),
                        required_bytes: requirement.required_bytes,
                        limit_bytes: requirement.limit_bytes,
                        fits: requirement.fits,
                    })
                    .collect(),
            }),
        },
        display: Display {
            width: render_config.width,
            height: render_config.height,
            dpr: 1.0,
            refresh_hz: config.refresh_hz,
            frame_budget_ms: config.frame_budget_ms,
            refresh_hz_source: "configured".to_owned(),
            frame_budget_source: "configured".to_owned(),
        },
        environment: Environment {
            platform: std::env::consts::OS.to_owned(),
            os: host_os_description(),
            device,
            browser: None,
            adapter,
            adapter_device_type,
            driver,
        },
        unavailable_fields,
    })
}

fn host_os_description() -> String {
    if let Some(version) = command_output("sw_vers", &["-productVersion"]) {
        return format!("macOS {version}");
    }
    command_output("uname", &["-sr"]).unwrap_or_else(|| std::env::consts::OS.to_owned())
}

fn host_model() -> Option<String> {
    command_output("sysctl", &["-n", "hw.model"]).or_else(|| command_output("uname", &["-m"]))
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn run_spatial_analysis(
    scene: &SceneBuffers,
    dataset_path: &str,
    config: SpatialAnalysisConfig,
) -> Result<(), String> {
    let Some((min, max)) = scene_bounds(scene) else {
        return Err("scene has no finite positions".to_owned());
    };

    let grid_axis = config.grid_axis.max(1);
    let grid_cell_count = grid_axis
        .checked_mul(grid_axis)
        .and_then(|v| v.checked_mul(grid_axis))
        .ok_or("analysis grid is too large")?;
    let mut grid_counts = vec![0_u32; grid_cell_count];
    let mut grid_min_index = vec![usize::MAX; grid_cell_count];
    let mut grid_max_index = vec![0_usize; grid_cell_count];

    for (i, position) in scene.positions.iter().enumerate() {
        let cell = grid_cell_index(*position, min, max, grid_axis);
        grid_counts[cell] += 1;
        grid_min_index[cell] = grid_min_index[cell].min(i);
        grid_max_index[cell] = grid_max_index[cell].max(i);
    }

    let mut non_empty_grid_counts = grid_counts
        .iter()
        .copied()
        .filter(|count| *count > 0)
        .collect::<Vec<_>>();
    non_empty_grid_counts.sort_unstable();

    let mut interval_span_ratios = Vec::new();
    for cell in 0..grid_cell_count {
        let count = grid_counts[cell] as usize;
        if count == 0 {
            continue;
        }
        let span = grid_max_index[cell] - grid_min_index[cell] + 1;
        interval_span_ratios.push(span as f32 / count as f32);
    }
    interval_span_ratios.sort_by(f32::total_cmp);

    let camera = auto_analysis_camera(scene, config.width, config.height)?;
    let aspect = config.width as f32 / config.height.max(1) as f32;
    let f = 1.0 / (camera.intrinsics.vertical_fov_radians * 0.5).tan();
    let (right, up, forward) =
        camera_basis_for_analysis(camera.pose.position, scene_center(min, max));
    let tile_axis = config.tile_axis.max(1);
    let mut tile_counts = vec![0_u32; tile_axis * tile_axis];
    let mut visible_center_count = 0_u32;
    let mut center_in_view_count = 0_u32;
    let mut visible_grid = vec![false; grid_cell_count];

    for position in &scene.positions {
        let rel = vec3_sub(*position, camera.pose.position);
        let p_cam = Vec3f::new(
            vec3_dot(right, rel),
            vec3_dot(up, rel),
            vec3_dot(forward, rel),
        );
        if p_cam.z < camera.intrinsics.near_plane
            || p_cam.z > camera.intrinsics.far_plane
            || p_cam.z <= 1.0e-6
        {
            continue;
        }
        visible_center_count += 1;
        visible_grid[grid_cell_index(*position, min, max, grid_axis)] = true;

        let inv_z = 1.0 / p_cam.z;
        let x_ndc = (p_cam.x * f) * inv_z / aspect;
        let y_ndc = (p_cam.y * f) * inv_z;
        if (-1.0..=1.0).contains(&x_ndc) && (-1.0..=1.0).contains(&y_ndc) {
            center_in_view_count += 1;
            let tx = (((x_ndc + 1.0) * 0.5) * tile_axis as f32)
                .floor()
                .clamp(0.0, tile_axis.saturating_sub(1) as f32) as usize;
            let ty = (((1.0 - y_ndc) * 0.5) * tile_axis as f32)
                .floor()
                .clamp(0.0, tile_axis.saturating_sub(1) as f32) as usize;
            tile_counts[ty * tile_axis + tx] += 1;
        }
    }

    let visible_grid_cells = visible_grid.iter().filter(|visible| **visible).count();
    let mut non_empty_tile_counts = tile_counts
        .iter()
        .copied()
        .filter(|count| *count > 0)
        .collect::<Vec<_>>();
    non_empty_tile_counts.sort_unstable();

    println!("bench-runner complete");
    println!("mode=spatial-analysis");
    println!("dataset={dataset_path}");
    println!("splats={}", scene.len());
    println!("bounds_min={:.6},{:.6},{:.6}", min.x, min.y, min.z);
    println!("bounds_max={:.6},{:.6},{:.6}", max.x, max.y, max.z);
    println!(
        "analysis_surface={}x{} grid_axis={} tile_axis={}",
        config.width, config.height, grid_axis, tile_axis
    );
    println!(
        "grid_non_empty={}/{}",
        non_empty_grid_counts.len(),
        grid_cell_count
    );
    println!(
        "grid_count_p50={} p90={} p99={} max={}",
        percentile_u32(&non_empty_grid_counts, 0.50),
        percentile_u32(&non_empty_grid_counts, 0.90),
        percentile_u32(&non_empty_grid_counts, 0.99),
        non_empty_grid_counts.last().copied().unwrap_or(0)
    );
    println!(
        "grid_visible_center_cells={}/{}",
        visible_grid_cells,
        non_empty_grid_counts.len().max(1)
    );
    println!(
        "grid_interval_span_per_count_p50={:.2} p90={:.2} p99={:.2} max={:.2}",
        percentile_f32(&interval_span_ratios, 0.50),
        percentile_f32(&interval_span_ratios, 0.90),
        percentile_f32(&interval_span_ratios, 0.99),
        interval_span_ratios.last().copied().unwrap_or(0.0)
    );
    println!("visible_center_count={visible_center_count}");
    println!("center_in_view_count={center_in_view_count}");
    println!(
        "tile_non_empty={}/{}",
        non_empty_tile_counts.len(),
        tile_counts.len()
    );
    println!(
        "tile_center_count_p50={} p90={} p99={} max={}",
        percentile_u32(&non_empty_tile_counts, 0.50),
        percentile_u32(&non_empty_tile_counts, 0.90),
        percentile_u32(&non_empty_tile_counts, 0.99),
        non_empty_tile_counts.last().copied().unwrap_or(0)
    );

    Ok(())
}

#[derive(Debug, Clone)]
struct BenchConfig {
    dataset_path: String,
    iterations: usize,
    warmup_iterations: usize,
    max_avg_gpu_complete_ms: Option<f32>,
    artifact_dir: Option<PathBuf>,
    series_id: String,
    run_id: Option<String>,
    frame_budget_ms: f64,
    refresh_hz: f64,
    stability_seconds: Option<u64>,
    rss_growth_limit_kib: u64,
    analysis: Option<SpatialAnalysisConfig>,
    geometry_path: GeometryPath,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            dataset_path: "tests/datasets/minimal_ascii.ply".to_owned(),
            iterations: 120,
            warmup_iterations: 10,
            max_avg_gpu_complete_ms: None,
            artifact_dir: None,
            series_id: "local".to_owned(),
            run_id: None,
            frame_budget_ms: 1000.0 / 60.0,
            refresh_hz: 60.0,
            stability_seconds: None,
            rss_growth_limit_kib: 64 * 1024,
            analysis: None,
            geometry_path: GeometryPath::SortedIndexDirect,
        }
    }
}

fn geometry_path_label(path: GeometryPath) -> &'static str {
    match path {
        GeometryPath::SortedIndexDirect => "sorted_index_direct",
        GeometryPath::PackedAtlas => "packed_atlas",
        GeometryPath::PagedActiveAtlas => "paged_active_atlas",
    }
}

fn parse_geometry_path(value: &str) -> Result<GeometryPath, String> {
    match value {
        "direct" | "sorted_index_direct" => Ok(GeometryPath::SortedIndexDirect),
        "packed" | "packed_atlas" => Ok(GeometryPath::PackedAtlas),
        "paged" | "paged_active_atlas" => Ok(GeometryPath::PagedActiveAtlas),
        other => Err(format!(
            "invalid --geometry-path '{other}' (expected direct|packed|paged)"
        )),
    }
}

#[derive(Debug, Clone, Copy)]
struct SpatialAnalysisConfig {
    width: u32,
    height: u32,
    grid_axis: usize,
    tile_axis: usize,
}

impl Default for SpatialAnalysisConfig {
    fn default() -> Self {
        Self {
            width: 1080,
            height: 2400,
            grid_axis: 16,
            tile_axis: 32,
        }
    }
}

impl BenchConfig {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut config = Self::default();
        let mut dataset_overridden = false;

        let mut i = 0_usize;
        while i < args.len() {
            match args[i].as_str() {
                "--analyze-spatial" => {
                    config
                        .analysis
                        .get_or_insert_with(SpatialAnalysisConfig::default);
                }
                "--analysis-width" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --analysis-width")?;
                    let analysis = config
                        .analysis
                        .get_or_insert_with(SpatialAnalysisConfig::default);
                    analysis.width = value
                        .parse::<u32>()
                        .map_err(|_| "invalid --analysis-width value")?
                        .max(1);
                }
                "--analysis-height" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --analysis-height")?;
                    let analysis = config
                        .analysis
                        .get_or_insert_with(SpatialAnalysisConfig::default);
                    analysis.height = value
                        .parse::<u32>()
                        .map_err(|_| "invalid --analysis-height value")?
                        .max(1);
                }
                "--analysis-grid" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --analysis-grid")?;
                    let analysis = config
                        .analysis
                        .get_or_insert_with(SpatialAnalysisConfig::default);
                    analysis.grid_axis = value
                        .parse::<usize>()
                        .map_err(|_| "invalid --analysis-grid value")?
                        .clamp(1, 128);
                }
                "--analysis-tiles" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --analysis-tiles")?;
                    let analysis = config
                        .analysis
                        .get_or_insert_with(SpatialAnalysisConfig::default);
                    analysis.tile_axis = value
                        .parse::<usize>()
                        .map_err(|_| "invalid --analysis-tiles value")?
                        .clamp(1, 256);
                }
                "--stability-seconds" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --stability-seconds")?;
                    config.stability_seconds = Some(
                        value
                            .parse::<u64>()
                            .map_err(|_| "invalid --stability-seconds value")?,
                    );
                }
                "--artifact-dir" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --artifact-dir")?;
                    if value.is_empty() {
                        return Err("--artifact-dir must not be empty".to_owned());
                    }
                    config.artifact_dir = Some(PathBuf::from(value));
                }
                "--series-id" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --series-id")?;
                    if value.is_empty() {
                        return Err("--series-id must not be empty".to_owned());
                    }
                    config.series_id = value.to_owned();
                }
                "--run-id" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --run-id")?;
                    if value.is_empty() {
                        return Err("--run-id must not be empty".to_owned());
                    }
                    config.run_id = Some(value.to_owned());
                }
                "--frame-budget-ms" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --frame-budget-ms")?;
                    config.frame_budget_ms = parse_positive_f64(value, "--frame-budget-ms")?;
                }
                "--refresh-hz" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --refresh-hz")?;
                    config.refresh_hz = parse_positive_f64(value, "--refresh-hz")?;
                }
                "--warmup-iterations" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --warmup-iterations")?;
                    config.warmup_iterations = value
                        .parse::<usize>()
                        .map_err(|_| "invalid --warmup-iterations value")?;
                }
                "--geometry-path" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --geometry-path")?;
                    config.geometry_path = parse_geometry_path(value)?;
                }
                "--max-avg-gpu-complete-ms" => {
                    i += 1;
                    let value = args
                        .get(i)
                        .ok_or("missing value for --max-avg-gpu-complete-ms")?;
                    let limit = value
                        .parse::<f32>()
                        .map_err(|_| "invalid --max-avg-gpu-complete-ms value")?;
                    if !limit.is_finite() || limit <= 0.0 {
                        return Err(
                            "--max-avg-gpu-complete-ms must be finite and positive".to_owned()
                        );
                    }
                    config.max_avg_gpu_complete_ms = Some(limit);
                }
                "--rss-growth-limit-kib" => {
                    i += 1;
                    let value = args
                        .get(i)
                        .ok_or("missing value for --rss-growth-limit-kib")?;
                    config.rss_growth_limit_kib = value
                        .parse::<u64>()
                        .map_err(|_| "invalid --rss-growth-limit-kib value")?;
                }
                value if value.starts_with("--") => {
                    return Err(format!("unknown option: {value}"));
                }
                value => {
                    if !dataset_overridden {
                        config.dataset_path = value.to_owned();
                        dataset_overridden = true;
                    } else {
                        config.iterations = value
                            .parse::<usize>()
                            .map_err(|_| "invalid iterations value")?;
                    }
                }
            }
            i += 1;
        }

        if config.iterations == 0 {
            return Err("iterations must be greater than zero".to_owned());
        }
        if config.stability_seconds == Some(0) {
            return Err("--stability-seconds must be greater than zero".to_owned());
        }
        if config.stability_seconds.is_some() && config.artifact_dir.is_some() {
            return Err("--artifact-dir currently supports iteration mode only".to_owned());
        }

        Ok(config)
    }
}

fn parse_positive_f64(value: &str, option: &str) -> Result<f64, String> {
    let value = value
        .parse::<f64>()
        .map_err(|_| format!("invalid {option} value"))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{option} must be finite and positive"));
    }
    Ok(value)
}

fn scene_bounds(scene: &SceneBuffers) -> Option<(Vec3f, Vec3f)> {
    let mut iter = scene
        .positions
        .iter()
        .copied()
        .filter(|position| position.is_finite());
    let first = iter.next()?;
    let mut min = first;
    let mut max = first;
    for position in iter {
        min.x = min.x.min(position.x);
        min.y = min.y.min(position.y);
        min.z = min.z.min(position.z);
        max.x = max.x.max(position.x);
        max.y = max.y.max(position.y);
        max.z = max.z.max(position.z);
    }
    Some((min, max))
}

fn scene_center(min: Vec3f, max: Vec3f) -> Vec3f {
    Vec3f::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        (min.z + max.z) * 0.5,
    )
}

fn grid_cell_index(position: Vec3f, min: Vec3f, max: Vec3f, axis: usize) -> usize {
    let axis = axis.max(1);
    let ix = grid_axis_index(position.x, min.x, max.x, axis);
    let iy = grid_axis_index(position.y, min.y, max.y, axis);
    let iz = grid_axis_index(position.z, min.z, max.z, axis);
    (iz * axis + iy) * axis + ix
}

fn grid_axis_index(value: f32, min: f32, max: f32, axis: usize) -> usize {
    if axis <= 1 || max <= min {
        return 0;
    }
    let normalized = ((value - min) / (max - min)).clamp(0.0, 0.999_999);
    (normalized * axis as f32) as usize
}

fn auto_analysis_camera(scene: &SceneBuffers, width: u32, height: u32) -> Result<Camera, String> {
    let Some((min, max)) = scene_bounds(scene) else {
        return Err("scene has no finite positions".to_owned());
    };
    let center = scene_center(min, max);
    let extent = Vec3f::new(max.x - min.x, max.y - min.y, max.z - min.z);
    let half_x = (extent.x * 0.5).max(1.0e-3);
    let half_y = (extent.y * 0.5).max(1.0e-3);
    let half_z = (extent.z * 0.5).max(1.0e-3);
    let aspect = width as f32 / height.max(1) as f32;
    let mut camera = Camera::default();
    let vfov = camera.intrinsics.vertical_fov_radians.max(1.0e-3);
    let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();
    let dist_y = half_y / (vfov * 0.5).tan();
    let dist_x = half_x / (hfov * 0.5).tan();
    let distance = (dist_y.max(dist_x) + half_z) * 1.2;
    let radius = half_x.max(half_y).max(half_z);

    camera.pose.position = Vec3f::new(center.x, center.y, center.z - distance);
    camera.pose.rotation_xyzw = [0.0, 0.0, 0.0, 1.0];
    camera.intrinsics.near_plane = (distance - radius * 2.0).max(0.01);
    camera.intrinsics.far_plane = (distance + radius * 8.0).max(100.0);
    let config = RendererConfig {
        width,
        height,
        ..RendererConfig::default()
    };
    let aspect = (config.width as f32 / config.height.max(1) as f32).max(1.0e-3);
    if aspect < 0.6 {
        camera.intrinsics.vertical_fov_radians = 65.0_f32.to_radians();
    }
    camera.validate().map_err(|err| format!("{err:?}"))?;
    Ok(camera)
}

fn camera_basis_for_analysis(position: Vec3f, target: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
    let forward = vec3_normalize(vec3_sub(target, position)).unwrap_or(Vec3f::new(0.0, 0.0, 1.0));
    let world_up = if forward.y.abs() > 0.98 {
        Vec3f::new(0.0, 0.0, 1.0)
    } else {
        Vec3f::new(0.0, 1.0, 0.0)
    };
    let right = vec3_normalize(vec3_cross(world_up, forward)).unwrap_or(Vec3f::new(1.0, 0.0, 0.0));
    let up = vec3_cross(forward, right);
    (right, up, forward)
}

fn vec3_sub(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn vec3_dot(a: Vec3f, b: Vec3f) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn vec3_cross(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn vec3_normalize(v: Vec3f) -> Option<Vec3f> {
    let len2 = vec3_dot(v, v);
    if !len2.is_finite() || len2 <= 1.0e-20 {
        return None;
    }
    let inv_len = len2.sqrt().recip();
    Some(Vec3f::new(v.x * inv_len, v.y * inv_len, v.z * inv_len))
}

fn percentile_u32(sorted_values: &[u32], p: f32) -> u32 {
    if sorted_values.is_empty() {
        return 0;
    }
    let index = percentile_index(sorted_values.len(), p);
    sorted_values[index]
}

fn percentile_f32(sorted_values: &[f32], p: f32) -> f32 {
    if sorted_values.is_empty() {
        return 0.0;
    }
    let index = percentile_index(sorted_values.len(), p);
    sorted_values[index]
}

fn percentile_index(len: usize, p: f32) -> usize {
    if len <= 1 {
        return 0;
    }
    ((len - 1) as f32 * p.clamp(0.0, 1.0)).round() as usize
}

fn process_rss_kib() -> Option<u64> {
    let pid = std::process::id().to_string();
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    value.trim().parse::<u64>().ok()
}

fn merge_min(current: Option<u64>, next: Option<u64>) -> Option<u64> {
    match (current, next) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (None, some @ Some(_)) => some,
        (some @ Some(_), None) => some,
        (None, None) => None,
    }
}

fn merge_max(current: Option<u64>, next: Option<u64>) -> Option<u64> {
    match (current, next) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (None, some @ Some(_)) => some,
        (some @ Some(_), None) => some,
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gsplat_core::{SceneBuffers, Vec3f};

    use super::{
        BenchConfig, grid_axis_index, grid_cell_index, merge_max, merge_min, percentile_f32,
        percentile_index, percentile_u32, scene_bounds,
    };

    fn scene_with_positions(positions: Vec<Vec3f>) -> SceneBuffers {
        let len = positions.len();
        SceneBuffers {
            positions,
            opacity: vec![1.0; len],
            scale_xyz: vec![[0.0, 0.0, 0.0]; len],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; len],
            color_dc: vec![[0.1, 0.2, 0.3]; len],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[test]
    fn bench_config_parse_defaults() {
        let config = BenchConfig::parse(Vec::new()).unwrap();

        assert_eq!(config.dataset_path, "tests/datasets/minimal_ascii.ply");
        assert_eq!(config.iterations, 120);
        assert_eq!(config.warmup_iterations, 10);
        assert_eq!(config.max_avg_gpu_complete_ms, None);
        assert!(config.artifact_dir.is_none());
        assert_eq!(config.series_id, "local");
        assert!(config.run_id.is_none());
        assert!((config.frame_budget_ms - (1000.0 / 60.0)).abs() < f64::EPSILON);
        assert_eq!(config.refresh_hz, 60.0);
        assert_eq!(config.stability_seconds, None);
        assert_eq!(config.rss_growth_limit_kib, 64 * 1024);
        assert!(config.analysis.is_none());
    }

    #[test]
    fn bench_config_parse_dataset_iterations_and_stability() {
        let config = BenchConfig::parse(vec![
            "scene.ply".to_owned(),
            "42".to_owned(),
            "--stability-seconds".to_owned(),
            "5".to_owned(),
            "--rss-growth-limit-kib".to_owned(),
            "4096".to_owned(),
            "--warmup-iterations".to_owned(),
            "3".to_owned(),
            "--max-avg-gpu-complete-ms".to_owned(),
            "12.5".to_owned(),
        ])
        .unwrap();

        assert_eq!(config.dataset_path, "scene.ply");
        assert_eq!(config.iterations, 42);
        assert_eq!(config.warmup_iterations, 3);
        assert_eq!(config.max_avg_gpu_complete_ms, Some(12.5));
        assert_eq!(config.stability_seconds, Some(5));
        assert_eq!(config.rss_growth_limit_kib, 4096);
    }

    #[test]
    fn bench_config_parse_artifact_options() {
        let config = BenchConfig::parse(vec![
            "--artifact-dir".to_owned(),
            "target/benchmarks/test".to_owned(),
            "--series-id".to_owned(),
            "series".to_owned(),
            "--run-id".to_owned(),
            "run".to_owned(),
            "--frame-budget-ms".to_owned(),
            "33.333".to_owned(),
            "--refresh-hz".to_owned(),
            "30".to_owned(),
        ])
        .unwrap();

        assert_eq!(
            config.artifact_dir,
            Some(PathBuf::from("target/benchmarks/test"))
        );
        assert_eq!(config.series_id, "series");
        assert_eq!(config.run_id.as_deref(), Some("run"));
        assert_eq!(config.frame_budget_ms, 33.333);
        assert_eq!(config.refresh_hz, 30.0);
        assert_eq!(config.stability_seconds, None);
    }

    #[test]
    fn bench_config_parse_spatial_flags_and_clamps() {
        let config = BenchConfig::parse(vec![
            "--analyze-spatial".to_owned(),
            "--analysis-width".to_owned(),
            "0".to_owned(),
            "--analysis-height".to_owned(),
            "720".to_owned(),
            "--analysis-grid".to_owned(),
            "999".to_owned(),
            "--analysis-tiles".to_owned(),
            "0".to_owned(),
        ])
        .unwrap();
        let analysis = config.analysis.unwrap();

        assert_eq!(analysis.width, 1);
        assert_eq!(analysis.height, 720);
        assert_eq!(analysis.grid_axis, 128);
        assert_eq!(analysis.tile_axis, 1);
    }

    #[test]
    fn bench_config_rejects_unknown_and_missing_option_values() {
        let err = BenchConfig::parse(vec!["--nope".to_owned()]).unwrap_err();
        assert_eq!(err, "unknown option: --nope");

        let err = BenchConfig::parse(vec!["--analysis-width".to_owned()]).unwrap_err();
        assert_eq!(err, "missing value for --analysis-width");

        let err = BenchConfig::parse(vec!["--max-avg-gpu-complete-ms".to_owned(), "0".to_owned()])
            .unwrap_err();
        assert_eq!(err, "--max-avg-gpu-complete-ms must be finite and positive");

        let err = BenchConfig::parse(vec!["scene.ply".to_owned(), "0".to_owned()]).unwrap_err();
        assert_eq!(err, "iterations must be greater than zero");

        let err =
            BenchConfig::parse(vec!["--stability-seconds".to_owned(), "0".to_owned()]).unwrap_err();
        assert_eq!(err, "--stability-seconds must be greater than zero");

        let err = BenchConfig::parse(vec![
            "--stability-seconds".to_owned(),
            "1".to_owned(),
            "--artifact-dir".to_owned(),
            "target/out".to_owned(),
        ])
        .unwrap_err();
        assert_eq!(err, "--artifact-dir currently supports iteration mode only");

        let err =
            BenchConfig::parse(vec!["--frame-budget-ms".to_owned(), "NaN".to_owned()]).unwrap_err();
        assert_eq!(err, "--frame-budget-ms must be finite and positive");
    }

    #[test]
    fn scene_bounds_ignores_non_finite_positions() {
        let scene = scene_with_positions(vec![
            Vec3f::new(f32::NAN, 0.0, 0.0),
            Vec3f::new(-1.0, 2.0, 3.0),
            Vec3f::new(4.0, f32::INFINITY, 6.0),
            Vec3f::new(2.0, -3.0, 5.0),
        ]);

        let (min, max) = scene_bounds(&scene).unwrap();

        assert_eq!(min, Vec3f::new(-1.0, -3.0, 3.0));
        assert_eq!(max, Vec3f::new(2.0, 2.0, 5.0));
    }

    #[test]
    fn grid_indices_clamp_bounds() {
        assert_eq!(grid_axis_index(-10.0, 0.0, 1.0, 4), 0);
        assert_eq!(grid_axis_index(1.0, 0.0, 1.0, 4), 3);
        assert_eq!(grid_axis_index(0.5, 0.0, 1.0, 4), 2);

        let min = Vec3f::new(0.0, 0.0, 0.0);
        let max = Vec3f::new(1.0, 1.0, 1.0);
        assert_eq!(grid_cell_index(Vec3f::new(1.0, 1.0, 1.0), min, max, 2), 7);
    }

    #[test]
    fn percentile_helpers_handle_empty_and_clamped_inputs() {
        assert_eq!(percentile_u32(&[], 0.5), 0);
        assert_eq!(percentile_f32(&[], 0.5), 0.0);
        assert_eq!(percentile_index(4, -1.0), 0);
        assert_eq!(percentile_index(4, 2.0), 3);
        assert_eq!(percentile_u32(&[10, 20, 30, 40], 0.5), 30);
        assert_eq!(percentile_f32(&[1.0, 2.0, 3.0], 0.5), 2.0);
    }

    #[test]
    fn merge_min_and_max_preserve_known_measurements() {
        assert_eq!(merge_min(None, Some(5)), Some(5));
        assert_eq!(merge_min(Some(7), Some(5)), Some(5));
        assert_eq!(merge_min(Some(7), None), Some(7));
        assert_eq!(merge_max(None, Some(5)), Some(5));
        assert_eq!(merge_max(Some(7), Some(5)), Some(7));
        assert_eq!(merge_max(Some(7), None), Some(7));
    }
}
