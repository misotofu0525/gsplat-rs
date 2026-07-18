//! Shared SortedAlpha draw-pass encoding for Surface and offscreen targets.

use crate::wgpu_label;

pub(crate) struct SplatDraw<'a> {
    pub(crate) encoder_label: &'static str,
    pub(crate) pass_label: &'static str,
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) pipeline: &'a wgpu::RenderPipeline,
    pub(crate) bind_group: &'a wgpu::BindGroup,
    pub(crate) clear: wgpu::Color,
    pub(crate) vertex_count: u32,
    pub(crate) instance_count: u32,
}

pub(crate) fn encode_splat_draw(device: &wgpu::Device, draw: SplatDraw<'_>) -> wgpu::CommandBuffer {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: wgpu_label(draw.encoder_label),
    });
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
        if draw.instance_count > 0 {
            pass.draw(0..draw.vertex_count, 0..draw.instance_count);
        }
    }
    encoder.finish()
}
