//! Resident GPU scene buffers, preflight, and bind-group/pipeline setup.

use bytemuck::{Pod, Zeroable};
use gsplat_core::Camera;
use gsplat_core::SceneBuffers;
use wgpu::util::DeviceExt;

use crate::draw_pass;
use crate::math::{CameraCovarianceTerms, quat_inverse, quat_to_mat3};
use crate::project::{
    PROJECTED_RECORD_STRIDE, ProjectBindGroupBuffers, ProjectShBindings, create_draw_bind_group,
    create_project_bind_group, create_project_bind_group_layout, create_project_pipeline,
    encode_project, encode_project_indirect, project_shader_source, validate_project_dispatch,
};
use crate::quantized::{
    QUANTIZED_SOURCE_STRIDE, QUANTIZED_STORAGE_BUFFERS_PER_STAGE, ResidentStorageProfile,
    pack_quantized_sh_sidecar, pack_quantized_sources, quantized_sh_max_sidecar_bytes,
    quantized_sh_max_sidecar_stride,
};
use crate::timing::wgpu_label;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidentSceneResource {
    SortedIndices,
    Source,
    ShRest,
    Projected,
    DrawInstances,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResidentSceneResourceRequirement {
    pub resource: ResidentSceneResource,
    pub required_bytes: u64,
    pub limit_bytes: u64,
    pub fits: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidentScenePath {
    Resident,
    CapacityExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidentSceneRemediation {
    None,
    ReduceScene { max_resident_splats: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResidentScenePreflight {
    pub splat_count: u64,
    pub sh_degree: u8,
    pub path: ResidentScenePath,
    pub effective_storage_binding_limit: u64,
    pub effective_max_buffer_size: u64,
    pub requirements: [ResidentSceneResourceRequirement; 4],
    pub limiting_resource: ResidentSceneResource,
    pub max_resident_splats: u64,
    pub remediation: ResidentSceneRemediation,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResidentSceneError {
    #[error("sorted index buffer capacity exceeded")]
    SortedIndexCapacityExceeded,
    #[error("gpu order initialization failed: {0}")]
    GpuOrderInitialization(String),
    #[error("resident scene resource size overflow")]
    ResourceSizeOverflow,
    #[error("resident scene resources exceed effective device limits: {0:?}")]
    ResourceLimitExceeded(Box<ResidentScenePreflight>),
    #[error(
        "quantized resident storage needs {required} storage buffers per shader stage; device allows {available}"
    )]
    StorageBuffersPerStage { required: u32, available: u32 },
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuSurfaceSourceElem {
    pub(crate) position: [f32; 4],
    pub(crate) covariance0: [f32; 4],
    pub(crate) covariance1: [f32; 4],
    pub(crate) color_dc: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuSurfaceRenderParams {
    pub(crate) camera_pos: [f32; 4],
    pub(crate) view_rot_row0: [f32; 4],
    pub(crate) view_rot_row1: [f32; 4],
    pub(crate) view_rot_row2: [f32; 4],
    pub(crate) vertical_fov_radians: f32,
    pub(crate) near_plane: f32,
    pub(crate) far_plane: f32,
    pub(crate) aspect: f32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) sh_degree: u32,
    pub(crate) len: u32,
    pub(crate) order_stride_words: u32,
    pub(crate) order_id_offset_words: u32,
    pub(crate) _order_pad: [u32; 2],
}

pub(crate) fn make_surface_source_elems(
    scene: &SceneBuffers,
    world_covariance_terms: &[CameraCovarianceTerms],
    alpha_values: &[f32],
) -> Vec<GpuSurfaceSourceElem> {
    if scene.positions.is_empty() {
        return vec![GpuSurfaceSourceElem::zeroed()];
    }

    (0..scene.positions.len())
        .map(|i| {
            let position = scene.positions[i];
            let color_dc = scene.color_dc.get(i).copied().unwrap_or([0.0, 0.0, 0.0]);
            let cov = world_covariance_terms
                .get(i)
                .copied()
                .unwrap_or(CameraCovarianceTerms {
                    xx: 0.0,
                    xy: 0.0,
                    xz: 0.0,
                    yy: 0.0,
                    yz: 0.0,
                    zz: 0.0,
                });
            let alpha = alpha_values.get(i).copied().unwrap_or(0.0);
            GpuSurfaceSourceElem {
                position: [position.x, position.y, position.z, 0.0],
                covariance0: [cov.xx, cov.xy, cov.xz, cov.yy],
                covariance1: [cov.yz, cov.zz, alpha, 0.0],
                color_dc: [color_dc[0], color_dc[1], color_dc[2], 0.0],
            }
        })
        .collect()
}

pub(crate) fn make_surface_render_params(
    camera: &Camera,
    width: u32,
    height: u32,
    len: u32,
    sh_degree: u32,
) -> GpuSurfaceRenderParams {
    let camera_inv_q = quat_inverse(camera.pose.rotation_xyzw);
    let view_rot = quat_to_mat3(camera_inv_q);
    GpuSurfaceRenderParams {
        camera_pos: [
            camera.pose.position.x,
            camera.pose.position.y,
            camera.pose.position.z,
            0.0,
        ],
        view_rot_row0: [view_rot[0][0], view_rot[0][1], view_rot[0][2], 0.0],
        view_rot_row1: [view_rot[1][0], view_rot[1][1], view_rot[1][2], 0.0],
        view_rot_row2: [view_rot[2][0], view_rot[2][1], view_rot[2][2], 0.0],
        vertical_fov_radians: camera.intrinsics.vertical_fov_radians,
        near_plane: camera.intrinsics.near_plane,
        far_plane: camera.intrinsics.far_plane,
        aspect: (width as f32 / height.max(1) as f32).max(1e-6),
        width,
        height,
        sh_degree,
        len,
        order_stride_words: 1,
        order_id_offset_words: 0,
        _order_pad: [0; 2],
    }
}

pub fn resident_scene_preflight(
    splat_count: usize,
    sh_degree: u8,
    limits: &wgpu::Limits,
) -> Result<ResidentScenePreflight, ResidentSceneError> {
    resident_scene_preflight_for_profile(
        splat_count,
        sh_degree,
        limits,
        ResidentStorageProfile::FullF32,
    )
}

pub fn resident_scene_preflight_for_profile(
    splat_count: usize,
    sh_degree: u8,
    limits: &wgpu::Limits,
    profile: ResidentStorageProfile,
) -> Result<ResidentScenePreflight, ResidentSceneError> {
    let splat_count =
        u64::try_from(splat_count).map_err(|_| ResidentSceneError::ResourceSizeOverflow)?;
    if profile == ResidentStorageProfile::Quantized
        && limits.max_storage_buffers_per_shader_stage < QUANTIZED_STORAGE_BUFFERS_PER_STAGE
    {
        return Err(ResidentSceneError::StorageBuffersPerStage {
            required: QUANTIZED_STORAGE_BUFFERS_PER_STAGE,
            available: limits.max_storage_buffers_per_shader_stage,
        });
    }
    let capacity = splat_count.max(1);
    let binding_limit = u64::from(limits.max_storage_buffer_binding_size);
    let buffer_limit = limits.max_buffer_size;
    let effective_limit = binding_limit.min(buffer_limit);

    let order_stride = std::mem::size_of::<u32>() as u64;
    let (source_stride, sh_bytes, sh_capacity_stride) = match profile {
        ResidentStorageProfile::FullF32 => {
            let source_stride = std::mem::size_of::<GpuSurfaceSourceElem>() as u64;
            let degree = u64::from(sh_degree);
            let sh_stride = degree
                .checked_add(1)
                .and_then(|value| value.checked_mul(value))
                .and_then(|value| value.checked_sub(1))
                .and_then(|value| value.checked_mul(3))
                .and_then(|value| value.checked_mul(std::mem::size_of::<f32>() as u64))
                .ok_or(ResidentSceneError::ResourceSizeOverflow)?;
            let sh_bytes = if sh_stride == 0 {
                std::mem::size_of::<f32>() as u64
            } else {
                splat_count
                    .checked_mul(sh_stride)
                    .ok_or(ResidentSceneError::ResourceSizeOverflow)?
            };
            let sh_capacity = if sh_stride == 0 { u64::MAX } else { sh_stride };
            (source_stride, sh_bytes, sh_capacity)
        }
        ResidentStorageProfile::Quantized => {
            let sh_bytes = quantized_sh_max_sidecar_bytes(splat_count.max(1), sh_degree)
                .ok_or(ResidentSceneError::ResourceSizeOverflow)?;
            let rest = quantized_sh_max_sidecar_stride(sh_degree);
            (QUANTIZED_SOURCE_STRIDE, sh_bytes, rest)
        }
    };

    let order_bytes = capacity
        .checked_mul(order_stride)
        .ok_or(ResidentSceneError::ResourceSizeOverflow)?;
    let source_bytes = capacity
        .checked_mul(source_stride)
        .ok_or(ResidentSceneError::ResourceSizeOverflow)?;
    let projected_bytes = capacity
        .checked_mul(PROJECTED_RECORD_STRIDE)
        .ok_or(ResidentSceneError::ResourceSizeOverflow)?;

    let requirements = [
        ResidentSceneResourceRequirement {
            resource: ResidentSceneResource::SortedIndices,
            required_bytes: order_bytes,
            limit_bytes: effective_limit,
            fits: order_bytes <= effective_limit,
        },
        ResidentSceneResourceRequirement {
            resource: ResidentSceneResource::Source,
            required_bytes: source_bytes,
            limit_bytes: effective_limit,
            fits: source_bytes <= effective_limit,
        },
        ResidentSceneResourceRequirement {
            resource: ResidentSceneResource::ShRest,
            required_bytes: sh_bytes,
            limit_bytes: effective_limit,
            fits: sh_bytes <= effective_limit,
        },
        ResidentSceneResourceRequirement {
            resource: ResidentSceneResource::Projected,
            required_bytes: projected_bytes,
            limit_bytes: effective_limit,
            fits: projected_bytes <= effective_limit,
        },
    ];

    let capacities = [
        (
            ResidentSceneResource::SortedIndices,
            effective_limit / order_stride,
        ),
        (
            ResidentSceneResource::Source,
            effective_limit / source_stride,
        ),
        (
            ResidentSceneResource::ShRest,
            if sh_capacity_stride == u64::MAX {
                u64::MAX
            } else {
                effective_limit / sh_capacity_stride
            },
        ),
        (
            ResidentSceneResource::Projected,
            effective_limit / PROJECTED_RECORD_STRIDE,
        ),
        (ResidentSceneResource::DrawInstances, u64::from(u32::MAX)),
    ];
    let (limiting_resource, max_resident_splats) = capacities
        .into_iter()
        .min_by_key(|(_, capacity)| *capacity)
        .expect("resident resource capacity list is non-empty");
    let fits = requirements.iter().all(|requirement| requirement.fits)
        && splat_count <= u64::from(u32::MAX);
    let path = if fits {
        ResidentScenePath::Resident
    } else {
        ResidentScenePath::CapacityExceeded
    };
    let remediation = if fits {
        ResidentSceneRemediation::None
    } else {
        ResidentSceneRemediation::ReduceScene {
            max_resident_splats,
        }
    };

    Ok(ResidentScenePreflight {
        splat_count,
        sh_degree,
        path,
        effective_storage_binding_limit: binding_limit,
        effective_max_buffer_size: buffer_limit,
        requirements,
        limiting_resource,
        max_resident_splats,
        remediation,
    })
}

pub(crate) struct ResidentSceneResources {
    sorted_indices_buffer: wgpu::Buffer,
    params_buffer: wgpu::Buffer,
    pub(crate) draw_bind_group: wgpu::BindGroup,
    capacity: usize,
    count: usize,
    sh_degree: u32,
    profile: ResidentStorageProfile,
    source_buffer: wgpu::Buffer,
    sh_rest_buffer: wgpu::Buffer,
    sh_sidecars: Option<[wgpu::Buffer; 3]>,
    projected_buffer: wgpu::Buffer,
    project_pipeline: wgpu::ComputePipeline,
    project_layout: wgpu::BindGroupLayout,
    cpu_project_bind_group: wgpu::BindGroup,
    gpu_order: Option<ResidentGpuSceneOrder>,
}

pub(crate) struct ResidentGpuSceneOrder {
    pub(crate) sorter: crate::resident_gpu_order::ResidentGpuOrder,
    project_bind_group: wgpu::BindGroup,
}

impl ResidentSceneResources {
    pub(crate) fn new(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        scene: &SceneBuffers,
        world_covariance_terms: &[CameraCovarianceTerms],
        alpha_values: &[f32],
        profile: ResidentStorageProfile,
    ) -> Result<Self, ResidentSceneError> {
        let preflight = resident_scene_preflight_for_profile(
            scene.len(),
            scene.sh_degree,
            &device.limits(),
            profile,
        )?;
        if preflight.path != ResidentScenePath::Resident {
            return Err(ResidentSceneError::ResourceLimitExceeded(Box::new(
                preflight,
            )));
        }
        let capacity = scene.len().max(1);
        let capacity_u32 =
            u32::try_from(capacity).map_err(|_| ResidentSceneError::SortedIndexCapacityExceeded)?;
        validate_project_dispatch(&device.limits(), capacity_u32)?;
        let sorted_indices_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-resident-sorted-indices"),
            size: (capacity as u64) * (std::mem::size_of::<u32>() as u64),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-resident-params"),
            contents: bytemuck::bytes_of(&GpuSurfaceRenderParams::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let (source_buffer, sh_rest_buffer, sh_sidecars) = match profile {
            ResidentStorageProfile::FullF32 => {
                let source_elems =
                    make_surface_source_elems(scene, world_covariance_terms, alpha_values);
                let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: wgpu_label("gsplat-resident-source"),
                    contents: bytemuck::cast_slice(&source_elems),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                let sh_rest_fallback = [0.0_f32];
                let sh_rest = scene.sh_rest.as_deref().unwrap_or(&sh_rest_fallback);
                let sh_rest_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: wgpu_label("gsplat-resident-sh-rest"),
                    contents: bytemuck::cast_slice(sh_rest),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                (source_buffer, sh_rest_buffer, None)
            }
            ResidentStorageProfile::Quantized => {
                let source_elems = pack_quantized_sources(scene, alpha_values);
                let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: wgpu_label("gsplat-resident-quantized-source"),
                    contents: bytemuck::cast_slice(&source_elems),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                let sh1 = pack_quantized_sh_sidecar(scene, 1);
                let sh_rest_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: wgpu_label("gsplat-resident-quantized-sh1"),
                    contents: bytemuck::cast_slice(&sh1),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                let sh2 = pack_quantized_sh_sidecar(scene, 2);
                let sh3 = pack_quantized_sh_sidecar(scene, 3);
                let sh4 = pack_quantized_sh_sidecar(scene, 4);
                let sidecars = [
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: wgpu_label("gsplat-resident-quantized-sh2"),
                        contents: bytemuck::cast_slice(&sh2),
                        usage: wgpu::BufferUsages::STORAGE,
                    }),
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: wgpu_label("gsplat-resident-quantized-sh3"),
                        contents: bytemuck::cast_slice(&sh3),
                        usage: wgpu::BufferUsages::STORAGE,
                    }),
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: wgpu_label("gsplat-resident-quantized-sh4"),
                        contents: bytemuck::cast_slice(&sh4),
                        usage: wgpu::BufferUsages::STORAGE,
                    }),
                ];
                (source_buffer, sh_rest_buffer, Some(sidecars))
            }
        };
        let projected_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-resident-projected"),
            size: (capacity as u64) * PROJECTED_RECORD_STRIDE,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let project_layout = create_project_bind_group_layout(device, profile);
        let shader_source = project_shader_source(profile);
        let project_pipeline = create_project_pipeline(device, &project_layout, &shader_source);
        let cpu_project_bind_group = create_project_bind_group(
            device,
            &project_layout,
            "gsplat-resident-cpu-project-bind-group",
            ProjectBindGroupBuffers {
                order: &sorted_indices_buffer,
                source: &source_buffer,
                sh: project_sh_bindings(&sh_rest_buffer, sh_sidecars.as_ref()),
                params: &params_buffer,
                projected: &projected_buffer,
            },
        );
        let draw_bind_group = create_draw_bind_group(device, bind_group_layout, &projected_buffer);

        Ok(Self {
            sorted_indices_buffer,
            params_buffer,
            draw_bind_group,
            capacity,
            count: scene.len(),
            sh_degree: scene.sh_degree as u32,
            profile,
            source_buffer,
            sh_rest_buffer,
            sh_sidecars,
            projected_buffer,
            project_pipeline,
            project_layout,
            cpu_project_bind_group,
            gpu_order: None,
        })
    }

    pub(crate) fn prepare_cpu(
        &self,
        queue: &wgpu::Queue,
        sorted_indices: &[u32],
        camera: &Camera,
        width: u32,
        height: u32,
        upload_order: bool,
    ) -> Result<u32, ResidentSceneError> {
        if sorted_indices.len() > self.capacity {
            return Err(ResidentSceneError::SortedIndexCapacityExceeded);
        }
        if upload_order && !sorted_indices.is_empty() {
            queue.write_buffer(
                &self.sorted_indices_buffer,
                0,
                bytemuck::cast_slice(sorted_indices),
            );
        }
        let instance_count = sorted_indices.len() as u32;
        let params =
            make_surface_render_params(camera, width, height, instance_count, self.sh_degree);
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(instance_count)
    }

    pub(crate) fn encode_project(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        instance_count: u32,
        use_gpu_order: bool,
    ) {
        let bind_group = if use_gpu_order {
            self.gpu_order
                .as_ref()
                .map(|order| &order.project_bind_group)
                .unwrap_or(&self.cpu_project_bind_group)
        } else {
            &self.cpu_project_bind_group
        };
        if use_gpu_order && let Some(order) = &self.gpu_order {
            encode_project_indirect(
                encoder,
                &self.project_pipeline,
                bind_group,
                order.sorter.indirect_args(),
                crate::resident_gpu_order::ORDER_META_DISPATCH_OFFSET,
            );
            return;
        }
        encode_project(encoder, &self.project_pipeline, bind_group, instance_count);
    }

    pub(crate) fn ensure_gpu_order(
        &mut self,
        device: &wgpu::Device,
    ) -> Result<(), ResidentSceneError> {
        if self.gpu_order.is_some() {
            return Ok(());
        }
        let count = u32::try_from(self.count)
            .map_err(|_| ResidentSceneError::SortedIndexCapacityExceeded)?;
        let capacity = u32::try_from(self.capacity)
            .map_err(|_| ResidentSceneError::SortedIndexCapacityExceeded)?;
        crate::resident_gpu_order::ResidentGpuOrder::validate_dispatch_limits(
            device, capacity, count,
        )?;
        #[cfg(not(target_arch = "wasm32"))]
        let (validation_scope, oom_scope, internal_scope) = (
            device.push_error_scope(wgpu::ErrorFilter::Validation),
            device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        let sorter = crate::resident_gpu_order::ResidentGpuOrder::new(
            device,
            &self.source_buffer,
            &self.params_buffer,
            capacity,
            count,
            self.profile,
        )?;
        let project_bind_group = create_project_bind_group(
            device,
            &self.project_layout,
            "gsplat-resident-gpu-project-bind-group",
            ProjectBindGroupBuffers {
                order: sorter.final_pairs(),
                source: &self.source_buffer,
                sh: project_sh_bindings(&self.sh_rest_buffer, self.sh_sidecars.as_ref()),
                params: &self.params_buffer,
                projected: &self.projected_buffer,
            },
        );
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(error) = [
            pollster::block_on(internal_scope.pop()),
            pollster::block_on(oom_scope.pop()),
            pollster::block_on(validation_scope.pop()),
        ]
        .into_iter()
        .flatten()
        .next()
        {
            return Err(ResidentSceneError::GpuOrderInitialization(
                error.to_string(),
            ));
        }
        self.gpu_order = Some(ResidentGpuSceneOrder {
            sorter,
            project_bind_group,
        });
        Ok(())
    }

    pub(crate) fn prepare_gpu(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<u32, ResidentSceneError> {
        self.ensure_gpu_order(device)?;
        let instance_count = u32::try_from(self.count)
            .map_err(|_| ResidentSceneError::SortedIndexCapacityExceeded)?;
        let mut params =
            make_surface_render_params(camera, width, height, instance_count, self.sh_degree);
        params.order_stride_words = 2;
        params.order_id_offset_words = 1;
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(instance_count)
    }

    pub(crate) fn gpu_order(&self) -> Option<&ResidentGpuSceneOrder> {
        self.gpu_order.as_ref()
    }
}

fn project_sh_bindings<'a>(
    sh1: &'a wgpu::Buffer,
    sidecars: Option<&'a [wgpu::Buffer; 3]>,
) -> ProjectShBindings<'a> {
    match sidecars {
        Some(sidecars) => ProjectShBindings::Quantized {
            sh1,
            sh2: &sidecars[0],
            sh3: &sidecars[1],
            sh4: &sidecars[2],
        },
        None => ProjectShBindings::FullF32 { rest: sh1 },
    }
}

pub(crate) fn create_resident_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    crate::project::create_draw_bind_group_layout(device)
}

pub(crate) fn create_resident_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    draw_pass::create_splat_pipeline(
        device,
        bind_group_layout,
        format,
        draw_pass::SplatPipeline {
            shader_label: "gsplat-resident-shader",
            shader_source: include_str!("../shaders/splat_surface_resident.wgsl"),
            layout_label: "gsplat-resident-pipeline-layout",
            pipeline_label: "gsplat-resident-pipeline",
            topology: wgpu::PrimitiveTopology::TriangleList,
        },
    )
}
