//! WGPU renderer with a SortedAlpha reference path.

use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use gsplat_core::{Camera, ErrorCode, FrameStats, RenderMode, RendererConfig, SceneBuffers, Vec3f};
use gsplat_sort::{CpuSortBackend, GpuRadixSortBackend, SortBackend, SortError};
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
    #[error("sort backend error: {0}")]
    Sort(#[from] SortError),
}

impl RendererError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidConfig | Self::InvalidScene => ErrorCode::InvalidArgument,
            Self::SceneNotLoaded => ErrorCode::SceneNotLoaded,
            Self::Sort(_) => ErrorCode::Internal,
        }
    }
}

pub struct Renderer {
    mode: RenderMode,
    config: RendererConfig,
    gpu_sort_backend: GpuRadixSortBackend,
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
            gpu_sort_backend: GpuRadixSortBackend::default(),
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

    pub fn preprocess_visible(&self, camera: &Camera) -> Result<PreprocessOutput, RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;

        let mut indices = Vec::with_capacity(scene.len());
        let mut depth_keys = Vec::with_capacity(scene.len());

        for (idx, position) in scene.positions.iter().enumerate() {
            if is_visible(position.z, camera) {
                indices.push(idx as u32);
                depth_keys.push(depth_to_key(position.z));
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
        let instances = build_instances(scene, &preprocessed.indices);

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

fn build_instances(scene: &SceneBuffers, indices: &[u32]) -> Vec<GpuInstance> {
    let mut out = Vec::with_capacity(indices.len());

    for &idx in indices {
        let i = idx as usize;
        let Vec3f { x, y, .. } = scene.positions[i];
        let scale = scene.scale_xyz[i];
        let avg_scale = (scale[0] + scale[1] + scale[2]) / 3.0;
        let size = (avg_scale * 0.01).clamp(0.002, 0.1);
        let alpha = sigmoid(scene.opacity[i]).clamp(0.0, 1.0);
        let color = scene.color_dc[i];

        out.push(GpuInstance {
            pos_xy: [x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)],
            size,
            _pad0: 0.0,
            color_rgba: [
                (color[0] * alpha).clamp(0.0, 1.0),
                (color[1] * alpha).clamp(0.0, 1.0),
                (color[2] * alpha).clamp(0.0, 1.0),
                alpha,
            ],
        });
    }

    out
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

#[derive(Debug, Error)]
enum GpuRasterError {
    #[error("no compatible wgpu adapter")]
    AdapterUnavailable,
    #[error("wgpu device creation failed")]
    DeviceCreation,
    #[error("invalid render dimensions")]
    InvalidDimensions,
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
            sh_rest_rgb: None,
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
