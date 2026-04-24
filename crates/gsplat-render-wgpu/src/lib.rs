//! WGPU renderer with a SortedAlpha reference path.

use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use gsplat_core::{Camera, ErrorCode, FrameStats, RenderMode, RendererConfig, SceneBuffers, Vec3f};
use gsplat_sort::{CpuSortBackend, SortBackend, SortError};
use rayon::prelude::*;
use thiserror::Error;
use wgpu::util::DeviceExt;

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

pub struct Renderer {
    mode: RenderMode,
    config: RendererConfig,
    cpu_sort_backend: CpuSortBackend,
    gpu_rasterizer: Option<GpuRasterizer>,
    scene: Option<SceneBuffers>,
    world_covariances: Option<Vec<[[f32; 3]; 3]>>,
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
        let world_covariances = precompute_world_covariances(&scene);
        self.scene = Some(scene);
        self.world_covariances = Some(world_covariances);
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
        camera
            .validate()
            .map_err(|_| RendererError::InvalidCamera)?;

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

    pub fn build_sorted_instances(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<GpuInstance>, FrameStats), RendererError> {
        let frame_start = Instant::now();

        let preprocess_start = Instant::now();
        let mut preprocessed = self.preprocess_visible(camera)?;
        let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

        let sort_start = Instant::now();
        self.sort_preprocessed(&mut preprocessed)?;
        let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

        let raster_start = Instant::now();
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let world_covariances = self
            .world_covariances
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        let instances = build_instances(
            scene,
            world_covariances,
            &preprocessed.indices,
            camera,
            self.config,
        );
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
        Ok((instances, stats))
    }

    pub fn build_sorted_indices(
        &mut self,
        camera: &Camera,
    ) -> Result<(Vec<u32>, FrameStats), RendererError> {
        let frame_start = Instant::now();

        let preprocess_start = Instant::now();
        let mut preprocessed = self.preprocess_visible(camera)?;
        let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

        let sort_start = Instant::now();
        self.sort_preprocessed(&mut preprocessed)?;
        let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

        let visible_count = preprocessed.indices.len() as u32;
        let stats = FrameStats {
            frame_ms: frame_start.elapsed().as_secs_f32() * 1000.0,
            preprocess_ms,
            sort_ms,
            raster_ms: 0.0,
            visible_count,
            drawn_count: visible_count,
        };
        self.last_stats = stats;
        Ok((preprocessed.indices, stats))
    }

    pub fn render_frame(&mut self, camera: &Camera) -> Result<FrameStats, RendererError> {
        let frame_start = Instant::now();

        let preprocess_start = Instant::now();
        let mut preprocessed = self.preprocess_visible(camera)?;
        let preprocess_ms = preprocess_start.elapsed().as_secs_f32() * 1000.0;

        let sort_start = Instant::now();
        self.sort_preprocessed(&mut preprocessed)?;
        let sort_ms = sort_start.elapsed().as_secs_f32() * 1000.0;

        let raster_start = Instant::now();
        let scene = self.scene.as_ref().ok_or(RendererError::SceneNotLoaded)?;
        let world_covariances = self
            .world_covariances
            .as_deref()
            .ok_or(RendererError::InvalidScene)?;
        let instances = build_instances(
            scene,
            world_covariances,
            &preprocessed.indices,
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

    fn sort_preprocessed(
        &mut self,
        preprocessed: &mut PreprocessOutput,
    ) -> Result<(), RendererError> {
        if self.mode == RenderMode::SortedAlpha {
            self.cpu_sort_backend
                .sort_pairs(&mut preprocessed.depth_keys, &mut preprocessed.indices)?;
        }
        Ok(())
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
            label: Some("gsplat-render-preprocess-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/preprocess_instances.wgsl").into(),
            ),
        });
        let preprocess_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("gsplat-render-preprocess-bgl"),
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
                label: Some("gsplat-render-preprocess-pl"),
                bind_group_layouts: &[&preprocess_bind_group_layout],
                immediate_size: 0,
            });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gsplat-render-preprocess-cp"),
            layout: Some(&preprocess_pipeline_layout),
            module: &preprocess_shader,
            entry_point: Some("main"),
            cache: None,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        });

        let scene_len = scene.len().max(1);
        let scene_data: Vec<GpuSceneElem> = if scene.positions.is_empty() {
            vec![GpuSceneElem {
                position: [0.0, 0.0, 0.0, 0.0],
                cov_row0: [0.0, 0.0, 0.0, 0.0],
                cov_row1: [0.0, 0.0, 0.0, 0.0],
                cov_row2: [0.0, 0.0, 0.0, 0.0],
                color_dc: [0.0, 0.0, 0.0, 0.0],
                opacity_and_pad: [0.0, 0.0, 0.0, 0.0],
            }]
        } else {
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
        };
        let scene_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("gsplat-render-preprocess-scene"),
            contents: bytemuck::cast_slice(&scene_data),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let sorted_indices_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gsplat-render-preprocess-sorted-indices"),
            size: (scene_len as u64) * std::mem::size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let empty_rest = [0.0_f32];
        let sh_rest = scene.sh_rest.as_deref().unwrap_or(&empty_rest);
        let sh_rest_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("gsplat-render-preprocess-sh-rest"),
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
            label: Some("gsplat-render-preprocess-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gsplat-render-preprocess-bg"),
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
            label: Some("gsplat-render-preprocess-pass"),
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
    indices: &[u32],
    camera: &Camera,
    config: RendererConfig,
) -> Vec<GpuInstance> {
    if world_covariances.len() != scene.len() {
        return Vec::new();
    }

    let aspect = config.width as f32 / config.height as f32;
    let f = 1.0 / (camera.intrinsics.vertical_fov_radians * 0.5).tan();
    let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);
    let view_rot = quat_to_mat3(camera_inv_q);
    let view_rot_t = mat3_transpose(view_rot);

    indices
        .par_iter()
        .filter_map(|&idx| {
            let i = idx as usize;
            let pos_world = scene.positions[i];
            let p_cam = world_to_camera_with_inv_q(pos_world, camera, camera_inv_q);
            // Preprocess already culled by z range, but keep this safe for runtime camera changes.
            if !is_visible(p_cam.z, camera) {
                return None;
            }

            // Project center to NDC for instance placement.
            let inv_z = 1.0 / p_cam.z.max(1e-6);
            let x_ndc = (p_cam.x * f) * inv_z / aspect;
            let y_ndc = (p_cam.y * f) * inv_z;

            let world_cov = world_covariances[i];
            let cam_cov = mat3_mul(mat3_mul(view_rot, world_cov), view_rot_t);
            let cov2_ndc = project_covariance_to_ndc(p_cam, cam_cov, camera, config)?;
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
        })
        .collect()
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn world_to_camera(pos_world: Vec3f, camera: &Camera) -> Vec3f {
    let inv_q = quat_inverse(camera.pose.rotation_xyzw);
    world_to_camera_with_inv_q(pos_world, camera, inv_q)
}

fn world_to_camera_with_inv_q(pos_world: Vec3f, camera: &Camera, camera_inv_q: [f32; 4]) -> Vec3f {
    let p = Vec3f::new(
        pos_world.x - camera.pose.position.x,
        pos_world.y - camera.pose.position.y,
        pos_world.z - camera.pose.position.z,
    );

    quat_rotate(camera_inv_q, p)
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

fn project_covariance_to_ndc(
    p_cam: Vec3f,
    cov_cam: [[f32; 3]; 3],
    camera: &Camera,
    config: RendererConfig,
) -> Option<[[f32; 2]; 2]> {
    if config.width == 0 || config.height == 0 {
        return None;
    }
    let z = p_cam.z;
    if z <= 1e-6 || !z.is_finite() {
        return None;
    }

    let aspect = config.width as f32 / config.height as f32;
    let tan_half_fovy = (camera.intrinsics.vertical_fov_radians * 0.5).tan();
    if tan_half_fovy <= 0.0 || !tan_half_fovy.is_finite() {
        return None;
    }
    let f = 1.0 / tan_half_fovy;
    let fx = f / aspect;
    let fy = f;

    // Match common 3DGS covariance projection behavior: clamp view-space x/z and y/z
    // before Jacobian evaluation to avoid extreme derivatives at frustum edges.
    let tan_half_fovx = tan_half_fovy * aspect;
    let lim_x = 1.3 * tan_half_fovx;
    let lim_y = 1.3 * tan_half_fovy;
    let x_clamped = (p_cam.x / z).clamp(-lim_x, lim_x) * z;
    let y_clamped = (p_cam.y / z).clamp(-lim_y, lim_y) * z;

    let inv_z = 1.0 / z;
    let inv_z2 = inv_z * inv_z;
    let j = [
        [fx * inv_z, 0.0, -fx * x_clamped * inv_z2],
        [0.0, fy * inv_z, -fy * y_clamped * inv_z2],
    ];

    let mut cov2 = [[0.0_f32; 2]; 2];
    for r in 0..2 {
        for c in r..2 {
            let mut sum = 0.0_f32;
            for i in 0..3 {
                for k in 0..3 {
                    sum += j[r][i] * cov_cam[i][k] * j[c][k];
                }
            }
            cov2[r][c] = sum;
            cov2[c][r] = sum;
        }
    }

    // Low-pass filter in NDC space to keep splats from collapsing to subpixel noise.
    let blur_pixels = 0.3_f32;
    let px_ndc_x = 2.0 / config.width as f32;
    let px_ndc_y = 2.0 / config.height as f32;
    cov2[0][0] += (blur_pixels * px_ndc_x).powi(2);
    cov2[1][1] += (blur_pixels * px_ndc_y).powi(2);

    if !cov2[0][0].is_finite()
        || !cov2[0][1].is_finite()
        || !cov2[1][0].is_finite()
        || !cov2[1][1].is_finite()
    {
        return None;
    }

    Some(cov2)
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
        let instances = build_instances(
            &scene,
            &world_cov,
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
        let instances = build_instances(
            &scene,
            &world_cov,
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
