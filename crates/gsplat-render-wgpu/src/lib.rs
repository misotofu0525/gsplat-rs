//! WGPU renderer with a SortedAlpha reference path.

use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use gsplat_core::{Camera, ErrorCode, FrameStats, RenderMode, RendererConfig, SceneBuffers, Vec3f};
use gsplat_sort::{CpuSortBackend, GpuOddEvenSortBackend, SortBackend, SortError};
use thiserror::Error;

const RENDER_TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessOutput {
    pub depth_keys: Vec<u32>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Error)]
pub enum RendererError {
    #[error("invalid renderer configuration")]
    InvalidConfig,
    #[error("scene not loaded")]
    SceneNotLoaded,
    #[error("invalid scene buffers")]
    InvalidScene,
    #[error("gpu rasterizer unavailable")]
    GpuRasterizerUnavailable,
    #[error("gpu readback failed")]
    GpuReadback,
    #[error("sort backend error: {0}")]
    Sort(#[from] SortError),
}

impl RendererError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidConfig | Self::InvalidScene => ErrorCode::InvalidArgument,
            Self::SceneNotLoaded => ErrorCode::SceneNotLoaded,
            Self::GpuRasterizerUnavailable => ErrorCode::Unsupported,
            Self::GpuReadback => ErrorCode::Internal,
            Self::Sort(_) => ErrorCode::Internal,
        }
    }
}

pub struct Renderer {
    mode: RenderMode,
    config: RendererConfig,
    gpu_sort_backend: GpuOddEvenSortBackend,
    cpu_sort_backend: CpuSortBackend,
    gpu_rasterizer: Option<GpuRasterizer>,
    scene: Option<SceneBuffers>,
    last_stats: FrameStats,
}

impl Renderer {
    pub fn new(mode: RenderMode) -> Result<Self, RendererError> {
        let config = RendererConfig {
            mode,
            ..RendererConfig::default()
        };
        Self::with_config(config)
    }

    pub fn with_config(config: RendererConfig) -> Result<Self, RendererError> {
        config
            .validate()
            .map_err(|_| RendererError::InvalidConfig)?;

        let gpu_rasterizer = GpuRasterizer::create(&config).ok();

        Ok(Self {
            mode: config.mode,
            config,
            gpu_sort_backend: GpuOddEvenSortBackend::default(),
            cpu_sort_backend: CpuSortBackend,
            gpu_rasterizer,
            scene: None,
            last_stats: FrameStats::zero(),
        })
    }

    pub fn config(&self) -> RendererConfig {
        self.config
    }

    pub fn mode(&self) -> RenderMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: RenderMode) {
        self.mode = mode;
        self.config.mode = mode;
    }

    pub fn has_gpu_rasterizer(&self) -> bool {
        self.gpu_rasterizer.is_some()
    }

    pub fn load_scene(&mut self, scene: SceneBuffers) -> Result<(), RendererError> {
        scene.validate().map_err(|_| RendererError::InvalidScene)?;
        self.scene = Some(scene);
        Ok(())
    }

    pub fn scene(&self) -> Option<&SceneBuffers> {
        self.scene.as_ref()
    }

    pub fn preprocess_visible(&self, camera: &Camera) -> Result<PreprocessOutput, RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;

        let mut indices = Vec::with_capacity(scene.len());
        let mut depth_keys = Vec::with_capacity(scene.len());

        for (idx, position) in scene.positions.iter().enumerate() {
            let p_cam = world_to_camera(*position, camera);
            if is_visible(p_cam.z, camera) {
                indices.push(idx as u32);
                depth_keys.push(depth_to_key(p_cam.z));
            }
        }

        Ok(PreprocessOutput {
            depth_keys,
            indices,
        })
    }

    pub fn render_frame(&mut self, camera: &Camera) -> Result<FrameStats, RendererError> {
        let frame_start = Instant::now();

        let preprocess_start = Instant::now();
        let mut preprocessed = self.preprocess_visible(camera)?;
        let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

        let sort_start = Instant::now();
        if self.mode == RenderMode::SortedAlpha {
            let gpu_sort_result = self
                .gpu_sort_backend
                .sort_pairs(&mut preprocessed.depth_keys, &mut preprocessed.indices);

            match gpu_sort_result {
                Ok(()) => {}
                Err(SortError::BackendUnavailable | SortError::BackendFailure) => {
                    self.cpu_sort_backend
                        .sort_pairs(&mut preprocessed.depth_keys, &mut preprocessed.indices)?;
                }
                Err(err) => return Err(RendererError::Sort(err)),
            }
        }
        let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

        let raster_start = Instant::now();
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let instances = build_instances(scene, &preprocessed.indices, camera, self.config);

        if let Some(gpu_rasterizer) = self.gpu_rasterizer.as_mut() {
            if gpu_rasterizer.render(self.config, &instances).is_err() {
                // If GPU path fails during runtime, drop to CPU placeholder path.
                self.gpu_rasterizer = None;
            }
        }

        let drawn_count = instances.len() as u32;
        let raster_ms = raster_start.elapsed().as_secs_f32() * 1000.0;

        let stats = FrameStats {
            frame_ms: frame_start.elapsed().as_secs_f32() * 1000.0,
            preprocess_ms,
            sort_ms,
            raster_ms,
            visible_count: preprocessed.indices.len() as u32,
            drawn_count,
        };

        self.last_stats = stats;
        Ok(stats)
    }

    pub fn last_stats(&self) -> FrameStats {
        self.last_stats
    }

    pub fn readback_rgba8(&mut self) -> Result<Vec<u8>, RendererError> {
        let rasterizer = self
            .gpu_rasterizer
            .as_mut()
            .ok_or(RendererError::GpuRasterizerUnavailable)?;
        rasterizer
            .readback_rgba8()
            .map_err(|_| RendererError::GpuReadback)
    }

    pub fn render_placeholder(&mut self) -> FrameStats {
        self.render_frame(&Camera::default()).unwrap_or_default()
    }
}

fn is_visible(depth_z: f32, camera: &Camera) -> bool {
    depth_z >= camera.intrinsics.near_plane && depth_z <= camera.intrinsics.far_plane
}

fn depth_to_key(depth_z: f32) -> u32 {
    // Positive finite depth values preserve a monotonic relationship when using IEEE-754 bits.
    depth_z.max(0.0).to_bits()
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuInstance {
    pos_xy: [f32; 2],
    size: f32,
    _pad0: f32,
    color_rgba: [f32; 4],
}

fn build_instances(
    scene: &SceneBuffers,
    indices: &[u32],
    camera: &Camera,
    config: RendererConfig,
) -> Vec<GpuInstance> {
    let mut out = Vec::with_capacity(indices.len());
    let aspect = config.width as f32 / config.height as f32;
    let f = 1.0 / (camera.intrinsics.vertical_fov_radians * 0.5).tan();

    for &idx in indices {
        let i = idx as usize;
        let pos_world = scene.positions[i];
        let p_cam = world_to_camera(pos_world, camera);
        // Preprocess already culled by z range, but keep this safe for runtime camera changes.
        if !is_visible(p_cam.z, camera) {
            continue;
        }

        let inv_z = 1.0 / p_cam.z.max(1e-6);
        let x_ndc = (p_cam.x * f) * inv_z / aspect;
        let y_ndc = (p_cam.y * f) * inv_z;

        // 3DGS stores log-scales; runtime applies exp() to restore linear sigma.
        let scale = scene.scale_xyz[i];
        let sx = scale[0].exp();
        let sy = scale[1].exp();
        let sz = scale[2].exp();
        let sigma = sx.max(sy).max(sz);
        let size = (sigma * f) * inv_z;
        let size = size.clamp(0.0005, 0.2);
        let alpha = sigmoid(scene.opacity[i]).clamp(0.0, 1.0);
        let dir_world = normalize3(Vec3f::new(
            pos_world.x - camera.pose.position.x,
            pos_world.y - camera.pose.position.y,
            pos_world.z - camera.pose.position.z,
        ));
        let rgb = sh_color(scene, i, dir_world);
        let rgb = [
            rgb[0].clamp(0.0, 1.0),
            rgb[1].clamp(0.0, 1.0),
            rgb[2].clamp(0.0, 1.0),
        ];

        out.push(GpuInstance {
            pos_xy: [x_ndc, y_ndc],
            size,
            _pad0: 0.0,
            color_rgba: [
                (rgb[0] * alpha).clamp(0.0, 1.0),
                (rgb[1] * alpha).clamp(0.0, 1.0),
                (rgb[2] * alpha).clamp(0.0, 1.0),
                alpha,
            ],
        });
    }

    out
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn world_to_camera(pos_world: Vec3f, camera: &Camera) -> Vec3f {
    let p = Vec3f::new(
        pos_world.x - camera.pose.position.x,
        pos_world.y - camera.pose.position.y,
        pos_world.z - camera.pose.position.z,
    );

    let inv_q = quat_inverse(camera.pose.rotation_xyzw);
    quat_rotate(inv_q, p)
}

fn quat_inverse(q: [f32; 4]) -> [f32; 4] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let norm2 = x * x + y * y + z * z + w * w;
    if norm2 <= 0.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    [-x / norm2, -y / norm2, -z / norm2, w / norm2]
}

fn quat_rotate(q: [f32; 4], v: Vec3f) -> Vec3f {
    // v' = v + 2w(qxyz x v) + 2(qxyz x (qxyz x v))
    let qv = Vec3f::new(q[0], q[1], q[2]);
    let t = cross(qv, v);
    let t2 = Vec3f::new(t.x * 2.0, t.y * 2.0, t.z * 2.0);
    let v_wt = Vec3f::new(v.x + q[3] * t2.x, v.y + q[3] * t2.y, v.z + q[3] * t2.z);
    let c = cross(qv, t2);
    Vec3f::new(v_wt.x + c.x, v_wt.y + c.y, v_wt.z + c.z)
}

fn cross(a: Vec3f, b: Vec3f) -> Vec3f {
    Vec3f::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn normalize3(v: Vec3f) -> [f32; 3] {
    let len2 = v.x * v.x + v.y * v.y + v.z * v.z;
    if len2 <= 0.0 {
        return [0.0, 0.0, 1.0];
    }
    let inv = 1.0 / len2.sqrt();
    [v.x * inv, v.y * inv, v.z * inv]
}

fn sh_color(scene: &SceneBuffers, index: usize, dir: [f32; 3]) -> [f32; 3] {
    // The PLY stores SH coefficients as `f_dc_*` + `f_rest_*`. Evaluate as in 3DGS:
    // `rgb = clamp_min(eval_sh(deg, sh, dir) + 0.5, 0.0)`.
    // Reference: graphdeco-inria/gaussian-splatting `utils/sh_utils.py`.
    let deg = if scene.sh_rest.is_some() {
        scene.sh_degree
    } else {
        0
    };

    let dc = scene.color_dc[index];
    let sh_eval_dc = [
        eval_sh_channel(deg, dc[0], sh_rest_channel(scene, index, 0), dir),
        eval_sh_channel(deg, dc[1], sh_rest_channel(scene, index, 1), dir),
        eval_sh_channel(deg, dc[2], sh_rest_channel(scene, index, 2), dir),
    ];

    [
        (sh_eval_dc[0] + 0.5).max(0.0),
        (sh_eval_dc[1] + 0.5).max(0.0),
        (sh_eval_dc[2] + 0.5).max(0.0),
    ]
}

fn sh_rest_channel(scene: &SceneBuffers, index: usize, channel: usize) -> &[f32] {
    let Some(rest) = scene.sh_rest.as_ref() else {
        return &[];
    };

    let coeff_total = (scene.sh_degree as usize + 1).pow(2);
    let per_channel = coeff_total.saturating_sub(1);
    let stride = per_channel * 3;
    let base = index * stride;
    let channel_base = base + channel * per_channel;
    &rest[channel_base..channel_base + per_channel]
}

fn eval_sh_channel(deg: u8, dc_coeff: f32, rest: &[f32], dir: [f32; 3]) -> f32 {
    const C0: f32 = 0.28209479177387814_f32;
    const C1: f32 = 0.4886025119029199_f32;
    const C2: [f32; 5] = [
        1.0925484305920792_f32,
        -1.0925484305920792_f32,
        0.31539156525252005_f32,
        -1.0925484305920792_f32,
        0.5462742152960396_f32,
    ];
    const C3: [f32; 7] = [
        -0.5900435899266435_f32,
        2.890611442640554_f32,
        -0.4570457994644658_f32,
        0.3731763325901154_f32,
        -0.4570457994644658_f32,
        1.445305721320277_f32,
        -0.5900435899266435_f32,
    ];
    const C4: [f32; 9] = [
        2.5033429417967046_f32,
        -1.7701307697799304_f32,
        0.9461746957575601_f32,
        -0.6690465435572892_f32,
        0.10578554691520431_f32,
        -0.6690465435572892_f32,
        0.47308734787878004_f32,
        -1.7701307697799304_f32,
        0.6258357354491761_f32,
    ];

    let mut result = C0 * dc_coeff;
    if deg == 0 {
        return result;
    }

    let x = dir[0];
    let y = dir[1];
    let z = dir[2];
    if rest.len() < 3 {
        return result;
    }

    result = result - C1 * y * rest[0] + C1 * z * rest[1] - C1 * x * rest[2];
    if deg == 1 {
        return result;
    }

    if rest.len() < 8 {
        return result;
    }
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let yz = y * z;
    let xz = x * z;

    result = result
        + C2[0] * xy * rest[3]
        + C2[1] * yz * rest[4]
        + C2[2] * (2.0 * zz - xx - yy) * rest[5]
        + C2[3] * xz * rest[6]
        + C2[4] * (xx - yy) * rest[7];

    if deg == 2 {
        return result;
    }

    if rest.len() < 15 {
        return result;
    }

    result = result
        + C3[0] * y * (3.0 * xx - yy) * rest[8]
        + C3[1] * xy * z * rest[9]
        + C3[2] * y * (4.0 * zz - xx - yy) * rest[10]
        + C3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy) * rest[11]
        + C3[4] * x * (4.0 * zz - xx - yy) * rest[12]
        + C3[5] * z * (xx - yy) * rest[13]
        + C3[6] * x * (xx - 3.0 * yy) * rest[14];

    if deg == 3 {
        return result;
    }

    if rest.len() < 24 {
        return result;
    }

    // deg 4 support (rare for 3DGS, but safe to handle).
    result = result
        + C4[0] * xy * (xx - yy) * rest[15]
        + C4[1] * yz * (3.0 * xx - yy) * rest[16]
        + C4[2] * xy * (7.0 * zz - 1.0) * rest[17]
        + C4[3] * yz * (7.0 * zz - 3.0) * rest[18]
        + C4[4] * (zz * (35.0 * zz - 30.0) + 3.0) * rest[19]
        + C4[5] * xz * (7.0 * zz - 3.0) * rest[20]
        + C4[6] * (xx - yy) * (7.0 * zz - 1.0) * rest[21]
        + C4[7] * xz * (xx - 3.0 * yy) * rest[22]
        + C4[8] * (xx * (xx - 3.0 * yy) - yy * (3.0 * xx - yy)) * rest[23];

    result
}

#[derive(Debug, Error)]
enum GpuRasterError {
    #[error("no compatible wgpu adapter")]
    AdapterUnavailable,
    #[error("wgpu device creation failed")]
    DeviceCreation,
    #[error("invalid render dimensions")]
    InvalidDimensions,
    #[error("failed to read back render target")]
    ReadbackFailed,
}

struct GpuRasterizer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    output_texture: wgpu::Texture,
    output_view: wgpu::TextureView,
    output_size: (u32, u32),
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    bind_group: wgpu::BindGroup,
}

impl GpuRasterizer {
    fn create(config: &RendererConfig) -> Result<Self, GpuRasterError> {
        pollster::block_on(Self::create_async(config))
    }

    async fn create_async(config: &RendererConfig) -> Result<Self, GpuRasterError> {
        if config.width == 0 || config.height == 0 {
            return Err(GpuRasterError::InvalidDimensions);
        }

        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| GpuRasterError::AdapterUnavailable)?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("gsplat-render-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|_| GpuRasterError::DeviceCreation)?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("splat-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/splat.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("splat-bgl"),
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("splat-pl"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("splat-rp"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: RENDER_TARGET_FORMAT,
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

        let (output_texture, output_view) =
            create_output_target(&device, config.width, config.height)?;
        let (instance_buffer, bind_group) =
            create_instance_resources(&device, &bind_group_layout, 1);

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            output_texture,
            output_view,
            output_size: (config.width, config.height),
            instance_buffer,
            instance_capacity: 1,
            bind_group,
        })
    }

    fn render(
        &mut self,
        config: RendererConfig,
        instances: &[GpuInstance],
    ) -> Result<(), GpuRasterError> {
        self.ensure_output_target(config.width, config.height)?;
        self.ensure_instance_capacity(instances.len().max(1));

        if !instances.is_empty() {
            self.queue
                .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("splat-render-encoder"),
            });

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("splat-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.output_view,
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
            if !instances.is_empty() {
                rpass.draw(0..6, 0..(instances.len() as u32));
            }
        }

        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn readback_rgba8(&mut self) -> Result<Vec<u8>, GpuRasterError> {
        use std::sync::mpsc;

        let (width, height) = self.output_size;
        if width == 0 || height == 0 {
            return Err(GpuRasterError::InvalidDimensions);
        }

        let bytes_per_pixel = 4_u32;
        let unpadded_bytes_per_row = width.saturating_mul(bytes_per_pixel);
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align).saturating_mul(align);
        let buffer_size = padded_bytes_per_row as u64 * height as u64;

        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("splat-readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("splat-readback-encoder"),
                });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.output_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_bytes_per_row),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));
        }

        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());

        match rx.recv() {
            Ok(Ok(())) => {}
            _ => return Err(GpuRasterError::ReadbackFailed),
        }

        let mapped = slice.get_mapped_range();
        let unpadded = unpadded_bytes_per_row as usize;
        let padded = padded_bytes_per_row as usize;
        let mut out = vec![0_u8; unpadded.saturating_mul(height as usize)];

        for row in 0..(height as usize) {
            let src_start = row * padded;
            let dst_start = row * unpadded;
            out[dst_start..dst_start + unpadded]
                .copy_from_slice(&mapped[src_start..src_start + unpadded]);
        }

        drop(mapped);
        readback.unmap();
        Ok(out)
    }

    fn ensure_output_target(&mut self, width: u32, height: u32) -> Result<(), GpuRasterError> {
        if width == 0 || height == 0 {
            return Err(GpuRasterError::InvalidDimensions);
        }

        if self.output_size == (width, height) {
            return Ok(());
        }

        let (texture, view) = create_output_target(&self.device, width, height)?;
        self.output_texture = texture;
        self.output_view = view;
        self.output_size = (width, height);
        Ok(())
    }

    fn ensure_instance_capacity(&mut self, required: usize) {
        if required <= self.instance_capacity {
            return;
        }

        let new_capacity = required.next_power_of_two();
        let (buffer, bind_group) =
            create_instance_resources(&self.device, &self.bind_group_layout, new_capacity);
        self.instance_buffer = buffer;
        self.bind_group = bind_group;
        self.instance_capacity = new_capacity;
    }
}

fn create_output_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> Result<(wgpu::Texture, wgpu::TextureView), GpuRasterError> {
    if width == 0 || height == 0 {
        return Err(GpuRasterError::InvalidDimensions);
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("splat-output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: RENDER_TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Ok((texture, view))
}

fn create_instance_resources(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    capacity: usize,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let stride = std::mem::size_of::<GpuInstance>() as u64;
    let size = stride * (capacity.max(1) as u64);

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("splat-instance-buffer"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("splat-bg"),
        layout: bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });

    (buffer, bind_group)
}

#[cfg(test)]
mod tests {
    use gsplat_core::{Camera, RenderMode, SceneBuffers, Vec3f};

    use super::Renderer;

    fn build_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 0.5), Vec3f::new(0.0, 0.0, 2.0)],
            opacity: vec![0.9, 0.8],
            scale_xyz: vec![[1.0, 1.0, 1.0], [1.2, 1.2, 1.2]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.1, 0.2, 0.3], [0.3, 0.2, 0.1]],
            sh_degree: 0,
            sh_rest: None,
        }
    }

    #[test]
    fn sorted_alpha_pipeline_renders_visible_gaussians() {
        let mut renderer = Renderer::new(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(build_scene()).unwrap();

        let stats = renderer.render_frame(&Camera::default()).unwrap();

        assert_eq!(stats.visible_count, 2);
        assert_eq!(stats.drawn_count, 2);
    }

    #[test]
    fn preprocess_rejects_missing_scene() {
        let renderer = Renderer::new(RenderMode::SortedAlpha).unwrap();
        let err = renderer.preprocess_visible(&Camera::default()).unwrap_err();
        assert_eq!(
            err.code() as i32,
            gsplat_core::ErrorCode::SceneNotLoaded as i32
        );
    }

    #[test]
    fn renderer_constructs_without_required_gpu_adapter() {
        let renderer = Renderer::new(RenderMode::SortedAlpha).unwrap();
        let _ = renderer.has_gpu_rasterizer();
    }
}
