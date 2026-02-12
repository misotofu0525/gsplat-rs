use std::env;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use gsplat_core::{Camera, FrameStats, RenderMode};
use gsplat_io_ply::load_ply;
use gsplat_render_wgpu::Renderer;

fn main() {
    if let Err(err) = run() {
        eprintln!("bench-runner failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = BenchConfig::parse(env::args().skip(1).collect())?;
    if config.iterations == 0 {
        return Err("iterations must be > 0".to_owned());
    }

    let loaded = load_ply(Path::new(&config.dataset_path)).map_err(|err| err.to_string())?;

    let mut renderer = Renderer::new(RenderMode::SortedAlpha).map_err(|err| err.to_string())?;
    renderer
        .load_scene(loaded.scene)
        .map_err(|err| err.to_string())?;

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
            &config.dataset_path,
            config.iterations,
        )
    }
}

fn run_iteration_mode(
    renderer: &mut Renderer,
    camera: &Camera,
    dataset_path: &str,
    iterations: usize,
) -> Result<(), String> {
    let mut sum = FrameStats::zero();

    for _ in 0..iterations {
        let stats = renderer
            .render_frame(camera)
            .map_err(|err| err.to_string())?;
        sum.frame_ms += stats.frame_ms;
        sum.preprocess_ms += stats.preprocess_ms;
        sum.sort_ms += stats.sort_ms;
        sum.raster_ms += stats.raster_ms;
        sum.visible_count += stats.visible_count;
        sum.drawn_count += stats.drawn_count;
    }

    let n = iterations as f32;
    println!("bench-runner complete");
    println!("mode=iterations");
    println!("dataset={dataset_path}");
    println!("iterations={iterations}");
    println!("avg_frame_ms={:.4}", sum.frame_ms / n);
    println!("avg_preprocess_ms={:.4}", sum.preprocess_ms / n);
    println!("avg_sort_ms={:.4}", sum.sort_ms / n);
    println!("avg_raster_ms={:.4}", sum.raster_ms / n);
    println!("avg_visible_count={:.2}", sum.visible_count as f32 / n);
    println!("avg_drawn_count={:.2}", sum.drawn_count as f32 / n);

    Ok(())
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
        frame_count += 1;

        if frame_count % 60 == 0 {
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

#[derive(Debug, Clone)]
struct BenchConfig {
    dataset_path: String,
    iterations: usize,
    stability_seconds: Option<u64>,
    rss_growth_limit_kib: u64,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            dataset_path: "tests/datasets/minimal_ascii.ply".to_owned(),
            iterations: 120,
            stability_seconds: None,
            rss_growth_limit_kib: 64 * 1024,
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
                "--stability-seconds" => {
                    i += 1;
                    let value = args.get(i).ok_or("missing value for --stability-seconds")?;
                    config.stability_seconds = Some(
                        value
                            .parse::<u64>()
                            .map_err(|_| "invalid --stability-seconds value")?,
                    );
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

        Ok(config)
    }
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
