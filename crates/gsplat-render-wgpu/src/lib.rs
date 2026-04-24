//! WGPU renderer with a SortedAlpha reference path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use gsplat_core::{Camera, ErrorCode, FrameStats, RenderMode, RendererConfig, SceneBuffers, Vec3f};
use gsplat_sort::{CpuSortBackend, SortError};
use rayon::prelude::*;
use thiserror::Error;
use wgpu::util::DeviceExt;

const RENDER_TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[cfg(target_os = "android")]
const fn wgpu_label(_label: &'static str) -> Option<&'static str> {
    None
}

#[cfg(not(target_os = "android"))]
const fn wgpu_label(label: &'static str) -> Option<&'static str> {
    Some(label)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessOutput {
    pub depth_keys: Vec<u32>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Error)]
pub enum RendererError {
    #[error("invalid renderer configuration")]
    InvalidConfig,
    #[error("invalid camera")]
    InvalidCamera,
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
    #[error("gpu instance preprocess error: {0}")]
    GpuInstancePreprocess(#[from] GpuInstancePreprocessError),
    #[error("surface presenter error: {0}")]
    SurfacePresenter(#[from] SurfacePresenterError),
}

impl RendererError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidConfig | Self::InvalidCamera | Self::InvalidScene => {
                ErrorCode::InvalidArgument
            }
            Self::SceneNotLoaded => ErrorCode::SceneNotLoaded,
            Self::GpuRasterizerUnavailable => ErrorCode::Unsupported,
            Self::GpuReadback => ErrorCode::Internal,
            Self::Sort(_) => ErrorCode::Internal,
            Self::GpuInstancePreprocess(_) => ErrorCode::Internal,
            Self::SurfacePresenter(err) => err.code(),
        }
    }
}

#[derive(Debug, Error)]
pub enum GpuInstancePreprocessError {
    #[error("scene/covariance length mismatch")]
    SceneCovarianceMismatch,
    #[error("instance buffer capacity must be greater than zero")]
    InvalidInstanceCapacity,
    #[error("surface size must be non-zero")]
    InvalidSurfaceSize,
    #[error("sorted index buffer capacity exceeded")]
    SortedIndexCapacityExceeded,
    #[error("instance buffer capacity exceeded")]
    InstanceBufferCapacityExceeded,
}

#[derive(Debug, Error)]
pub enum SurfacePresenterError {
    #[error("invalid surface size")]
    InvalidSurfaceSize,
    #[error("surface creation failed")]
    SurfaceCreation,
    #[error("no compatible surface adapter")]
    NoAdapter,
    #[error("surface device creation failed: {0}")]
    DeviceCreation(String),
    #[error("surface has no compatible format")]
    NoSurfaceFormat,
    #[error("surface configure failed: {0}")]
    SurfaceConfigure(String),
    #[error("surface requires a loaded scene")]
    SceneNotLoaded,
    #[error("surface acquire failed: {0}")]
    SurfaceAcquire(String),
    #[error("surface out of memory")]
    SurfaceOutOfMemory,
    #[error("gpu instance preprocess error: {0}")]
    GpuInstancePreprocess(#[from] GpuInstancePreprocessError),
}

impl SurfacePresenterError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidSurfaceSize => ErrorCode::InvalidArgument,
            Self::SurfaceCreation | Self::NoAdapter | Self::DeviceCreation(_) => {
                ErrorCode::Unsupported
            }
            Self::SceneNotLoaded => ErrorCode::SceneNotLoaded,
            Self::NoSurfaceFormat
            | Self::SurfaceConfigure(_)
            | Self::SurfaceAcquire(_)
            | Self::SurfaceOutOfMemory
            | Self::GpuInstancePreprocess(_) => ErrorCode::Internal,
        }
    }
}

pub struct Renderer {
    mode: RenderMode,
    config: RendererConfig,
    cpu_sort_backend: CpuSortBackend,
    gpu_rasterizer: Option<GpuRasterizer>,
    scene: Option<SceneBuffers>,
    world_covariances: Option<Vec<[[f32; 3]; 3]>>,
    world_covariance_terms: Option<Vec<CameraCovarianceTerms>>,
    alpha_values: Option<Vec<f32>>,
    preprocess_depth_keys: Vec<u32>,
    preprocess_indices: Vec<u32>,
    surface_instances_by_index: Vec<GpuSurfaceInstance>,
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
            cpu_sort_backend: CpuSortBackend::default(),
            gpu_rasterizer,
            scene: None,
            world_covariances: None,
            world_covariance_terms: None,
            alpha_values: None,
            preprocess_depth_keys: Vec::new(),
            preprocess_indices: Vec::new(),
            surface_instances_by_index: Vec::new(),
            last_stats: FrameStats::zero(),
        })
    }

    pub fn config(&self) -> RendererConfig {
        self.config
    }

    pub fn set_size(&mut self, width: u32, height: u32) -> Result<(), RendererError> {
        let config = RendererConfig {
            width,
            height,
            ..self.config
        };
        config
            .validate()
            .map_err(|_| RendererError::InvalidConfig)?;
        self.config = config;
        Ok(())
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
        let world_covariances = precompute_world_covariances(&scene);
        let world_covariance_terms = world_covariances
            .iter()
            .copied()
            .map(CameraCovarianceTerms::from_matrix)
            .collect();
        let alpha_values = precompute_alpha_values(&scene);
        self.scene = Some(scene);
        self.world_covariances = Some(world_covariances);
        self.world_covariance_terms = Some(world_covariance_terms);
        self.alpha_values = Some(alpha_values);
        Ok(())
    }

    pub fn scene(&self) -> Option<&SceneBuffers> {
        self.scene.as_ref()
    }

    pub fn world_covariances(&self) -> Option<&[[[f32; 3]; 3]]> {
        self.world_covariances.as_deref()
    }

    pub fn create_gpu_instance_preprocessor(
        &self,
        device: &wgpu::Device,
        instance_buffer: &wgpu::Buffer,
        instance_capacity: usize,
    ) -> Result<GpuInstancePreprocessor, RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let world_covariances = self
            .world_covariances
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        Ok(GpuInstancePreprocessor::new(
            device,
            scene,
            world_covariances,
            instance_buffer,
            instance_capacity,
        )?)
    }

    pub fn preprocess_visible(&self, camera: &Camera) -> Result<PreprocessOutput, RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let mut output = PreprocessOutput {
            depth_keys: Vec::with_capacity(scene.len()),
            indices: Vec::with_capacity(scene.len()),
        };
        preprocess_visible_into(scene, camera, &mut output.depth_keys, &mut output.indices)?;
        Ok(output)
    }

    pub fn build_sorted_instances(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<GpuInstance>, FrameStats), RendererError> {
        let mut instances = Vec::new();
        let stats = self.build_sorted_instances_into(camera, &mut instances)?;
        Ok((instances, stats))
    }

    pub fn build_sorted_instances_into(
        &mut self,
        camera: &Camera,
        instances: &mut Vec<GpuInstance>,
    ) -> Result<FrameStats, RendererError> {
        let frame_start = Instant::now();

        let preprocess_start = Instant::now();
        self.preprocess_visible_scratch(camera)?;
        let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

        let sort_start = Instant::now();
        self.sort_preprocessed_scratch()?;
        let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

        let raster_start = Instant::now();
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let world_covariances = self
            .world_covariances
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        let alpha_values = self
            .alpha_values
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        build_instances_into(
            scene,
            world_covariances,
            alpha_values,
            &self.preprocess_indices,
            camera,
            self.config,
            instances,
        );
        let drawn_count = instances.len() as u32;
        let raster_ms = raster_start.elapsed().as_secs_f32() * 1000.0;

        let stats = FrameStats {
            frame_ms: frame_start.elapsed().as_secs_f32() * 1000.0,
            preprocess_ms,
            sort_ms,
            raster_ms,
            visible_count: self.preprocess_indices.len() as u32,
            drawn_count,
        };
        self.last_stats = stats;
        Ok(stats)
    }

    pub fn build_sorted_surface_instances_into(
        &mut self,
        camera: &Camera,
        instances: &mut Vec<GpuSurfaceInstance>,
    ) -> Result<FrameStats, RendererError> {
        let frame_start = Instant::now();

        let preprocess_start = Instant::now();
        self.preprocess_visible_scratch(camera)?;
        let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

        let sort_start = Instant::now();
        self.sort_preprocessed_scratch()?;
        let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

        let raster_start = Instant::now();
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let world_covariance_terms = self
            .world_covariance_terms
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        let alpha_values = self
            .alpha_values
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        build_surface_instances_into(
            scene,
            world_covariance_terms,
            alpha_values,
            &self.preprocess_indices,
            camera,
            self.config,
            &mut self.surface_instances_by_index,
            instances,
        );
        let drawn_count = instances.len() as u32;
        let raster_ms = raster_start.elapsed().as_secs_f32() * 1000.0;

        let stats = FrameStats {
            frame_ms: frame_start.elapsed().as_secs_f32() * 1000.0,
            preprocess_ms,
            sort_ms,
            raster_ms,
            visible_count: self.preprocess_indices.len() as u32,
            drawn_count,
        };
        self.last_stats = stats;
        Ok(stats)
    }

    pub fn build_sorted_indices(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<u32>, FrameStats), RendererError> {
        let frame_start = Instant::now();

        let preprocess_start = Instant::now();
        self.preprocess_visible_scratch(camera)?;
        let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

        let sort_start = Instant::now();
        self.sort_preprocessed_scratch()?;
        let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

        let visible_count = self.preprocess_indices.len() as u32;
        let stats = FrameStats {
            frame_ms: frame_start.elapsed().as_secs_f32() * 1000.0,
            preprocess_ms,
            sort_ms,
            raster_ms: 0.0,
            visible_count,
            drawn_count: visible_count,
        };
        self.last_stats = stats;
        Ok((self.preprocess_indices.clone(), stats))
    }

    pub fn render_frame(&mut self, camera: &Camera) -> Result<FrameStats, RendererError> {
        let frame_start = Instant::now();

        let preprocess_start = Instant::now();
        self.preprocess_visible_scratch(camera)?;
        let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

        let sort_start = Instant::now();
        self.sort_preprocessed_scratch()?;
        let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

        let raster_start = Instant::now();
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let world_covariances = self
            .world_covariances
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        let alpha_values = self
            .alpha_values
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        let instances = build_instances(
            scene,
            world_covariances,
            alpha_values,
            &self.preprocess_indices,
            camera,
            self.config,
        );

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
            visible_count: self.preprocess_indices.len() as u32,
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

    fn preprocess_visible_scratch(&mut self, camera: &Camera) -> Result<(), RendererError> {
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        preprocess_visible_into(
            scene,
            camera,
            &mut self.preprocess_depth_keys,
            &mut self.preprocess_indices,
        )
    }

    fn sort_preprocessed_scratch(&mut self) -> Result<(), RendererError> {
        if self.mode == RenderMode::SortedAlpha {
            self.cpu_sort_backend
                .sort_values_by_keys(&self.preprocess_depth_keys, &mut self.preprocess_indices)?;
        }
        Ok(())
    }
}

pub struct SurfacePresenter {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    surface_config: wgpu::SurfaceConfiguration,
    max_texture_dimension_2d: u32,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instance_count: u32,
    params_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    sh_degree: u32,
    _surface_color_buffer: wgpu::Buffer,
    _sh_rest_buffer: wgpu::Buffer,
}

impl SurfacePresenter {
    /// Creates a presenter from raw handles supplied by an embedding platform.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the raw display and window handles remain valid until
    /// after the returned presenter is dropped.
    pub unsafe fn from_raw_handles(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        pollster::block_on(Self::from_raw_handles_async(
            raw_display_handle,
            raw_window_handle,
            width,
            height,
            renderer,
        ))
    }

    async fn from_raw_handles_async(
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
        renderer: &Renderer,
    ) -> Result<Self, SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }

        let instance = create_surface_instance();
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
        }
        .map_err(|_| SurfacePresenterError::SurfaceCreation)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| SurfacePresenterError::NoAdapter)?;

        let adapter_info = adapter.get_info();
        let adapter_limits = adapter.limits();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: wgpu_label("gsplat-surface-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|err| {
                SurfacePresenterError::DeviceCreation(format!(
                    "{err}; adapter={adapter_info:?}; limits={adapter_limits:?}"
                ))
            })?;

        let caps = surface.get_capabilities(&adapter);
        let Some(format) = caps.formats.first().copied() else {
            return Err(SurfacePresenterError::NoSurfaceFormat);
        };
        let present_mode = select_present_mode(&caps);
        let alpha_mode = caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Opaque);

        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d.max(1);
        let (surface_width, surface_height) =
            fit_surface_size(width, height, max_texture_dimension_2d);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: surface_width,
            height: surface_height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(&device, &surface_config);
        if let Some(err) = error_scope.pop().await {
            return Err(SurfacePresenterError::SurfaceConfigure(err.to_string()));
        }

        let render_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-surface-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/splat_surface.wgsl").into()),
        });

        let render_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: wgpu_label("gsplat-surface-bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: wgpu_label("gsplat-surface-pipeline-layout"),
                bind_group_layouts: &[&render_bind_group_layout],
                immediate_size: 0,
            });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: wgpu_label("gsplat-surface-pipeline"),
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
        let scene = renderer
            .scene()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        let scene_len = scene.len().max(1);
        let instance_capacity = surface_instance_capacity(scene_len);
        let (instance_buffer, params_buffer, surface_color_buffer, sh_rest_buffer, bind_group) =
            create_surface_instance_resources(
                &device,
                &render_bind_group_layout,
                instance_capacity,
                scene,
            );

        Ok(Self {
            surface,
            device,
            queue,
            pipeline,
            surface_config,
            max_texture_dimension_2d,
            instance_buffer,
            instance_capacity,
            instance_count: 0,
            params_buffer,
            bind_group,
            sh_degree: scene.sh_degree as u32,
            _surface_color_buffer: surface_color_buffer,
            _sh_rest_buffer: sh_rest_buffer,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let (width, height) = fit_surface_size(width, height, self.max_texture_dimension_2d);
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub const fn surface_size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }

    pub fn render_instances(
        &mut self,
        instances: &[GpuSurfaceInstance],
        camera: &Camera,
    ) -> Result<(), SurfacePresenterError> {
        if instances.len() > self.instance_capacity {
            return Err(SurfacePresenterError::GpuInstancePreprocess(
                GpuInstancePreprocessError::InstanceBufferCapacityExceeded,
            ));
        }
        if !instances.is_empty() {
            self.queue
                .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));
        }
        let params = GpuSurfaceRenderParams {
            camera_pos: [
                camera.pose.position.x,
                camera.pose.position.y,
                camera.pose.position.z,
                0.0,
            ],
            sh_degree: self.sh_degree,
            len: instances.len() as u32,
            _pad0: 0,
            _pad2: 0,
        };
        self.queue
            .write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        self.instance_count = instances.len() as u32;
        self.render_current()
    }

    pub fn render_current(&mut self) -> Result<(), SurfacePresenterError> {
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                match self.surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(wgpu::SurfaceError::Timeout) => return Ok(()),
                    Err(err) => return Err(surface_error_to_presenter(err)),
                }
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(()),
            Err(err) => return Err(surface_error_to_presenter(err)),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: wgpu_label("gsplat-surface-encoder"),
            });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: wgpu_label("gsplat-surface-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
            if self.instance_count > 0 {
                rpass.draw(0..6, 0..self.instance_count);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    pub const fn instance_count(&self) -> u32 {
        self.instance_count
    }
}

fn create_surface_instance_resources(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    capacity: usize,
    scene: &SceneBuffers,
) -> (
    wgpu::Buffer,
    wgpu::Buffer,
    wgpu::Buffer,
    wgpu::Buffer,
    wgpu::BindGroup,
) {
    let stride = std::mem::size_of::<GpuSurfaceInstance>() as u64;
    let size = stride * (capacity.max(1) as u64);
    let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: wgpu_label("gsplat-surface-instance-buffer"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: wgpu_label("gsplat-surface-params-buffer"),
        contents: bytemuck::bytes_of(&GpuSurfaceRenderParams::zeroed()),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let surface_color_elems = make_surface_color_elems(scene);
    let surface_color_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: wgpu_label("gsplat-surface-color-buffer"),
        contents: bytemuck::cast_slice(&surface_color_elems),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let sh_rest_fallback = [0.0_f32];
    let sh_rest = scene.sh_rest.as_deref().unwrap_or(&sh_rest_fallback);
    let sh_rest_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: wgpu_label("gsplat-surface-sh-rest-buffer"),
        contents: bytemuck::cast_slice(sh_rest),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("gsplat-surface-bind-group"),
        layout: bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: instance_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: surface_color_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: sh_rest_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params_buffer.as_entire_binding(),
            },
        ],
    });
    (
        instance_buffer,
        params_buffer,
        surface_color_buffer,
        sh_rest_buffer,
        bind_group,
    )
}

fn make_surface_color_elems(scene: &SceneBuffers) -> Vec<GpuSurfaceColorElem> {
    scene
        .positions
        .iter()
        .zip(scene.color_dc.iter())
        .map(|(position, color_dc)| GpuSurfaceColorElem {
            position: [position.x, position.y, position.z, 0.0],
            color_dc: [color_dc[0], color_dc[1], color_dc[2], 0.0],
        })
        .collect()
}

fn make_gpu_scene_elems(
    scene: &SceneBuffers,
    world_covariances: &[[[f32; 3]; 3]],
) -> Vec<GpuSceneElem> {
    if scene.positions.is_empty() {
        return vec![GpuSceneElem::zeroed()];
    }

    (0..scene.positions.len())
        .map(|i| {
            let pos = scene.positions[i];
            let cov = world_covariances[i];
            let color_dc = scene.color_dc.get(i).copied().unwrap_or([0.0, 0.0, 0.0]);
            let opacity = *scene.opacity.get(i).unwrap_or(&0.0);
            GpuSceneElem {
                position: [pos.x, pos.y, pos.z, 0.0],
                cov_row0: [cov[0][0], cov[0][1], cov[0][2], 0.0],
                cov_row1: [cov[1][0], cov[1][1], cov[1][2], 0.0],
                cov_row2: [cov[2][0], cov[2][1], cov[2][2], 0.0],
                color_dc: [color_dc[0], color_dc[1], color_dc[2], 0.0],
                opacity_and_pad: [opacity, 0.0, 0.0, 0.0],
            }
        })
        .collect()
}

fn select_present_mode(caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        return wgpu::PresentMode::Mailbox;
    }
    if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
        return wgpu::PresentMode::Fifo;
    }

    caps.present_modes
        .first()
        .copied()
        .unwrap_or(wgpu::PresentMode::Fifo)
}

fn surface_error_to_presenter(err: wgpu::SurfaceError) -> SurfacePresenterError {
    match err {
        wgpu::SurfaceError::OutOfMemory => SurfacePresenterError::SurfaceOutOfMemory,
        other => SurfacePresenterError::SurfaceAcquire(format!("{other:?}")),
    }
}

fn surface_instance_capacity(scene_len: usize) -> usize {
    scene_len.max(1)
}

fn fit_surface_size(width: u32, height: u32, max_dimension: u32) -> (u32, u32) {
    let max_input_dimension = width.max(height).max(1);
    if max_input_dimension <= max_dimension {
        return (width, height);
    }

    let scale = max_dimension as f32 / max_input_dimension as f32;
    let scaled_width = ((width as f32) * scale).round() as u32;
    let scaled_height = ((height as f32) * scale).round() as u32;
    (scaled_width.max(1), scaled_height.max(1))
}

#[cfg(target_os = "android")]
fn create_surface_instance() -> wgpu::Instance {
    wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        flags: wgpu::InstanceFlags::empty(),
        ..Default::default()
    })
}

#[cfg(not(target_os = "android"))]
fn create_surface_instance() -> wgpu::Instance {
    wgpu::Instance::default()
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
pub struct GpuInstance {
    // xy = center in NDC, zw = major axis in NDC
    pub center_and_axis_u: [f32; 4],
    // xy = minor axis in NDC, zw = reserved
    pub axis_v_and_pad: [f32; 4],
    // Premultiplied RGB + alpha
    pub color_rgba: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GpuSurfaceInstance {
    // xy = center in NDC, zw = major axis in NDC
    pub center_and_axis_u: [f32; 4],
    // xy = minor axis in NDC, z = source splat index, w = alpha
    pub axis_v_index_alpha: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuSurfaceColorElem {
    position: [f32; 4],
    color_dc: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuSurfaceRenderParams {
    camera_pos: [f32; 4],
    sh_degree: u32,
    len: u32,
    _pad0: u32,
    _pad2: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuSceneElem {
    position: [f32; 4],
    cov_row0: [f32; 4],
    cov_row1: [f32; 4],
    cov_row2: [f32; 4],
    color_dc: [f32; 4],
    opacity_and_pad: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuPreprocessParams {
    camera_pos: [f32; 4],
    camera_inv_q: [f32; 4],
    view_rot_row0: [f32; 4],
    view_rot_row1: [f32; 4],
    view_rot_row2: [f32; 4],
    vertical_fov_radians: f32,
    near_plane: f32,
    far_plane: f32,
    aspect: f32,
    width: u32,
    height: u32,
    len: u32,
    sh_degree: u32,
}

pub struct GpuInstancePreprocessor {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    params_buffer: wgpu::Buffer,
    sorted_indices_buffer: wgpu::Buffer,
    sorted_capacity: usize,
    instance_capacity: usize,
    sh_degree: u32,
    _scene_buffer: wgpu::Buffer,
    _sh_rest_buffer: wgpu::Buffer,
}

impl GpuInstancePreprocessor {
    pub fn new(
        device: &wgpu::Device,
        scene: &SceneBuffers,
        world_covariances: &[[[f32; 3]; 3]],
        instance_buffer: &wgpu::Buffer,
        instance_capacity: usize,
    ) -> Result<Self, GpuInstancePreprocessError> {
        if scene.len() != world_covariances.len() {
            return Err(GpuInstancePreprocessError::SceneCovarianceMismatch);
        }
        if instance_capacity == 0 {
            return Err(GpuInstancePreprocessError::InvalidInstanceCapacity);
        }

        let preprocess_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-render-preprocess-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/preprocess_instances.wgsl").into(),
            ),
        });
        let preprocess_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: wgpu_label("gsplat-render-preprocess-bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let preprocess_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: wgpu_label("gsplat-render-preprocess-pl"),
                bind_group_layouts: &[&preprocess_bind_group_layout],
                immediate_size: 0,
            });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: wgpu_label("gsplat-render-preprocess-cp"),
            layout: Some(&preprocess_pipeline_layout),
            module: &preprocess_shader,
            entry_point: Some("main"),
            cache: None,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        });

        let scene_len = scene.len().max(1);
        let scene_data = make_gpu_scene_elems(scene, world_covariances);
        let scene_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-render-preprocess-scene"),
            contents: bytemuck::cast_slice(&scene_data),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let sorted_indices_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-render-preprocess-sorted-indices"),
            size: (scene_len as u64) * std::mem::size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let empty_rest = [0.0_f32];
        let sh_rest = scene.sh_rest.as_deref().unwrap_or(&empty_rest);
        let sh_rest_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-render-preprocess-sh-rest"),
            contents: bytemuck::cast_slice(sh_rest),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let params = GpuPreprocessParams {
            camera_pos: [0.0, 0.0, 0.0, 0.0],
            camera_inv_q: [0.0, 0.0, 0.0, 1.0],
            view_rot_row0: [1.0, 0.0, 0.0, 0.0],
            view_rot_row1: [0.0, 1.0, 0.0, 0.0],
            view_rot_row2: [0.0, 0.0, 1.0, 0.0],
            vertical_fov_radians: 60.0_f32.to_radians(),
            near_plane: 0.01,
            far_plane: 1000.0,
            aspect: 1.0,
            width: 1,
            height: 1,
            len: 0,
            sh_degree: scene.sh_degree as u32,
        };
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-render-preprocess-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-render-preprocess-bg"),
            layout: &preprocess_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: sorted_indices_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: scene_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: sh_rest_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: instance_buffer.as_entire_binding(),
                },
            ],
        });

        Ok(Self {
            pipeline,
            bind_group,
            params_buffer,
            sorted_indices_buffer,
            sorted_capacity: scene_len,
            instance_capacity,
            sh_degree: scene.sh_degree as u32,
            _scene_buffer: scene_buffer,
            _sh_rest_buffer: sh_rest_buffer,
        })
    }

    pub const fn sorted_capacity(&self) -> usize {
        self.sorted_capacity
    }

    pub const fn instance_capacity(&self) -> usize {
        self.instance_capacity
    }

    pub fn prepare(
        &self,
        queue: &wgpu::Queue,
        sorted_indices: &[u32],
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<u32, GpuInstancePreprocessError> {
        if width == 0 || height == 0 {
            return Err(GpuInstancePreprocessError::InvalidSurfaceSize);
        }
        if sorted_indices.len() > self.sorted_capacity {
            return Err(GpuInstancePreprocessError::SortedIndexCapacityExceeded);
        }
        if sorted_indices.len() > self.instance_capacity {
            return Err(GpuInstancePreprocessError::InstanceBufferCapacityExceeded);
        }
        if !sorted_indices.is_empty() {
            queue.write_buffer(
                &self.sorted_indices_buffer,
                0,
                bytemuck::cast_slice(sorted_indices),
            );
        }

        let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);
        let view_rot = quat_to_mat3(camera_inv_q);
        let params = GpuPreprocessParams {
            camera_pos: [
                camera.pose.position.x,
                camera.pose.position.y,
                camera.pose.position.z,
                0.0,
            ],
            camera_inv_q,
            view_rot_row0: [view_rot[0][0], view_rot[0][1], view_rot[0][2], 0.0],
            view_rot_row1: [view_rot[1][0], view_rot[1][1], view_rot[1][2], 0.0],
            view_rot_row2: [view_rot[2][0], view_rot[2][1], view_rot[2][2], 0.0],
            vertical_fov_radians: camera.intrinsics.vertical_fov_radians,
            near_plane: camera.intrinsics.near_plane,
            far_plane: camera.intrinsics.far_plane,
            aspect: (width as f32 / height as f32).max(1e-6),
            width,
            height,
            len: sorted_indices.len() as u32,
            sh_degree: self.sh_degree,
        };
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(sorted_indices.len() as u32)
    }

    pub fn encode_dispatch(&self, encoder: &mut wgpu::CommandEncoder, instance_count: u32) {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: wgpu_label("gsplat-render-preprocess-pass"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(&self.pipeline);
        cpass.set_bind_group(0, &self.bind_group, &[]);
        cpass.dispatch_workgroups(instance_count.div_ceil(64).max(1), 1, 1);
    }
}

fn build_instances(
    scene: &SceneBuffers,
    world_covariances: &[[[f32; 3]; 3]],
    alpha_values: &[f32],
    indices: &[u32],
    camera: &Camera,
    config: RendererConfig,
) -> Vec<GpuInstance> {
    let mut out = Vec::new();
    build_instances_into(
        scene,
        world_covariances,
        alpha_values,
        indices,
        camera,
        config,
        &mut out,
    );
    out
}

fn build_instances_into(
    scene: &SceneBuffers,
    world_covariances: &[[[f32; 3]; 3]],
    alpha_values: &[f32],
    indices: &[u32],
    camera: &Camera,
    config: RendererConfig,
    out: &mut Vec<GpuInstance>,
) {
    if world_covariances.len() != scene.len() || alpha_values.len() != scene.len() {
        out.clear();
        return;
    }

    let Some(params) = InstanceBuildParams::new(camera, config) else {
        out.clear();
        return;
    };

    if out.len() < indices.len() {
        out.resize(indices.len(), GpuInstance::zeroed());
    } else {
        out.truncate(indices.len());
    }

    let sh_layout = ShColorLayout::new(scene);
    let had_invalid = AtomicBool::new(false);
    out.par_iter_mut()
        .zip(indices.par_iter())
        .for_each(|(slot, &idx)| {
            let i = idx as usize;
            let instance = if i < scene.len() {
                // SAFETY: the explicit bounds check above covers all scene-parallel arrays because
                // the caller validated equal lengths before entering this loop.
                unsafe {
                    build_instance_unchecked(
                        scene,
                        world_covariances,
                        alpha_values,
                        i,
                        camera,
                        &params,
                        sh_layout,
                    )
                }
            } else {
                None
            };
            if let Some(instance) = instance {
                *slot = instance;
            } else {
                *slot = invalid_gpu_instance();
                had_invalid.store(true, Ordering::Relaxed);
            }
        });

    if had_invalid.load(Ordering::Relaxed) {
        out.retain(|instance| instance.color_rgba[3] >= 0.0);
    }
}

unsafe fn build_instance_unchecked(
    scene: &SceneBuffers,
    world_covariances: &[[[f32; 3]; 3]],
    alpha_values: &[f32],
    i: usize,
    camera: &Camera,
    params: &InstanceBuildParams,
    sh_layout: ShColorLayout<'_>,
) -> Option<GpuInstance> {
    let pos_world = unsafe { *scene.positions.get_unchecked(i) };
    let p_cam = world_to_camera_with_view_rot(pos_world, camera.pose.position, params.view_rot);
    // Preprocess already culled by z range, but keep this safe for runtime camera changes.
    if !is_visible(p_cam.z, camera) {
        return None;
    }

    // Project center to NDC for instance placement.
    let inv_z = 1.0 / p_cam.z.max(1e-6);
    let x_ndc = (p_cam.x * params.f) * inv_z / params.aspect;
    let y_ndc = (p_cam.y * params.f) * inv_z;

    let cov2_ndc = project_world_covariance_to_ndc(
        p_cam,
        unsafe { *world_covariances.get_unchecked(i) },
        params,
    )?;
    let (axis_u, axis_v) = ellipse_axes_from_covariance(cov2_ndc)?;
    let extent_x = axis_u[0].abs() + axis_v[0].abs();
    let extent_y = axis_u[1].abs() + axis_v[1].abs();
    if x_ndc + extent_x < -1.0
        || x_ndc - extent_x > 1.0
        || y_ndc + extent_y < -1.0
        || y_ndc - extent_y > 1.0
    {
        return None;
    }

    let alpha = unsafe { *alpha_values.get_unchecked(i) };
    let dir_world = normalize3(Vec3f::new(
        pos_world.x - camera.pose.position.x,
        pos_world.y - camera.pose.position.y,
        pos_world.z - camera.pose.position.z,
    ));
    let rgb = unsafe { sh_color_unchecked(scene, i, dir_world, sh_layout) };
    let rgb = [
        rgb[0].clamp(0.0, 1.0),
        rgb[1].clamp(0.0, 1.0),
        rgb[2].clamp(0.0, 1.0),
    ];

    Some(GpuInstance {
        center_and_axis_u: [x_ndc, y_ndc, axis_u[0], axis_u[1]],
        axis_v_and_pad: [axis_v[0], axis_v[1], 0.0, 0.0],
        color_rgba: [
            (rgb[0] * alpha).clamp(0.0, 1.0),
            (rgb[1] * alpha).clamp(0.0, 1.0),
            (rgb[2] * alpha).clamp(0.0, 1.0),
            alpha,
        ],
    })
}

fn invalid_gpu_instance() -> GpuInstance {
    GpuInstance {
        color_rgba: [0.0, 0.0, 0.0, -1.0],
        ..GpuInstance::zeroed()
    }
}

fn build_surface_instances_into(
    scene: &SceneBuffers,
    world_covariance_terms: &[CameraCovarianceTerms],
    alpha_values: &[f32],
    indices: &[u32],
    camera: &Camera,
    config: RendererConfig,
    by_index: &mut Vec<GpuSurfaceInstance>,
    out: &mut Vec<GpuSurfaceInstance>,
) {
    if world_covariance_terms.len() != scene.len() || alpha_values.len() != scene.len() {
        out.clear();
        return;
    }

    let Some(params) = InstanceBuildParams::new(camera, config) else {
        out.clear();
        return;
    };

    if should_use_scene_order_surface_build(scene.len(), indices.len()) {
        build_surface_instances_scene_order_into(
            scene,
            world_covariance_terms,
            alpha_values,
            indices,
            camera,
            &params,
            by_index,
            out,
        );
    } else {
        build_surface_instances_index_order_into(
            scene,
            world_covariance_terms,
            alpha_values,
            indices,
            camera,
            &params,
            out,
        );
    }
}

fn should_use_scene_order_surface_build(scene_len: usize, visible_len: usize) -> bool {
    scene_len > 0 && visible_len.saturating_mul(100) >= scene_len.saturating_mul(85)
}

fn build_surface_instances_scene_order_into(
    scene: &SceneBuffers,
    world_covariance_terms: &[CameraCovarianceTerms],
    alpha_values: &[f32],
    indices: &[u32],
    camera: &Camera,
    params: &InstanceBuildParams,
    by_index: &mut Vec<GpuSurfaceInstance>,
    out: &mut Vec<GpuSurfaceInstance>,
) {
    if by_index.len() < scene.len() {
        by_index.resize(scene.len(), GpuSurfaceInstance::zeroed());
    } else {
        by_index.truncate(scene.len());
    }

    by_index.par_iter_mut().enumerate().for_each(|(i, slot)| {
        // SAFETY: this scene-order path iterates exactly over scene indices and the caller already
        // validated equal-length side arrays.
        let instance = unsafe {
            build_surface_instance_unchecked(
                scene,
                world_covariance_terms,
                alpha_values,
                i,
                camera,
                params,
            )
        };
        *slot = instance.unwrap_or_else(invalid_gpu_surface_instance);
    });

    if out.len() < indices.len() {
        out.resize(indices.len(), GpuSurfaceInstance::zeroed());
    } else {
        out.truncate(indices.len());
    }

    let mut write_index = 0_usize;
    for &idx in indices {
        let i = idx as usize;
        if i >= by_index.len() {
            continue;
        }
        let instance = by_index[i];
        if instance.axis_v_index_alpha[3] >= 0.0 {
            out[write_index] = instance;
            write_index += 1;
        }
    }
    out.truncate(write_index);
}

fn build_surface_instances_index_order_into(
    scene: &SceneBuffers,
    world_covariance_terms: &[CameraCovarianceTerms],
    alpha_values: &[f32],
    indices: &[u32],
    camera: &Camera,
    params: &InstanceBuildParams,
    out: &mut Vec<GpuSurfaceInstance>,
) {
    if out.len() < indices.len() {
        out.resize(indices.len(), GpuSurfaceInstance::zeroed());
    } else {
        out.truncate(indices.len());
    }

    let had_invalid = AtomicBool::new(false);
    out.par_iter_mut()
        .zip(indices.par_iter())
        .for_each(|(slot, &idx)| {
            let i = idx as usize;
            let instance = if i < scene.len() {
                // SAFETY: the explicit bounds check above covers all scene-parallel arrays because
                // the caller validated equal lengths before entering this loop.
                unsafe {
                    build_surface_instance_unchecked(
                        scene,
                        world_covariance_terms,
                        alpha_values,
                        i,
                        camera,
                        &params,
                    )
                }
            } else {
                None
            };
            if let Some(instance) = instance {
                *slot = instance;
            } else {
                *slot = invalid_gpu_surface_instance();
                had_invalid.store(true, Ordering::Relaxed);
            }
        });

    if had_invalid.load(Ordering::Relaxed) {
        out.retain(|instance| instance.axis_v_index_alpha[3] >= 0.0);
    }
}

unsafe fn build_surface_instance_unchecked(
    scene: &SceneBuffers,
    world_covariance_terms: &[CameraCovarianceTerms],
    alpha_values: &[f32],
    i: usize,
    camera: &Camera,
    params: &InstanceBuildParams,
) -> Option<GpuSurfaceInstance> {
    let pos_world = unsafe { *scene.positions.get_unchecked(i) };
    let p_cam = world_to_camera_with_view_rot(pos_world, camera.pose.position, params.view_rot);
    if !is_visible(p_cam.z, camera) {
        return None;
    }

    let inv_z = 1.0 / p_cam.z.max(1e-6);
    let x_ndc = (p_cam.x * params.f) * inv_z / params.aspect;
    let y_ndc = (p_cam.y * params.f) * inv_z;

    let cov2_ndc = project_world_covariance_terms_to_ndc(
        p_cam,
        unsafe { *world_covariance_terms.get_unchecked(i) },
        params,
    )?;
    let (axis_u, axis_v) = ellipse_axes_from_covariance(cov2_ndc)?;
    let extent_x = axis_u[0].abs() + axis_v[0].abs();
    let extent_y = axis_u[1].abs() + axis_v[1].abs();
    if x_ndc + extent_x < -1.0
        || x_ndc - extent_x > 1.0
        || y_ndc + extent_y < -1.0
        || y_ndc - extent_y > 1.0
    {
        return None;
    }

    Some(GpuSurfaceInstance {
        center_and_axis_u: [x_ndc, y_ndc, axis_u[0], axis_u[1]],
        axis_v_index_alpha: [axis_v[0], axis_v[1], i as f32, unsafe {
            *alpha_values.get_unchecked(i)
        }],
    })
}

fn invalid_gpu_surface_instance() -> GpuSurfaceInstance {
    GpuSurfaceInstance {
        axis_v_index_alpha: [0.0, 0.0, 0.0, -1.0],
        ..GpuSurfaceInstance::zeroed()
    }
}

#[derive(Clone, Copy)]
struct InstanceBuildParams {
    aspect: f32,
    f: f32,
    fx: f32,
    fy: f32,
    lim_x: f32,
    lim_y: f32,
    blur_cov_x: f32,
    blur_cov_y: f32,
    view_rot: [[f32; 3]; 3],
}

impl InstanceBuildParams {
    fn new(camera: &Camera, config: RendererConfig) -> Option<Self> {
        if config.width == 0 || config.height == 0 {
            return None;
        }
        let tan_half_fovy = (camera.intrinsics.vertical_fov_radians * 0.5).tan();
        if tan_half_fovy <= 0.0 || !tan_half_fovy.is_finite() {
            return None;
        }

        let aspect = config.width as f32 / config.height as f32;
        let f = 1.0 / tan_half_fovy;
        let fx = f / aspect;
        let fy = f;
        let tan_half_fovx = tan_half_fovy * aspect;
        let blur_pixels = 0.3_f32;
        let px_ndc_x = 2.0 / config.width as f32;
        let px_ndc_y = 2.0 / config.height as f32;
        let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);

        Some(Self {
            aspect,
            f,
            fx,
            fy,
            lim_x: 1.3 * tan_half_fovx,
            lim_y: 1.3 * tan_half_fovy,
            blur_cov_x: (blur_pixels * px_ndc_x).powi(2),
            blur_cov_y: (blur_pixels * px_ndc_y).powi(2),
            view_rot: quat_to_mat3(camera_inv_q),
        })
    }
}

fn precompute_alpha_values(scene: &SceneBuffers) -> Vec<f32> {
    scene
        .opacity
        .iter()
        .map(|&opacity| sigmoid(opacity).clamp(0.0, 1.0))
        .collect()
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn preprocess_visible_into(
    scene: &SceneBuffers,
    camera: &Camera,
    depth_keys: &mut Vec<u32>,
    indices: &mut Vec<u32>,
) -> Result<(), RendererError> {
    camera
        .validate()
        .map_err(|_| RendererError::InvalidCamera)?;

    depth_keys.clear();
    indices.clear();
    if depth_keys.capacity() < scene.len() {
        depth_keys.reserve(scene.len() - depth_keys.capacity());
    }
    if indices.capacity() < scene.len() {
        indices.reserve(scene.len() - indices.capacity());
    }

    let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);
    let view_rot = quat_to_mat3(camera_inv_q);
    let depth_row = view_rot[2];
    let camera_position = camera.pose.position;

    for (idx, position) in scene.positions.iter().enumerate() {
        let depth_z = world_to_camera_depth_with_view_row(*position, camera_position, depth_row);
        if is_visible(depth_z, camera) {
            indices.push(idx as u32);
            depth_keys.push(depth_to_key(depth_z));
        }
    }

    Ok(())
}

fn world_to_camera_depth_with_view_row(
    pos_world: Vec3f,
    camera_position: Vec3f,
    depth_row: [f32; 3],
) -> f32 {
    depth_row[0] * (pos_world.x - camera_position.x)
        + depth_row[1] * (pos_world.y - camera_position.y)
        + depth_row[2] * (pos_world.z - camera_position.z)
}

fn world_to_camera_with_view_rot(
    pos_world: Vec3f,
    camera_position: Vec3f,
    view_rot: [[f32; 3]; 3],
) -> Vec3f {
    let p = Vec3f::new(
        pos_world.x - camera_position.x,
        pos_world.y - camera_position.y,
        pos_world.z - camera_position.z,
    );

    Vec3f::new(
        view_rot[0][0] * p.x + view_rot[0][1] * p.y + view_rot[0][2] * p.z,
        view_rot[1][0] * p.x + view_rot[1][1] * p.y + view_rot[1][2] * p.z,
        view_rot[2][0] * p.x + view_rot[2][1] * p.y + view_rot[2][2] * p.z,
    )
}

fn quat_inverse(q: [f32; 4]) -> [f32; 4] {
    let q = quat_normalize(q);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [-x, -y, -z, w]
}

fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let norm2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if norm2 <= 0.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let inv = 1.0 / norm2.sqrt();
    [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
}

fn quat_to_mat3(q: [f32; 4]) -> [[f32; 3]; 3] {
    let q = quat_normalize(q);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;

    [
        [1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy)],
        [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx)],
        [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy)],
    ]
}

fn mat3_mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    out
}

fn mat3_transpose(m: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
}

#[cfg(test)]
fn project_covariance_to_ndc(
    p_cam: Vec3f,
    cov_cam: [[f32; 3]; 3],
    camera: &Camera,
    config: RendererConfig,
) -> Option<[[f32; 2]; 2]> {
    let params = InstanceBuildParams::new(camera, config)?;
    project_camera_covariance_to_ndc(p_cam, CameraCovarianceTerms::from_matrix(cov_cam), &params)
}

fn project_world_covariance_to_ndc(
    p_cam: Vec3f,
    world_cov: [[f32; 3]; 3],
    params: &InstanceBuildParams,
) -> Option<[[f32; 2]; 2]> {
    let world_cov = CameraCovarianceTerms::from_matrix(world_cov);
    project_world_covariance_terms_to_ndc(p_cam, world_cov, params)
}

fn project_world_covariance_terms_to_ndc(
    p_cam: Vec3f,
    world_cov: CameraCovarianceTerms,
    params: &InstanceBuildParams,
) -> Option<[[f32; 2]; 2]> {
    let cov_cam = transform_covariance_terms_to_camera(world_cov, params.view_rot);
    project_camera_covariance_to_ndc(p_cam, cov_cam, params)
}

fn project_camera_covariance_to_ndc(
    p_cam: Vec3f,
    cov_cam: CameraCovarianceTerms,
    params: &InstanceBuildParams,
) -> Option<[[f32; 2]; 2]> {
    let z = p_cam.z;
    if z <= 1e-6 || !z.is_finite() {
        return None;
    }

    // Match common 3DGS covariance projection behavior: clamp view-space x/z and y/z
    // before Jacobian evaluation to avoid extreme derivatives at frustum edges.
    let x_clamped = (p_cam.x / z).clamp(-params.lim_x, params.lim_x) * z;
    let y_clamped = (p_cam.y / z).clamp(-params.lim_y, params.lim_y) * z;

    let inv_z = 1.0 / z;
    let inv_z2 = inv_z * inv_z;
    let j00 = params.fx * inv_z;
    let j02 = -params.fx * x_clamped * inv_z2;
    let j11 = params.fy * inv_z;
    let j12 = -params.fy * y_clamped * inv_z2;

    let cov01 = j00 * j11 * cov_cam.xy
        + j00 * j12 * cov_cam.xz
        + j02 * j11 * cov_cam.yz
        + j02 * j12 * cov_cam.zz;
    let mut cov2 = [
        [
            j00 * j00 * cov_cam.xx + 2.0 * j00 * j02 * cov_cam.xz + j02 * j02 * cov_cam.zz,
            cov01,
        ],
        [
            cov01,
            j11 * j11 * cov_cam.yy + 2.0 * j11 * j12 * cov_cam.yz + j12 * j12 * cov_cam.zz,
        ],
    ];

    // Low-pass filter in NDC space to keep splats from collapsing to subpixel noise.
    cov2[0][0] += params.blur_cov_x;
    cov2[1][1] += params.blur_cov_y;

    if !cov2[0][0].is_finite()
        || !cov2[0][1].is_finite()
        || !cov2[1][0].is_finite()
        || !cov2[1][1].is_finite()
    {
        return None;
    }

    Some(cov2)
}

#[derive(Clone, Copy)]
struct CameraCovarianceTerms {
    xx: f32,
    xy: f32,
    xz: f32,
    yy: f32,
    yz: f32,
    zz: f32,
}

impl CameraCovarianceTerms {
    fn from_matrix(cov: [[f32; 3]; 3]) -> Self {
        Self {
            xx: cov[0][0],
            xy: cov[0][1],
            xz: cov[0][2],
            yy: cov[1][1],
            yz: cov[1][2],
            zz: cov[2][2],
        }
    }
}

fn transform_covariance_terms_to_camera(
    cov: CameraCovarianceTerms,
    view_rot: [[f32; 3]; 3],
) -> CameraCovarianceTerms {
    let c00 = cov.xx;
    let c01 = cov.xy;
    let c02 = cov.xz;
    let c11 = cov.yy;
    let c12 = cov.yz;
    let c22 = cov.zz;
    let r0 = view_rot[0];
    let r1 = view_rot[1];
    let r2 = view_rot[2];

    CameraCovarianceTerms {
        xx: covariance_quadratic(c00, c01, c02, c11, c12, c22, r0),
        xy: covariance_bilinear(c00, c01, c02, c11, c12, c22, r0, r1),
        xz: covariance_bilinear(c00, c01, c02, c11, c12, c22, r0, r2),
        yy: covariance_quadratic(c00, c01, c02, c11, c12, c22, r1),
        yz: covariance_bilinear(c00, c01, c02, c11, c12, c22, r1, r2),
        zz: covariance_quadratic(c00, c01, c02, c11, c12, c22, r2),
    }
}

fn covariance_quadratic(
    c00: f32,
    c01: f32,
    c02: f32,
    c11: f32,
    c12: f32,
    c22: f32,
    r: [f32; 3],
) -> f32 {
    r[0] * r[0] * c00
        + 2.0 * r[0] * r[1] * c01
        + 2.0 * r[0] * r[2] * c02
        + r[1] * r[1] * c11
        + 2.0 * r[1] * r[2] * c12
        + r[2] * r[2] * c22
}

fn covariance_bilinear(
    c00: f32,
    c01: f32,
    c02: f32,
    c11: f32,
    c12: f32,
    c22: f32,
    a: [f32; 3],
    b: [f32; 3],
) -> f32 {
    let bx = c00 * b[0] + c01 * b[1] + c02 * b[2];
    let by = c01 * b[0] + c11 * b[1] + c12 * b[2];
    let bz = c02 * b[0] + c12 * b[1] + c22 * b[2];
    a[0] * bx + a[1] * by + a[2] * bz
}

fn ellipse_axes_from_covariance(cov2: [[f32; 2]; 2]) -> Option<([f32; 2], [f32; 2])> {
    let a = cov2[0][0];
    let b = cov2[0][1];
    let c = cov2[1][1];
    if !a.is_finite() || !b.is_finite() || !c.is_finite() {
        return None;
    }

    let apco2 = (a + c) * 0.5;
    let amco2 = (a - c) * 0.5;
    let term = (amco2 * amco2 + b * b).sqrt();
    let major = (apco2 + term).max(1e-10);
    let minor = (apco2 - term).max(1e-10);

    let axis_u_dir = if b.abs() > 1e-8 {
        normalize2([b, major - a])
    } else if a >= c {
        [1.0, 0.0]
    } else {
        [0.0, 1.0]
    };
    let axis_v_dir = [-axis_u_dir[1], axis_u_dir[0]];

    // 3-sigma support region for rasterization.
    let radius_k = 3.0_f32;
    let mut major_radius = (major.sqrt() * radius_k).clamp(1e-4, 2.0);
    let minor_radius = (minor.sqrt() * radius_k).clamp(1e-4, 2.0);

    // Guard against extreme anisotropy from unstable covariance outliers. Those splats tend to
    // show up as needle-like streaks while contributing little stable structure.
    const MAX_ANISOTROPY: f32 = 64.0;
    if major_radius > minor_radius * MAX_ANISOTROPY {
        major_radius = minor_radius * MAX_ANISOTROPY;
    }
    let axis_u = [axis_u_dir[0] * major_radius, axis_u_dir[1] * major_radius];
    let axis_v = [axis_v_dir[0] * minor_radius, axis_v_dir[1] * minor_radius];
    if !axis_u[0].is_finite()
        || !axis_u[1].is_finite()
        || !axis_v[0].is_finite()
        || !axis_v[1].is_finite()
    {
        return None;
    }

    Some((axis_u, axis_v))
}

fn normalize3(v: Vec3f) -> [f32; 3] {
    let len2 = v.x * v.x + v.y * v.y + v.z * v.z;
    if len2 <= 0.0 {
        return [0.0, 0.0, 1.0];
    }
    let inv = 1.0 / len2.sqrt();
    [v.x * inv, v.y * inv, v.z * inv]
}

fn normalize2(v: [f32; 2]) -> [f32; 2] {
    let len2 = v[0] * v[0] + v[1] * v[1];
    if len2 <= 0.0 {
        return [1.0, 0.0];
    }
    let inv = 1.0 / len2.sqrt();
    [v[0] * inv, v[1] * inv]
}

fn precompute_world_covariances(scene: &SceneBuffers) -> Vec<[[f32; 3]; 3]> {
    let mut out = Vec::with_capacity(scene.len());
    for i in 0..scene.len() {
        let scale = scene.scale_xyz[i];
        let sx = scale[0].exp().max(1e-6);
        let sy = scale[1].exp().max(1e-6);
        let sz = scale[2].exp().max(1e-6);
        let object_cov = [
            [sx * sx, 0.0, 0.0],
            [0.0, sy * sy, 0.0],
            [0.0, 0.0, sz * sz],
        ];
        let rot_gaussian = quat_to_mat3(quat_normalize(scene.rotation_xyzw[i]));
        let world_cov = mat3_mul(
            mat3_mul(rot_gaussian, object_cov),
            mat3_transpose(rot_gaussian),
        );
        out.push(world_cov);
    }
    out
}

#[derive(Clone, Copy)]
struct ShColorLayout<'a> {
    rest: Option<&'a [f32]>,
    degree: u8,
    per_channel: usize,
    stride: usize,
}

impl<'a> ShColorLayout<'a> {
    fn new(scene: &'a SceneBuffers) -> Self {
        let rest = scene.sh_rest.as_deref();
        let coeff_total = if rest.is_some() {
            (scene.sh_degree as usize + 1).pow(2)
        } else {
            1
        };
        let per_channel = coeff_total.saturating_sub(1);
        Self {
            rest,
            degree: if rest.is_some() { scene.sh_degree } else { 0 },
            per_channel,
            stride: per_channel * 3,
        }
    }
}

unsafe fn sh_color_unchecked(
    scene: &SceneBuffers,
    index: usize,
    dir: [f32; 3],
    layout: ShColorLayout<'_>,
) -> [f32; 3] {
    // The PLY stores SH coefficients as `f_dc_*` + `f_rest_*`. Evaluate as in 3DGS:
    // `rgb = clamp_min(eval_sh(deg, sh, dir) + 0.5, 0.0)`.
    // Reference: graphdeco-inria/gaussian-splatting `utils/sh_utils.py`.
    const C0: f32 = 0.28209479177387814_f32;
    let dc = unsafe { *scene.color_dc.get_unchecked(index) };
    let mut rgb = [C0 * dc[0], C0 * dc[1], C0 * dc[2]];

    if let Some(rest) = layout.rest {
        let base = index * layout.stride;
        if layout.degree == 3 && layout.per_channel == 15 {
            if let Some(end) = base.checked_add(45) {
                if end <= rest.len() {
                    let sh_rgb = sh_color_rest_deg3(dir, &rest[base..end]);
                    return [
                        (rgb[0] + sh_rgb[0] + 0.5).max(0.0),
                        (rgb[1] + sh_rgb[1] + 0.5).max(0.0),
                        (rgb[2] + sh_rgb[2] + 0.5).max(0.0),
                    ];
                }
            }
        }

        let (basis, basis_len) = sh_basis(layout.degree, dir);
        for (channel, value) in rgb.iter_mut().enumerate() {
            let channel_base = base + channel * layout.per_channel;
            if channel_base >= rest.len() {
                continue;
            }
            let available = rest.len() - channel_base;
            let term_count = basis_len.min(layout.per_channel).min(available);
            *value += dot_sh_terms(&basis, &rest[channel_base..], term_count);
        }
    }

    [
        (rgb[0] + 0.5).max(0.0),
        (rgb[1] + 0.5).max(0.0),
        (rgb[2] + 0.5).max(0.0),
    ]
}

fn sh_color_rest_deg3(dir: [f32; 3], rest: &[f32]) -> [f32; 3] {
    debug_assert!(rest.len() >= 45);

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

    let x = dir[0];
    let y = dir[1];
    let z = dir[2];
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let yz = y * z;
    let xz = x * z;

    let b0 = -C1 * y;
    let b1 = C1 * z;
    let b2 = -C1 * x;
    let b3 = C2[0] * xy;
    let b4 = C2[1] * yz;
    let b5 = C2[2] * (2.0 * zz - xx - yy);
    let b6 = C2[3] * xz;
    let b7 = C2[4] * (xx - yy);
    let b8 = C3[0] * y * (3.0 * xx - yy);
    let b9 = C3[1] * xy * z;
    let b10 = C3[2] * y * (4.0 * zz - xx - yy);
    let b11 = C3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy);
    let b12 = C3[4] * x * (4.0 * zz - xx - yy);
    let b13 = C3[5] * z * (xx - yy);
    let b14 = C3[6] * x * (xx - 3.0 * yy);

    [
        sh_dot15(
            rest, b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14,
        ),
        sh_dot15(
            &rest[15..],
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
            b7,
            b8,
            b9,
            b10,
            b11,
            b12,
            b13,
            b14,
        ),
        sh_dot15(
            &rest[30..],
            b0,
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
            b7,
            b8,
            b9,
            b10,
            b11,
            b12,
            b13,
            b14,
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn sh_dot15(
    rest: &[f32],
    b0: f32,
    b1: f32,
    b2: f32,
    b3: f32,
    b4: f32,
    b5: f32,
    b6: f32,
    b7: f32,
    b8: f32,
    b9: f32,
    b10: f32,
    b11: f32,
    b12: f32,
    b13: f32,
    b14: f32,
) -> f32 {
    debug_assert!(rest.len() >= 15);
    b0 * rest[0]
        + b1 * rest[1]
        + b2 * rest[2]
        + b3 * rest[3]
        + b4 * rest[4]
        + b5 * rest[5]
        + b6 * rest[6]
        + b7 * rest[7]
        + b8 * rest[8]
        + b9 * rest[9]
        + b10 * rest[10]
        + b11 * rest[11]
        + b12 * rest[12]
        + b13 * rest[13]
        + b14 * rest[14]
}

fn sh_basis(deg: u8, dir: [f32; 3]) -> ([f32; 24], usize) {
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

    let mut basis = [0.0_f32; 24];
    let x = dir[0];
    let y = dir[1];
    let z = dir[2];
    if deg == 0 {
        return (basis, 0);
    }

    basis[0] = -C1 * y;
    basis[1] = C1 * z;
    basis[2] = -C1 * x;
    if deg == 1 {
        return (basis, 3);
    }

    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let yz = y * z;
    let xz = x * z;

    basis[3] = C2[0] * xy;
    basis[4] = C2[1] * yz;
    basis[5] = C2[2] * (2.0 * zz - xx - yy);
    basis[6] = C2[3] * xz;
    basis[7] = C2[4] * (xx - yy);
    if deg == 2 {
        return (basis, 8);
    }

    basis[8] = C3[0] * y * (3.0 * xx - yy);
    basis[9] = C3[1] * xy * z;
    basis[10] = C3[2] * y * (4.0 * zz - xx - yy);
    basis[11] = C3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy);
    basis[12] = C3[4] * x * (4.0 * zz - xx - yy);
    basis[13] = C3[5] * z * (xx - yy);
    basis[14] = C3[6] * x * (xx - 3.0 * yy);
    if deg == 3 {
        return (basis, 15);
    }

    // deg 4 support (rare for 3DGS, but safe to handle).
    basis[15] = C4[0] * xy * (xx - yy);
    basis[16] = C4[1] * yz * (3.0 * xx - yy);
    basis[17] = C4[2] * xy * (7.0 * zz - 1.0);
    basis[18] = C4[3] * yz * (7.0 * zz - 3.0);
    basis[19] = C4[4] * (zz * (35.0 * zz - 30.0) + 3.0);
    basis[20] = C4[5] * xz * (7.0 * zz - 3.0);
    basis[21] = C4[6] * (xx - yy) * (7.0 * zz - 1.0);
    basis[22] = C4[7] * xz * (xx - 3.0 * yy);
    basis[23] = C4[8] * (xx * (xx - 3.0 * yy) - yy * (3.0 * xx - yy));
    (basis, 24)
}

fn dot_sh_terms(basis: &[f32; 24], rest: &[f32], count: usize) -> f32 {
    debug_assert!(count <= basis.len());
    debug_assert!(count <= rest.len());

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: AArch64 guarantees Neon availability and the count has been bounds-checked.
        unsafe {
            return dot_sh_terms_neon(basis, rest, count);
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut result = 0.0_f32;
        for i in 0..count {
            result += basis[i] * rest[i];
        }
        result
    }
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn dot_sh_terms_neon(basis: &[f32; 24], rest: &[f32], count: usize) -> f32 {
    use std::arch::aarch64::*;

    let mut sum4 = vdupq_n_f32(0.0);
    let mut i = 0_usize;
    while i + 4 <= count {
        let b = unsafe { vld1q_f32(basis.as_ptr().add(i)) };
        let r = unsafe { vld1q_f32(rest.as_ptr().add(i)) };
        sum4 = vmlaq_f32(sum4, b, r);
        i += 4;
    }

    let mut lanes = [0.0_f32; 4];
    unsafe { vst1q_f32(lanes.as_mut_ptr(), sum4) };
    let mut result = lanes[0] + lanes[1] + lanes[2] + lanes[3];
    while i < count {
        result += unsafe { *basis.as_ptr().add(i) } * unsafe { *rest.as_ptr().add(i) };
        i += 1;
    }
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
                label: wgpu_label("gsplat-render-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|_| GpuRasterError::DeviceCreation)?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("splat-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/splat.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("splat-bgl"),
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
            label: wgpu_label("splat-pl"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: wgpu_label("splat-rp"),
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
                label: wgpu_label("splat-render-encoder"),
            });

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: wgpu_label("splat-render-pass"),
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
            label: wgpu_label("splat-readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: wgpu_label("splat-readback-encoder"),
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
        label: wgpu_label("splat-output"),
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
        label: wgpu_label("splat-instance-buffer"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("splat-bg"),
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
    use gsplat_core::{Camera, ErrorCode, RenderMode, RendererConfig, SceneBuffers, Vec3f};

    use super::{
        Renderer, build_instances, ellipse_axes_from_covariance, project_covariance_to_ndc,
        quat_inverse,
    };

    fn build_scene() -> SceneBuffers {
        SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 0.5), Vec3f::new(0.0, 0.0, 2.0)],
            opacity: vec![0.9, 0.8],
            scale_xyz: vec![[0.0, 0.0, 0.0], [0.2, 0.2, 0.2]],
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
    fn render_frame_rejects_invalid_camera() {
        let mut renderer = Renderer::new(RenderMode::SortedAlpha).unwrap();
        renderer.load_scene(build_scene()).unwrap();
        let mut camera = Camera::default();
        camera.intrinsics.vertical_fov_radians = 0.0;

        let err = renderer.render_frame(&camera).unwrap_err();

        assert_eq!(err.code(), ErrorCode::InvalidArgument);
    }

    #[test]
    fn quaternion_inverse_normalizes_scaled_input() {
        assert_eq!(quat_inverse([0.0, 0.0, 0.0, 2.0]), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn renderer_constructs_without_required_gpu_adapter() {
        let renderer = Renderer::new(RenderMode::SortedAlpha).unwrap();
        let _ = renderer.has_gpu_rasterizer();
    }

    #[test]
    fn covariance_projection_produces_nonzero_ellipse_axes() {
        let camera = Camera::default();
        let config = RendererConfig::default();
        let cov_cam = [
            [0.020, 0.000, 0.000],
            [0.000, 0.005, 0.000],
            [0.000, 0.000, 0.002],
        ];
        let cov2 = project_covariance_to_ndc(Vec3f::new(0.2, -0.1, 2.0), cov_cam, &camera, config)
            .expect("covariance should project");
        let (axis_u, axis_v) =
            ellipse_axes_from_covariance(cov2).expect("ellipse axes should be finite");

        let lu = (axis_u[0] * axis_u[0] + axis_u[1] * axis_u[1]).sqrt();
        let lv = (axis_v[0] * axis_v[0] + axis_v[1] * axis_v[1]).sqrt();
        assert!(lu > 0.0);
        assert!(lv > 0.0);
        assert!(lu > lv);
    }

    #[test]
    fn build_instances_generates_anisotropic_oriented_axes() {
        let qz = 0.5_f32.sqrt(); // sin/cos(90deg / 2)
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(0.0, 0.0, 2.0)],
            opacity: vec![1.0],
            scale_xyz: vec![[0.8, -0.4, -0.4]],
            rotation_xyzw: vec![[0.0, 0.0, qz, qz]],
            color_dc: vec![[0.2, 0.3, 0.4]],
            sh_degree: 0,
            sh_rest: None,
        };

        let world_cov = super::precompute_world_covariances(&scene);
        let alpha_values = super::precompute_alpha_values(&scene);
        let instances = build_instances(
            &scene,
            &world_cov,
            &alpha_values,
            &[0],
            &Camera::default(),
            RendererConfig::default(),
        );
        assert_eq!(instances.len(), 1);
        let inst = instances[0];
        let axis_u = [inst.center_and_axis_u[2], inst.center_and_axis_u[3]];
        let axis_v = [inst.axis_v_and_pad[0], inst.axis_v_and_pad[1]];

        let lu = (axis_u[0] * axis_u[0] + axis_u[1] * axis_u[1]).sqrt();
        let lv = (axis_v[0] * axis_v[0] + axis_v[1] * axis_v[1]).sqrt();
        let dot = axis_u[0] * axis_v[0] + axis_u[1] * axis_v[1];
        assert!(lu > lv);
        assert!(dot.abs() < 1e-4);
    }

    #[test]
    fn build_instances_keeps_partial_splats_with_offscreen_center() {
        let scene = SceneBuffers {
            positions: vec![Vec3f::new(2.3, 0.0, 1.0)],
            opacity: vec![1.0],
            // Large sigma to ensure the projected ellipse overlaps the viewport.
            scale_xyz: vec![[2.0, 2.0, 2.0]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.2, 0.3, 0.4]],
            sh_degree: 0,
            sh_rest: None,
        };

        let world_cov = super::precompute_world_covariances(&scene);
        let alpha_values = super::precompute_alpha_values(&scene);
        let instances = build_instances(
            &scene,
            &world_cov,
            &alpha_values,
            &[0],
            &Camera::default(),
            RendererConfig::default(),
        );
        assert_eq!(instances.len(), 1);

        let inst = instances[0];
        let center_x = inst.center_and_axis_u[0];
        let extent_x = inst.center_and_axis_u[2].abs() + inst.axis_v_and_pad[0].abs();
        assert!(center_x > 2.0);
        assert!(center_x - extent_x <= 1.0);
    }
}
