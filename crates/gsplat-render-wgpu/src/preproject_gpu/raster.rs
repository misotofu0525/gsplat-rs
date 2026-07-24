use crate::raster::{SplatPipeline, create_splat_pipeline};
use crate::resident_gpu::ResidentGpuResources;
use crate::wgpu_label;

use super::{PreprojectedGpuCompute, entry, storage_layout};

/// Thin target-format adapter retained solely for the legacy Surface path.
/// Exact projection, compaction, ordering and count ownership stay in the
/// target-independent [`PreprojectedGpuCompute`].
pub(super) struct PreprojectedGpuRaster {
    draw_pipeline: wgpu::RenderPipeline,
    draw_bind_group: wgpu::BindGroup,
}

impl PreprojectedGpuRaster {
    pub(super) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        resident: &ResidentGpuResources,
        compute: &PreprojectedGpuCompute,
    ) -> Self {
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-preproject-draw-bgl"),
            entries: &[
                storage_layout(0, true, wgpu::ShaderStages::VERTEX),
                storage_layout(1, true, wgpu::ShaderStages::VERTEX),
                storage_layout(2, true, wgpu::ShaderStages::VERTEX),
                storage_layout(3, true, wgpu::ShaderStages::VERTEX),
            ],
        });
        let draw_pipeline = create_splat_pipeline(
            device,
            &draw_layout,
            target_format,
            SplatPipeline {
                shader_label: "gsplat-preproject-draw-shader",
                shader_source: include_str!("../../shaders/preproject_draw.wgsl"),
                layout_label: "gsplat-preproject-draw-pipeline-layout",
                pipeline_label: "gsplat-preproject-draw-pipeline",
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
        );
        let draw_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-preproject-draw-bg"),
            layout: &draw_layout,
            entries: &[
                entry(0, compute.final_source_ids()),
                entry(1, compute.source_center_alpha_key()),
                entry(2, compute.source_axes()),
                entry(3, &resident.resolved_color_buffer),
            ],
        });
        Self {
            draw_pipeline,
            draw_bind_group,
        }
    }

    pub(super) fn draw_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.draw_pipeline
    }

    pub(super) fn draw_bind_group(&self) -> &wgpu::BindGroup {
        &self.draw_bind_group
    }
}
