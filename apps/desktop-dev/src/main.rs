use std::env;
use std::f32::consts::PI;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[cfg(feature = "interactive-viewer")]
use std::time::Duration;

use gsplat_core::{Camera, RenderMode, RendererConfig, Vec3f};
#[cfg(feature = "interactive-viewer")]
use gsplat_core::{CameraIntrinsics, CameraPose};
use gsplat_io_ply::load_ply;
use gsplat_render_wgpu::Renderer;
#[cfg(feature = "interactive-viewer")]
use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};

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
    yaw_deg: Option<f32>,
    interactive: bool,
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
        let mut yaw_deg: Option<f32> = None;
        let mut interactive = false;
        let mut png_out: Option<PathBuf> = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    return Err(usage());
                }
                "--frames" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --frames".to_owned())?;
                    frames = value
                        .parse::<u32>()
                        .map_err(|_| "invalid --frames".to_owned())?;
                    frames = frames.max(1);
                }
                "--width" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --width".to_owned())?;
                    config.width = value
                        .parse::<u32>()
                        .map_err(|_| "invalid --width".to_owned())?;
                }
                "--height" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --height".to_owned())?;
                    config.height = value
                        .parse::<u32>()
                        .map_err(|_| "invalid --height".to_owned())?;
                }
                "--orbit" => orbit = true,
                "--auto-camera" => auto_camera = true,
                "--yaw-deg" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --yaw-deg".to_owned())?;
                    yaw_deg = Some(
                        value
                            .parse::<f32>()
                            .map_err(|_| "invalid --yaw-deg".to_owned())?,
                    );
                }
                "--interactive" => interactive = true,
                "--png" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "missing value for --png".to_owned())?;
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
            yaw_deg,
            interactive,
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
        "  --yaw-deg A      set a fixed yaw angle in degrees for static frame rendering",
        "  --interactive    launch realtime on-screen viewer loop (feature: interactive-viewer)",
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
    if let Some(yaw_deg) = args.yaw_deg {
        let yaw = yaw_deg.to_radians();
        camera.pose.rotation_xyzw = [0.0, (yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()];
    }

    if args.interactive {
        return run_interactive(&args, renderer, camera);
    }

    run_offscreen(&args, renderer, camera)
}

fn run_offscreen(args: &Args, mut renderer: Renderer, mut camera: Camera) -> Result<(), String> {
    let start = Instant::now();
    let mut last_stats = None;
    for frame_index in 0..args.frames {
        if args.orbit && args.frames > 1 {
            let t = (frame_index as f32) / ((args.frames - 1) as f32);
            let angle = t * 2.0 * PI;
            camera.pose.rotation_xyzw = [0.0, (angle * 0.5).sin(), 0.0, (angle * 0.5).cos()];
        }

        let stats = renderer
            .render_frame(&camera)
            .map_err(|err| err.to_string())?;
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

#[cfg(feature = "interactive-viewer")]
fn run_interactive(args: &Args, mut renderer: Renderer, camera: Camera) -> Result<(), String> {
    if !renderer.has_gpu_rasterizer() {
        return Err(
            "interactive mode requires an available GPU rasterizer (adapter not found)".to_owned(),
        );
    }

    let width = args.config.width as usize;
    let height = args.config.height as usize;
    let mut window = Window::new(
        "gsplat-rs viewer",
        width,
        height,
        WindowOptions {
            resize: false,
            ..WindowOptions::default()
        },
    )
    .map_err(|err| format!("window creation failed: {err}"))?;
    window.set_target_fps(60);

    println!("interactive viewer controls:");
    println!("  mouse-left drag / arrow keys: look");
    println!("  W/S: forward/backward, A/D: strafe");
    println!("  Q/E: down/up, Shift: faster, Ctrl: slower");
    println!("  Esc: exit");

    let mut controller = CameraController::new(camera);
    let mut last_frame = Instant::now();
    let mut title_timer = Instant::now();
    let mut title_frames = 0_u32;
    let mut orbit_phase = 0.0_f32;
    let mut xrgb = vec![0_u32; width.saturating_mul(height)];
    let mut last_rgba = vec![0_u8; width.saturating_mul(height).saturating_mul(4)];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let now = Instant::now();
        let dt = (now - last_frame).as_secs_f32().clamp(0.0, 0.1);
        last_frame = now;

        if args.orbit {
            orbit_phase += dt * 0.8;
            controller.set_yaw(orbit_phase);
        } else {
            controller.update(&window, dt);
        }

        let stats = renderer
            .render_frame(&controller.camera())
            .map_err(|err| format!("interactive render failed: {err}"))?;
        last_rgba = renderer
            .readback_rgba8()
            .map_err(|err| format!("interactive readback failed: {err}"))?;
        rgba_to_xrgb(&last_rgba, &mut xrgb)?;
        window
            .update_with_buffer(&xrgb, width, height)
            .map_err(|err| format!("interactive present failed: {err}"))?;

        title_frames = title_frames.saturating_add(1);
        let elapsed = title_timer.elapsed();
        if elapsed >= Duration::from_millis(500) {
            let fps = (title_frames as f32) / elapsed.as_secs_f32().max(1e-6);
            window.set_title(&format!(
                "gsplat-rs viewer | fps={fps:.1} frame={:.2}ms visible={} drawn={}",
                stats.frame_ms, stats.visible_count, stats.drawn_count
            ));
            title_timer = Instant::now();
            title_frames = 0;
        }
    }

    if let Some(png_path) = args.png_out.as_deref() {
        write_png(png_path, args.config.width, args.config.height, &last_rgba)?;
        println!("wrote_png={}", png_path.display());
    }

    println!("interactive-viewer exited");
    Ok(())
}

#[cfg(not(feature = "interactive-viewer"))]
fn run_interactive(_args: &Args, _renderer: Renderer, _camera: Camera) -> Result<(), String> {
    Err("interactive mode is not enabled. re-run with `--features interactive-viewer`".to_owned())
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone)]
struct CameraController {
    position: Vec3f,
    intrinsics: CameraIntrinsics,
    yaw: f32,
    pitch: f32,
    move_speed: f32,
    look_speed: f32,
    mouse_sensitivity: f32,
    last_mouse: Option<(f32, f32)>,
}

#[cfg(feature = "interactive-viewer")]
impl CameraController {
    fn new(camera: Camera) -> Self {
        Self {
            position: camera.pose.position,
            intrinsics: camera.intrinsics,
            yaw: 0.0,
            pitch: 0.0,
            move_speed: 2.0,
            look_speed: 1.8,
            mouse_sensitivity: 0.003,
            last_mouse: None,
        }
    }

    fn set_yaw(&mut self, yaw: f32) {
        self.yaw = yaw;
    }

    fn camera(&self) -> Camera {
        Camera {
            pose: CameraPose {
                position: self.position,
                rotation_xyzw: quat_from_yaw_pitch(self.yaw, self.pitch),
            },
            intrinsics: self.intrinsics,
        }
    }

    fn update(&mut self, window: &Window, dt: f32) {
        let mut yaw_delta = 0.0_f32;
        let mut pitch_delta = 0.0_f32;
        if window.is_key_down(Key::Left) {
            yaw_delta -= 1.0;
        }
        if window.is_key_down(Key::Right) {
            yaw_delta += 1.0;
        }
        if window.is_key_down(Key::Up) {
            pitch_delta += 1.0;
        }
        if window.is_key_down(Key::Down) {
            pitch_delta -= 1.0;
        }

        self.yaw += yaw_delta * self.look_speed * dt;
        self.pitch = (self.pitch + pitch_delta * self.look_speed * dt).clamp(-1.45, 1.45);

        if window.get_mouse_down(MouseButton::Left) {
            if let Some((mx, my)) = window.get_mouse_pos(MouseMode::Pass) {
                if let Some((last_x, last_y)) = self.last_mouse {
                    self.yaw += (mx - last_x) * self.mouse_sensitivity;
                    self.pitch =
                        (self.pitch - (my - last_y) * self.mouse_sensitivity).clamp(-1.45, 1.45);
                }
                self.last_mouse = Some((mx, my));
            }
        } else {
            self.last_mouse = None;
        }

        let mut forward_axis = 0.0_f32;
        let mut right_axis = 0.0_f32;
        let mut up_axis = 0.0_f32;
        if window.is_key_down(Key::W) {
            forward_axis += 1.0;
        }
        if window.is_key_down(Key::S) {
            forward_axis -= 1.0;
        }
        if window.is_key_down(Key::D) {
            right_axis += 1.0;
        }
        if window.is_key_down(Key::A) {
            right_axis -= 1.0;
        }
        if window.is_key_down(Key::E) {
            up_axis += 1.0;
        }
        if window.is_key_down(Key::Q) {
            up_axis -= 1.0;
        }

        let mut speed = self.move_speed;
        if window.is_key_down(Key::LeftShift) || window.is_key_down(Key::RightShift) {
            speed *= 4.0;
        }
        if window.is_key_down(Key::LeftCtrl) || window.is_key_down(Key::RightCtrl) {
            speed *= 0.25;
        }

        let rotation = quat_from_yaw_pitch(self.yaw, self.pitch);
        let forward = quat_rotate_vec3(rotation, Vec3f::new(0.0, 0.0, 1.0));
        let right = quat_rotate_vec3(rotation, Vec3f::new(1.0, 0.0, 0.0));
        let up = Vec3f::new(0.0, 1.0, 0.0);

        let desired = Vec3f::new(
            forward.x * forward_axis + right.x * right_axis + up.x * up_axis,
            forward.y * forward_axis + right.y * right_axis + up.y * up_axis,
            forward.z * forward_axis + right.z * right_axis + up.z * up_axis,
        );
        let desired = normalize_or_zero(desired);
        self.position = Vec3f::new(
            self.position.x + desired.x * speed * dt,
            self.position.y + desired.y * speed * dt,
            self.position.z + desired.z * speed * dt,
        );

        if let Some((_, scroll_y)) = window.get_scroll_wheel() {
            if scroll_y != 0.0 {
                self.position = Vec3f::new(
                    self.position.x + forward.x * scroll_y * speed * 0.2,
                    self.position.y + forward.y * scroll_y * speed * 0.2,
                    self.position.z + forward.z * scroll_y * speed * 0.2,
                );
            }
        }
    }
}

#[cfg(feature = "interactive-viewer")]
fn normalize_or_zero(v: Vec3f) -> Vec3f {
    let len_sq = v.x * v.x + v.y * v.y + v.z * v.z;
    if len_sq <= 1e-12 {
        return Vec3f::new(0.0, 0.0, 0.0);
    }

    let inv_len = len_sq.sqrt().recip();
    Vec3f::new(v.x * inv_len, v.y * inv_len, v.z * inv_len)
}

#[cfg(feature = "interactive-viewer")]
fn quat_from_yaw_pitch(yaw: f32, pitch: f32) -> [f32; 4] {
    let q_yaw = [0.0, (yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()];
    let q_pitch = [(pitch * 0.5).sin(), 0.0, 0.0, (pitch * 0.5).cos()];
    quat_normalize(quat_mul(q_yaw, q_pitch))
}

#[cfg(feature = "interactive-viewer")]
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

#[cfg(feature = "interactive-viewer")]
fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let len_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if len_sq <= 1e-12 {
        return [0.0, 0.0, 0.0, 1.0];
    }

    let inv_len = len_sq.sqrt().recip();
    [
        q[0] * inv_len,
        q[1] * inv_len,
        q[2] * inv_len,
        q[3] * inv_len,
    ]
}

#[cfg(feature = "interactive-viewer")]
fn quat_rotate_vec3(q: [f32; 4], v: Vec3f) -> Vec3f {
    let u = Vec3f::new(q[0], q[1], q[2]);
    let s = q[3];
    let uv = cross(u, v);
    let uuv = cross(u, uv);
    Vec3f::new(
        v.x + 2.0 * (s * uv.x + uuv.x),
        v.y + 2.0 * (s * uv.y + uuv.y),
        v.z + 2.0 * (s * uv.z + uuv.z),
    )
}

#[cfg(feature = "interactive-viewer")]
fn cross(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

#[cfg(feature = "interactive-viewer")]
fn rgba_to_xrgb(rgba: &[u8], out: &mut [u32]) -> Result<(), String> {
    let pixel_count = rgba.len() / 4;
    if out.len() != pixel_count || rgba.len() % 4 != 0 {
        return Err("interactive present failed: rgba/xrgb buffer size mismatch".to_owned());
    }

    for (dst, px) in out.iter_mut().zip(rgba.chunks_exact(4)) {
        *dst = ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | (px[2] as u32);
    }

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
    let half_z = (extent.z * 0.5).max(1e-3);

    let aspect = (config.width as f32) / (config.height as f32);
    let vfov = camera.intrinsics.vertical_fov_radians.max(1e-3);
    let hfov = 2.0 * ((vfov * 0.5).tan() * aspect).atan();

    let dist_y = half_y / (vfov * 0.5).tan();
    let dist_x = half_x / (hfov * 0.5).tan();
    // Keep enough standoff for thick scenes; using only x/y fit can place the camera too close
    // to the frontmost Gaussians and amplify projection anisotropy into visible streaking.
    let base_dist = dist_y.max(dist_x);
    let depth_aware_dist = base_dist + half_z;
    let dist = depth_aware_dist * 1.2;
    camera.pose.position = Vec3f::new(center.x, center.y, center.z - dist);
    camera.pose.rotation_xyzw = [0.0, 0.0, 0.0, 1.0];

    // Keep conservative planes to avoid accidental clipping when orbiting.
    let radius = half_x.max(half_y).max((extent.z * 0.5).max(1e-3));
    camera.intrinsics.near_plane = (dist - radius * 2.0).max(0.01);
    camera.intrinsics.far_plane = (dist + radius * 8.0).max(100.0);

    camera
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if rgba.len()
        != (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    {
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
