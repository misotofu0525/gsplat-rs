//! Strategy-free Resident color-kernel mechanics.
//!
//! Callers retain scene resources and camera-change policy. This leaf owns the
//! fixed color parameter ABI, pipeline factories, parameter upload, portable
//! two-dimensional dispatch, and compute-pass encoding.

use std::mem::size_of;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::gpu_error::ResidentGpuError;
use crate::wgpu_label;

const COLOR_WORKGROUP_SIZE: u32 = 128;
const COLOR_PARAMS_LABEL: &str = "gsplat-resident-color-params";
const COLOR_BIND_GROUP_LAYOUT_LABEL: &str = "gsplat-resident-color-bgl";
const COLOR_SHADER_LABEL: &str = "gsplat-resident-color-shader";
const COLOR_PIPELINE_LAYOUT_LABEL: &str = "gsplat-resident-color-pipeline-layout";
const COLOR_PIPELINE_LABEL: &str = "gsplat-resident-color-pipeline";
const COLOR_PASS_LABEL: &str = "gsplat-resident-color-resolve-pass";
const COLOR_ENTRY_POINT: &str = "main";
const COLOR_SHADER_SOURCE: &str = include_str!("../../shaders/resident_color_resolve.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuResidentColorParams {
    camera_pos: [f32; 4],
    len: u32,
    sh_degree: u32,
    _reserved0: u32,
    _pad: u32,
}

const _: [(); 32] = [(); size_of::<GpuResidentColorParams>()];

pub(crate) struct ResidentColorKernel<'a> {
    pub(crate) pipeline: &'a wgpu::ComputePipeline,
    pub(crate) bind_group: &'a wgpu::BindGroup,
    pub(crate) params_buffer: &'a wgpu::Buffer,
    pub(crate) splat_count: usize,
    pub(crate) sh_degree: u32,
    pub(crate) max_workgroups_per_dimension: u32,
}

impl ResidentColorKernel<'_> {
    pub(crate) fn encode(
        self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        camera_position: [f32; 3],
    ) -> Result<(), ResidentGpuError> {
        let params = GpuResidentColorParams {
            camera_pos: [
                camera_position[0],
                camera_position[1],
                camera_position[2],
                0.0,
            ],
            len: u32::try_from(self.splat_count)
                .map_err(|_| ResidentGpuError::AddressSpaceExceeded)?,
            sh_degree: self.sh_degree,
            _reserved0: 0,
            _pad: 0,
        };
        queue.write_buffer(self.params_buffer, 0, bytemuck::bytes_of(&params));
        let (groups_x, groups_y) = dispatch_2d(
            self.splat_count.div_ceil(COLOR_WORKGROUP_SIZE as usize),
            self.max_workgroups_per_dimension,
        )?;
        if self.splat_count > 0 {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: wgpu_label(COLOR_PASS_LABEL),
                timestamp_writes: None,
            });
            pass.set_pipeline(self.pipeline);
            pass.set_bind_group(0, self.bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        Ok(())
    }
}

pub(crate) fn create_resident_color_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: wgpu_label(COLOR_PARAMS_LABEL),
        contents: bytemuck::bytes_of(&GpuResidentColorParams::zeroed()),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

pub(crate) fn create_resident_color_bind_group_layout(
    device: &wgpu::Device,
) -> wgpu::BindGroupLayout {
    let entries = color_bind_group_layout_entries();
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label(COLOR_BIND_GROUP_LAYOUT_LABEL),
        entries: &entries,
    })
}

pub(crate) fn create_resident_color_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::ComputePipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: wgpu_label(COLOR_SHADER_LABEL),
        source: wgpu::ShaderSource::Wgsl(COLOR_SHADER_SOURCE.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: wgpu_label(COLOR_PIPELINE_LAYOUT_LABEL),
        bind_group_layouts: &[bind_group_layout],
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: wgpu_label(COLOR_PIPELINE_LABEL),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some(COLOR_ENTRY_POINT),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

fn color_bind_group_layout_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
    let mut entries = Vec::with_capacity(9);
    for binding in 0..8 {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: binding != 7,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: 8,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    });
    entries
}

fn dispatch_2d(groups: usize, limit: u32) -> Result<(u32, u32), ResidentGpuError> {
    if groups == 0 {
        return Ok((1, 1));
    }
    let limit = limit.max(1) as usize;
    let x = groups.min(limit);
    let y = groups.div_ceil(x);
    if y > limit {
        return Err(ResidentGpuError::DispatchLimitExceeded);
    }
    Ok((x as u32, y as u32))
}

#[cfg(test)]
mod tests {
    use std::mem::offset_of;

    use super::*;

    #[test]
    fn color_parameter_abi_matches_wgsl_exactly() {
        assert_eq!(size_of::<GpuResidentColorParams>(), 32);
        assert_eq!(offset_of!(GpuResidentColorParams, camera_pos), 0);
        assert_eq!(offset_of!(GpuResidentColorParams, len), 16);
        assert_eq!(offset_of!(GpuResidentColorParams, sh_degree), 20);
        assert_eq!(offset_of!(GpuResidentColorParams, _reserved0), 24);
        assert_eq!(offset_of!(GpuResidentColorParams, _pad), 28);
    }

    #[test]
    fn color_layout_binding_inventory_is_exact() {
        let entries = color_bind_group_layout_entries();
        assert_eq!(entries.len(), 9);
        for (binding, entry) in entries.iter().enumerate() {
            assert_eq!(entry.binding, binding as u32);
            assert_eq!(entry.visibility, wgpu::ShaderStages::COMPUTE);
            assert!(entry.count.is_none());
            let wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset,
                min_binding_size,
            } = entry.ty
            else {
                panic!("Resident color binding {binding} must remain a buffer");
            };
            assert!(!has_dynamic_offset);
            assert!(min_binding_size.is_none());
            if binding < 8 {
                assert_eq!(
                    ty,
                    wgpu::BufferBindingType::Storage {
                        read_only: binding != 7,
                    }
                );
            } else {
                assert_eq!(ty, wgpu::BufferBindingType::Uniform);
            }
        }
    }

    #[test]
    fn color_shader_entry_binding_and_dispatch_inventory_is_exact() {
        assert_eq!(COLOR_ENTRY_POINT, "main");
        assert_eq!(COLOR_WORKGROUP_SIZE, 128);
        assert_eq!(
            COLOR_SHADER_SOURCE.matches("@group(0) @binding(").count(),
            9
        );
        for binding in 0..=8 {
            assert!(
                COLOR_SHADER_SOURCE.contains(&format!("@group(0) @binding({binding})")),
                "missing Resident color binding {binding}"
            );
        }
        assert!(COLOR_SHADER_SOURCE.contains("@compute @workgroup_size(128)"));
        assert!(COLOR_SHADER_SOURCE.contains("fn main("));
        assert!(COLOR_SHADER_SOURCE.contains("camera_pos: vec4<f32>"));
        assert!(COLOR_SHADER_SOURCE.contains("sh_degree: u32"));
    }

    #[test]
    fn color_labels_remain_compatible() {
        assert_eq!(
            [
                COLOR_PARAMS_LABEL,
                COLOR_BIND_GROUP_LAYOUT_LABEL,
                COLOR_SHADER_LABEL,
                COLOR_PIPELINE_LAYOUT_LABEL,
                COLOR_PIPELINE_LABEL,
                COLOR_PASS_LABEL,
            ],
            [
                "gsplat-resident-color-params",
                "gsplat-resident-color-bgl",
                "gsplat-resident-color-shader",
                "gsplat-resident-color-pipeline-layout",
                "gsplat-resident-color-pipeline",
                "gsplat-resident-color-resolve-pass",
            ]
        );
    }

    #[test]
    fn dispatch_flattens_across_two_dimensions() {
        assert_eq!(dispatch_2d(0, 7).unwrap(), (1, 1));
        assert_eq!(dispatch_2d(7, 7).unwrap(), (7, 1));
        assert_eq!(dispatch_2d(8, 7).unwrap(), (7, 2));
        assert_eq!(dispatch_2d(49, 7).unwrap(), (7, 7));
        assert!(matches!(
            dispatch_2d(50, 7),
            Err(ResidentGpuError::DispatchLimitExceeded)
        ));
    }
}
