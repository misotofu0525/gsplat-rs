//! Shared SortedAlpha draw-pass encoding for Surface and offscreen targets.

use crate::wgpu_label;

pub(crate) struct SplatPipeline {
    pub(crate) shader_label: &'static str,
    pub(crate) shader_source: &'static str,
    pub(crate) layout_label: &'static str,
    pub(crate) pipeline_label: &'static str,
    pub(crate) topology: wgpu::PrimitiveTopology,
}

pub(crate) fn create_splat_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    splat: SplatPipeline,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: wgpu_label(splat.shader_label),
        source: wgpu::ShaderSource::Wgsl(splat.shader_source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: wgpu_label(splat.layout_label),
        bind_group_layouts: &[bind_group_layout],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: wgpu_label(splat.pipeline_label),
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
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: splat.topology,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

pub(crate) struct SplatDraw<'a> {
    pub(crate) pass_label: &'static str,
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) pipeline: &'a wgpu::RenderPipeline,
    pub(crate) bind_group: &'a wgpu::BindGroup,
    pub(crate) clear: wgpu::Color,
    pub(crate) vertex_count: u32,
    pub(crate) instance_count: u32,
    pub(crate) indirect: Option<(&'a wgpu::Buffer, u64)>,
}

pub(crate) fn encode_splat_draw_into(encoder: &mut wgpu::CommandEncoder, draw: &SplatDraw<'_>) {
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: wgpu_label(draw.pass_label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: draw.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(draw.clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(draw.pipeline);
        pass.set_bind_group(0, draw.bind_group, &[]);
        if let Some((buffer, offset)) = draw.indirect {
            pass.draw_indirect(buffer, offset);
        } else if draw.instance_count > 0 {
            pass.draw(0..draw.vertex_count, 0..draw.instance_count);
        }
    }
}
