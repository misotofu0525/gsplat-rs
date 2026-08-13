//! Per-splat compute projection: writes compact draw records once per refresh.

use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};

use crate::quantized::ResidentStorageProfile;
use crate::resident::ResidentSceneError;
use crate::timing::wgpu_label;

pub(crate) const PROJECT_WORKGROUP_SIZE: u32 = 64;
pub(crate) const PROJECT_ITEMS_PER_THREAD: u32 = 4;
pub(crate) const PROJECT_TILE_SIZE: u32 = PROJECT_WORKGROUP_SIZE * PROJECT_ITEMS_PER_THREAD;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuProjectedRecord {
    pub(crate) center: [f32; 2],
    pub(crate) axis_u: [f32; 2],
    pub(crate) axis_v: [f32; 2],
    pub(crate) _pad: [f32; 2],
    pub(crate) color: [f32; 4],
}

pub(crate) const PROJECTED_RECORD_STRIDE: u64 = std::mem::size_of::<GpuProjectedRecord>() as u64;

pub(crate) fn project_workgroup_count(count: u32) -> u32 {
    count.div_ceil(PROJECT_TILE_SIZE).max(1)
}

pub(crate) fn validate_project_dispatch(
    limits: &wgpu::Limits,
    capacity: u32,
) -> Result<(), ResidentSceneError> {
    let groups = project_workgroup_count(capacity.max(1));
    let dispatch_limit = limits.max_compute_workgroups_per_dimension;
    if groups > dispatch_limit {
        return Err(ResidentSceneError::GpuOrderInitialization(format!(
            "resident project compute requires {groups} workgroups; device limit is {dispatch_limit}"
        )));
    }
    Ok(())
}

pub(crate) fn preprocess_shader_source() -> String {
    format!(
        "{}\n{}",
        include_str!("../shaders/splat_common.wgsl"),
        include_str!("../shaders/splat_preprocess.wgsl"),
    )
}

pub(crate) fn quantized_preprocess_shader_source() -> String {
    format!(
        "{}\n{}",
        include_str!("../shaders/splat_common.wgsl"),
        include_str!("../shaders/splat_preprocess_quantized.wgsl"),
    )
}

pub(crate) fn project_shader_source(profile: ResidentStorageProfile) -> String {
    match profile {
        ResidentStorageProfile::FullF32 => preprocess_shader_source(),
        ResidentStorageProfile::Quantized => quantized_preprocess_shader_source(),
    }
}

pub(crate) fn create_project_bind_group_layout(
    device: &wgpu::Device,
    profile: ResidentStorageProfile,
) -> wgpu::BindGroupLayout {
    match profile {
        ResidentStorageProfile::FullF32 => {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: wgpu_label("gsplat-resident-project-bgl"),
                entries: &[
                    storage_entry(0, true),
                    storage_entry(1, true),
                    storage_entry(2, true),
                    uniform_entry(3),
                    storage_entry(4, false),
                ],
            })
        }
        ResidentStorageProfile::Quantized => {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: wgpu_label("gsplat-resident-quantized-project-bgl"),
                entries: &[
                    storage_entry(0, true),
                    storage_entry(1, true),
                    storage_entry(2, true),
                    uniform_entry(3),
                    storage_entry(4, false),
                    storage_entry(5, true),
                    storage_entry(6, true),
                    storage_entry(7, true),
                ],
            })
        }
    }
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub(crate) fn create_project_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    shader_source: &str,
) -> wgpu::ComputePipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: wgpu_label("gsplat-resident-project-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader_source)),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: wgpu_label("gsplat-resident-project-pipeline-layout"),
        bind_group_layouts: &[layout],
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: wgpu_label("gsplat-resident-project-pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("cs_main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

pub(crate) struct ProjectBindGroupBuffers<'a> {
    pub(crate) order: &'a wgpu::Buffer,
    pub(crate) source: &'a wgpu::Buffer,
    pub(crate) sh: ProjectShBindings<'a>,
    pub(crate) params: &'a wgpu::Buffer,
    pub(crate) projected: &'a wgpu::Buffer,
}

pub(crate) enum ProjectShBindings<'a> {
    FullF32 {
        rest: &'a wgpu::Buffer,
    },
    Quantized {
        sh1: &'a wgpu::Buffer,
        sh2: &'a wgpu::Buffer,
        sh3: &'a wgpu::Buffer,
        sh4: &'a wgpu::Buffer,
    },
}

pub(crate) fn create_project_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    buffers: ProjectBindGroupBuffers<'_>,
) -> wgpu::BindGroup {
    match buffers.sh {
        ProjectShBindings::FullF32 { rest } => {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: wgpu_label(label),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffers.order.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: buffers.source.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: rest.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: buffers.params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: buffers.projected.as_entire_binding(),
                    },
                ],
            })
        }
        ProjectShBindings::Quantized { sh1, sh2, sh3, sh4 } => {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: wgpu_label(label),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffers.order.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: buffers.source.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: sh1.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: buffers.params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: buffers.projected.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: sh2.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: sh3.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: sh4.as_entire_binding(),
                    },
                ],
            })
        }
    }
}

pub(crate) fn create_draw_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    projected_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("gsplat-resident-draw-bind-group"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: projected_buffer.as_entire_binding(),
        }],
    })
}

pub(crate) fn encode_project(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    instance_count: u32,
) {
    if instance_count == 0 {
        return;
    }
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: wgpu_label("gsplat-resident-project-pass"),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.dispatch_workgroups(project_workgroup_count(instance_count), 1, 1);
}

pub(crate) fn create_draw_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label("gsplat-resident-draw-bgl"),
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
    })
}

#[cfg(test)]
mod tests {
    use super::{GpuProjectedRecord, PROJECTED_RECORD_STRIDE};

    #[test]
    fn projected_record_matches_wgsl_stride() {
        assert_eq!(std::mem::size_of::<GpuProjectedRecord>(), 48);
        assert_eq!(PROJECTED_RECORD_STRIDE, 48);
    }
}
