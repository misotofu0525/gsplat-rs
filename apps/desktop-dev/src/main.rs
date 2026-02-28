use std::env;
use std::f32::consts::PI;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[cfg(feature = "interactive-viewer")]
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use gsplat_core::{Camera, RenderMode, RendererConfig, Vec3f};
#[cfg(feature = "interactive-viewer")]
use gsplat_core::{CameraIntrinsics, CameraPose};
use gsplat_io_ply::load_ply;
use gsplat_render_wgpu::Renderer;
#[cfg(feature = "interactive-viewer")]
use gsplat_render_wgpu::{GpuInstance, GpuInstancePreprocessor};
#[cfg(feature = "interactive-viewer")]
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes},
};

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
    if args.png_out.is_some() {
        return Err("interactive mode does not support --png in surface-present path".to_owned());
    }
    let event_loop =
        EventLoop::new().map_err(|err| format!("event loop creation failed: {err}"))?;
    let window = Arc::new(
        event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("gsplat-rs viewer")
                    .with_inner_size(PhysicalSize::new(args.config.width, args.config.height)),
            )
            .map_err(|err| format!("window creation failed: {err}"))?,
    );
    let mut presenter = pollster::block_on(SurfacePresenter::new(
        window.clone(),
        args.config.width,
        args.config.height,
        &renderer,
    ))?;

    println!("interactive viewer controls:");
    println!("  mouse-left drag / arrow keys: orbit around scene");
    println!("  W/S or wheel: dolly in/out, A/D/Q/E: pan");
    println!("  Q/E: down/up, Shift: faster, Ctrl: slower");
    println!("  Esc: exit");

    let orbit_target = renderer
        .scene()
        .and_then(scene_center)
        .unwrap_or(Vec3f::new(0.0, 0.0, 0.0));
    let mut controller = CameraController::new(camera, orbit_target);
    let mut input = InputState::default();
    let mut last_frame = Instant::now();
    let mut title_timer = Instant::now();
    let mut title_frames = 0_u32;
    let mut orbit_phase = 0.0_f32;
    let window_id = window.id();
    let render_error = Arc::new(Mutex::new(None::<String>));
    let render_error_shared = render_error.clone();

    event_loop
        .run(move |event, target| match event {
            Event::AboutToWait => {
                window.request_redraw();
            }
            Event::WindowEvent {
                window_id: id,
                event,
            } if id == window_id => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Resized(size) => {
                    presenter.resize(size.width, size.height);
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if code == KeyCode::Escape && event.state == ElementState::Pressed {
                            target.exit();
                            return;
                        }
                        match event.state {
                            ElementState::Pressed => {
                                input.keys_down.insert(code);
                            }
                            ElementState::Released => {
                                input.keys_down.remove(&code);
                            }
                        }
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    if button == MouseButton::Left {
                        input.mouse_left_down = state == ElementState::Pressed;
                        if !input.mouse_left_down {
                            input.last_cursor = None;
                        }
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let current = (position.x as f32, position.y as f32);
                    if input.mouse_left_down {
                        if let Some((lx, ly)) = input.last_cursor {
                            input.mouse_delta.0 += current.0 - lx;
                            input.mouse_delta.1 += current.1 - ly;
                        }
                        input.last_cursor = Some(current);
                    } else {
                        input.last_cursor = Some(current);
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => match delta {
                    MouseScrollDelta::LineDelta(_, y) => input.scroll_y += y,
                    MouseScrollDelta::PixelDelta(p) => input.scroll_y += (p.y as f32) / 100.0,
                },
                WindowEvent::RedrawRequested => {
                    let now = Instant::now();
                    let dt = (now - last_frame).as_secs_f32().clamp(0.0, 0.1);
                    last_frame = now;

                    if args.orbit {
                        orbit_phase += dt * 0.8;
                        controller.set_yaw(orbit_phase);
                    } else {
                        controller.update(&input, dt);
                    }

                    let camera_now = controller.camera();
                    let (sorted_indices, stats) = match renderer.build_sorted_indices(&camera_now) {
                        Ok(v) => v,
                        Err(err) => {
                            if let Ok(mut slot) = render_error_shared.lock() {
                                *slot = Some(format!("interactive prepare failed: {err}"));
                            }
                            target.exit();
                            return;
                        }
                    };

                    if let Err(err) = presenter.render(&sorted_indices, &camera_now) {
                        if let Ok(mut slot) = render_error_shared.lock() {
                            *slot = Some(format!("interactive present failed: {err}"));
                        }
                        target.exit();
                        return;
                    }

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

                    input.end_frame();
                }
                _ => {}
            },
            _ => {}
        })
        .map_err(|err| format!("interactive event loop failed: {err}"))?;

    if let Some(err) = render_error
        .lock()
        .map_err(|_| "interactive error state lock poisoned".to_owned())?
        .take()
    {
        return Err(err);
    }
    println!("interactive-viewer exited");
    Ok(())
}

#[cfg(not(feature = "interactive-viewer"))]
fn run_interactive(_args: &Args, _renderer: Renderer, _camera: Camera) -> Result<(), String> {
    Err("interactive mode is not enabled. re-run with `--features interactive-viewer`".to_owned())
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Default, Clone)]
struct InputState {
    keys_down: HashSet<KeyCode>,
    mouse_left_down: bool,
    last_cursor: Option<(f32, f32)>,
    mouse_delta: (f32, f32),
    scroll_y: f32,
}

#[cfg(feature = "interactive-viewer")]
impl InputState {
    fn is_key_down(&self, key: KeyCode) -> bool {
        self.keys_down.contains(&key)
    }

    fn end_frame(&mut self) {
        self.mouse_delta = (0.0, 0.0);
        self.scroll_y = 0.0;
    }
}

#[cfg(feature = "interactive-viewer")]
struct SurfacePresenter {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    preprocessor: GpuInstancePreprocessor,
    pipeline: wgpu::RenderPipeline,
    surface_config: wgpu::SurfaceConfiguration,
    _instance_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

#[cfg(feature = "interactive-viewer")]
impl SurfacePresenter {
    async fn new(
        window: Arc<Window>,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("invalid surface size".to_owned());
        }

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|err| format!("surface creation failed: {err}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| "surface adapter unavailable".to_owned())?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("desktop-dev-surface-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|err| format!("surface device creation failed: {err}"))?;

        let caps = surface.get_capabilities(&adapter);
        let Some(format) = caps.formats.first().copied() else {
            return Err("surface has no compatible format".to_owned());
        };
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
        let alpha_mode = caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Opaque);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let render_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("desktop-dev-splat-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../crates/gsplat-render-wgpu/shaders/splat.wgsl").into(),
            ),
        });

        let render_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("desktop-dev-splat-bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("desktop-dev-splat-bgl"),
                bind_group_layouts: &[&render_bind_group_layout],
                immediate_size: 0,
            });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("desktop-dev-splat-rp"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let scene_len = renderer
            .scene()
            .map(|scene| scene.len().max(1))
            .ok_or_else(|| "interactive mode requires a loaded scene".to_owned())?;
        let (instance_buffer, bind_group) =
            create_surface_instance_resources(&device, &render_bind_group_layout, scene_len);
        let preprocessor = renderer
            .create_gpu_instance_preprocessor(&device, &instance_buffer, scene_len)
            .map_err(|err| format!("interactive preprocess init failed: {err}"))?;

        Ok(Self {
            surface,
            device,
            queue,
            preprocessor,
            pipeline,
            surface_config,
            _instance_buffer: instance_buffer,
            bind_group,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    fn render(&mut self, sorted_indices: &[u32], camera: &Camera) -> Result<(), String> {
        let instance_count = self
            .preprocessor
            .prepare(
                &self.queue,
                sorted_indices,
                camera,
                self.surface_config.width,
                self.surface_config.height,
            )
            .map_err(|err| err.to_string())?;

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(());
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(()),
            Err(wgpu::SurfaceError::OutOfMemory) => {
                return Err("surface out of memory".to_owned());
            }
            Err(wgpu::SurfaceError::Other) => return Err("surface acquire failed".to_owned()),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("desktop-dev-surface-encoder"),
            });
        self.preprocessor
            .encode_dispatch(&mut encoder, instance_count);
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("desktop-dev-surface-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, &self.bind_group, &[]);
            if instance_count > 0 {
                rpass.draw(0..6, 0..instance_count);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

#[cfg(feature = "interactive-viewer")]
fn create_surface_instance_resources(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    capacity: usize,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let stride = std::mem::size_of::<GpuInstance>() as u64;
    let size = stride * (capacity.max(1) as u64);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("desktop-dev-surface-instance-buffer"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("desktop-dev-surface-bg"),
        layout: bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    (buffer, bind_group)
}

#[cfg(feature = "interactive-viewer")]
#[derive(Debug, Clone)]
struct CameraController {
    position: Vec3f,
    target: Vec3f,
    distance: f32,
    intrinsics: CameraIntrinsics,
    yaw: f32,
    pitch: f32,
    move_speed: f32,
    look_speed: f32,
    mouse_sensitivity: f32,
}

#[cfg(feature = "interactive-viewer")]
impl CameraController {
    fn new(camera: Camera, target: Vec3f) -> Self {
        let to_target = Vec3f::new(
            target.x - camera.pose.position.x,
            target.y - camera.pose.position.y,
            target.z - camera.pose.position.z,
        );
        let mut distance = (to_target.x * to_target.x + to_target.y * to_target.y + to_target.z * to_target.z)
            .sqrt();
        if !distance.is_finite() || distance < 0.05 {
            distance = 1.0;
        }
        let forward = if distance > 1e-6 {
            Vec3f::new(to_target.x / distance, to_target.y / distance, to_target.z / distance)
        } else {
            quat_rotate_vec3(camera.pose.rotation_xyzw, Vec3f::new(0.0, 0.0, 1.0))
        };
        let mut controller = Self {
            position: camera.pose.position,
            target,
            distance,
            intrinsics: camera.intrinsics,
            yaw: forward.x.atan2(forward.z),
            pitch: (-forward.y).asin().clamp(-1.45, 1.45),
            move_speed: 2.0,
            look_speed: 1.8,
            mouse_sensitivity: 0.003,
        };
        controller.sync_position_from_orbit();
        controller
    }

    fn set_yaw(&mut self, yaw: f32) {
        self.yaw = yaw;
        self.sync_position_from_orbit();
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

    fn update(&mut self, input: &InputState, dt: f32) {
        let mut yaw_delta = 0.0_f32;
        let mut pitch_delta = 0.0_f32;
        if input.is_key_down(KeyCode::ArrowLeft) {
            yaw_delta -= 1.0;
        }
        if input.is_key_down(KeyCode::ArrowRight) {
            yaw_delta += 1.0;
        }
        if input.is_key_down(KeyCode::ArrowUp) {
            pitch_delta += 1.0;
        }
        if input.is_key_down(KeyCode::ArrowDown) {
            pitch_delta -= 1.0;
        }

        self.yaw += yaw_delta * self.look_speed * dt;
        self.pitch = (self.pitch + pitch_delta * self.look_speed * dt).clamp(-1.45, 1.45);

        self.yaw += input.mouse_delta.0 * self.mouse_sensitivity;
        self.pitch = (self.pitch - input.mouse_delta.1 * self.mouse_sensitivity).clamp(-1.45, 1.45);

        let mut dolly_axis = 0.0_f32;
        let mut right_axis = 0.0_f32;
        let mut up_axis = 0.0_f32;
        if input.is_key_down(KeyCode::KeyW) {
            dolly_axis += 1.0;
        }
        if input.is_key_down(KeyCode::KeyS) {
            dolly_axis -= 1.0;
        }
        if input.is_key_down(KeyCode::KeyD) {
            right_axis += 1.0;
        }
        if input.is_key_down(KeyCode::KeyA) {
            right_axis -= 1.0;
        }
        if input.is_key_down(KeyCode::KeyE) {
            up_axis += 1.0;
        }
        if input.is_key_down(KeyCode::KeyQ) {
            up_axis -= 1.0;
        }

        let mut speed = self.move_speed;
        if input.is_key_down(KeyCode::ShiftLeft) || input.is_key_down(KeyCode::ShiftRight) {
            speed *= 4.0;
        }
        if input.is_key_down(KeyCode::ControlLeft) || input.is_key_down(KeyCode::ControlRight) {
            speed *= 0.25;
        }

        let rotation = quat_from_yaw_pitch(self.yaw, self.pitch);
        let right = quat_rotate_vec3(rotation, Vec3f::new(1.0, 0.0, 0.0));
        let up = quat_rotate_vec3(rotation, Vec3f::new(0.0, 1.0, 0.0));

        let pan = Vec3f::new(
            right.x * right_axis + up.x * up_axis,
            right.y * right_axis + up.y * up_axis,
            right.z * right_axis + up.z * up_axis,
        );
        let pan = normalize_or_zero(pan);
        let pan_scale = speed * dt * self.distance * 0.5;
        self.target = Vec3f::new(
            self.target.x + pan.x * pan_scale,
            self.target.y + pan.y * pan_scale,
            self.target.z + pan.z * pan_scale,
        );

        let dolly = dolly_axis * speed * dt + input.scroll_y * speed * 0.2;
        self.distance = (self.distance - dolly).clamp(0.05, 1.0e5);
        self.sync_position_from_orbit();
    }

    fn sync_position_from_orbit(&mut self) {
        let rotation = quat_from_yaw_pitch(self.yaw, self.pitch);
        let forward = quat_rotate_vec3(rotation, Vec3f::new(0.0, 0.0, 1.0));
        self.position = Vec3f::new(
            self.target.x - forward.x * self.distance,
            self.target.y - forward.y * self.distance,
            self.target.z - forward.z * self.distance,
        );
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

fn auto_camera(renderer: &Renderer, config: RendererConfig) -> Camera {
    let mut camera = Camera::default();
    camera.intrinsics.vertical_fov_radians = 60.0_f32.to_radians();

    let Some(scene) = renderer.scene() else {
        return camera;
    };
    let Some((min, max)) = scene_bounds(scene) else {
        return camera;
    };

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

fn scene_bounds(scene: &gsplat_core::SceneBuffers) -> Option<(Vec3f, Vec3f)> {
    if scene.positions.is_empty() {
        return None;
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
    Some((min, max))
}

#[cfg(feature = "interactive-viewer")]
fn scene_center(scene: &gsplat_core::SceneBuffers) -> Option<Vec3f> {
    let (min, max) = scene_bounds(scene)?;
    Some(Vec3f::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        (min.z + max.z) * 0.5,
    ))
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
