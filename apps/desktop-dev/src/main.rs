use std::env;
use std::f32::consts::PI;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Instant;

use gsplat_core::{Camera, RenderMode, RendererConfig, Vec3f};
use gsplat_io_ply::load_ply;
use gsplat_render_wgpu::Renderer;

fn main() {
    let args = match Args::parse(env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };

    if let Err(err) = run(args) {
        eprintln!("desktop-dev failed: {err}");
        std::process::exit(1);
    }
}

#[derive(Debug, Clone)]
struct Args {
    dataset_path: PathBuf,
    config: RendererConfig,
    frames: u32,
    orbit: bool,
    auto_camera: bool,
    png_out: Option<PathBuf>,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut dataset_path: Option<PathBuf> = None;
        let mut config = RendererConfig::default();
        config.mode = RenderMode::SortedAlpha;
        let mut frames = 1_u32;
        let mut orbit = false;
        let mut auto_camera = false;
        let mut png_out: Option<PathBuf> = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    return Err(usage());
                }
                "--frames" => {
                    let value = args.next().ok_or_else(|| "missing value for --frames".to_owned())?;
                    frames = value.parse::<u32>().map_err(|_| "invalid --frames".to_owned())?;
                    frames = frames.max(1);
                }
                "--width" => {
                    let value = args.next().ok_or_else(|| "missing value for --width".to_owned())?;
                    config.width = value.parse::<u32>().map_err(|_| "invalid --width".to_owned())?;
                }
                "--height" => {
                    let value = args.next().ok_or_else(|| "missing value for --height".to_owned())?;
                    config.height = value.parse::<u32>().map_err(|_| "invalid --height".to_owned())?;
                }
                "--orbit" => orbit = true,
                "--auto-camera" => auto_camera = true,
                "--png" => {
                    let value = args.next().ok_or_else(|| "missing value for --png".to_owned())?;
                    png_out = Some(PathBuf::from(value));
                }
                _ if arg.starts_with("--") => {
                    return Err(format!("unknown flag: {arg}\n\n{}", usage()));
                }
                _ => {
                    if dataset_path.is_some() {
                        return Err(format!("unexpected extra arg: {arg}\n\n{}", usage()));
                    }
                    dataset_path = Some(PathBuf::from(arg));
                }
            }
        }

        Ok(Self {
            dataset_path: dataset_path.unwrap_or_else(|| "tests/datasets/minimal_ascii.ply".into()),
            config,
            frames,
            orbit,
            auto_camera,
            png_out,
        })
    }
}

fn usage() -> String {
    let lines = [
        "usage: cargo run -p desktop-dev -- [dataset.ply] [flags]",
        "",
        "flags:",
        "  --frames N       render N frames (default: 1)",
        "  --width W        output width (default: 1280)",
        "  --height H       output height (default: 720)",
        "  --orbit          animate camera yaw over frames",
        "  --auto-camera    place camera based on dataset bounds",
        "  --png PATH       write the last rendered frame to PATH (requires GPU rasterizer)",
    ];
    lines.join("\n")
}

fn run(args: Args) -> Result<(), String> {
    let loaded = load_ply(Path::new(&args.dataset_path)).map_err(|err| err.to_string())?;

    let mut renderer = Renderer::with_config(args.config).map_err(|err| err.to_string())?;
    renderer
        .load_scene(loaded.scene)
        .map_err(|err| err.to_string())?;

    let mut camera = if args.auto_camera {
        auto_camera(&renderer, args.config)
    } else {
        Camera::default()
    };

    let start = Instant::now();
    let mut last_stats = None;
    for frame_index in 0..args.frames {
        if args.orbit && args.frames > 1 {
            let t = (frame_index as f32) / ((args.frames - 1) as f32);
            let angle = t * 2.0 * PI;
            camera.pose.rotation_xyzw = [0.0, (angle * 0.5).sin(), 0.0, (angle * 0.5).cos()];
        }

        let stats = renderer.render_frame(&camera).map_err(|err| err.to_string())?;
        last_stats = Some(stats);
    }
    let elapsed = start.elapsed();
    let stats = last_stats.unwrap_or_default();

    if let Some(png_path) = args.png_out.as_deref() {
        let rgba = renderer.readback_rgba8().map_err(|err| err.to_string())?;
        write_png(png_path, args.config.width, args.config.height, &rgba)?;
        println!("wrote_png={}", png_path.display());
    }

    println!("desktop-dev ok");
    println!("dataset={}", args.dataset_path.display());
    println!("gpu_rasterizer={}", renderer.has_gpu_rasterizer());
    println!("frames={}", args.frames);
    println!("elapsed_ms={:.4}", elapsed.as_secs_f32() * 1000.0);
    println!("frame_ms={:.4}", stats.frame_ms);
    println!("preprocess_ms={:.4}", stats.preprocess_ms);
    println!("sort_ms={:.4}", stats.sort_ms);
    println!("raster_ms={:.4}", stats.raster_ms);
    println!("visible_count={}", stats.visible_count);
    println!("drawn_count={}", stats.drawn_count);

    Ok(())
}

fn auto_camera(renderer: &Renderer, config: RendererConfig) -> Camera {
    let mut camera = Camera::default();
    camera.intrinsics.vertical_fov_radians = 60.0_f32.to_radians();

    let Some(scene) = renderer.scene() else {
        return camera;
    };
    if scene.positions.is_empty() {
        return camera;
    }

    let mut min = Vec3f::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3f::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in &scene.positions {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        min.z = min.z.min(p.z);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
        max.z = max.z.max(p.z);
    }

    let center = Vec3f::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        (min.z + max.z) * 0.5,
    );
    let extent = Vec3f::new(max.x - min.x, max.y - min.y, max.z - min.z);
    let half_x = (extent.x * 0.5).max(1e-3);
    let half_y = (extent.y * 0.5).max(1e-3);

    let aspect = (config.width as f32) / (config.height as f32);
    let vfov = camera.intrinsics.vertical_fov_radians.max(1e-3);
    let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();

    let dist_y = half_y / (vfov * 0.5).tan();
    let dist_x = half_x / (hfov * 0.5).tan();
    let dist = dist_y.max(dist_x) * 1.2;
    camera.pose.position = Vec3f::new(center.x, center.y, center.z - dist);
    camera.pose.rotation_xyzw = [0.0, 0.0, 0.0, 1.0];

    // Keep conservative planes to avoid accidental clipping when orbiting.
    let radius = half_x.max(half_y).max((extent.z * 0.5).max(1e-3));
    camera.intrinsics.near_plane = (dist - radius * 2.0).max(0.01);
    camera.intrinsics.far_plane = (dist + radius * 8.0).max(100.0);

    camera
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if rgba.len() != (width as usize).saturating_mul(height as usize).saturating_mul(4) {
        return Err("png write failed: rgba buffer size mismatch".to_owned());
    }

    let file = File::create(path).map_err(|err| err.to_string())?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer
        .write_image_data(rgba)
        .map_err(|err| err.to_string())?;
    Ok(())
}
