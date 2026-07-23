use std::sync::mpsc;

use crate::{RendererError, offscreen::OffscreenTarget, wgpu_label};

pub(crate) fn readback_rgba8(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &OffscreenTarget,
) -> Result<Vec<u8>, RendererError> {
    let (width, height) = target.size();
    if width == 0 || height == 0 {
        return Err(RendererError::InvalidConfig);
    }

    let bytes_per_pixel = 4_u32;
    let unpadded_bytes_per_row = width.saturating_mul(bytes_per_pixel);
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align).saturating_mul(align);
    let buffer_size = padded_bytes_per_row as u64 * height as u64;

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: wgpu_label("splat-readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: wgpu_label("splat-readback-encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: target.texture(),
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
        queue.submit(Some(encoder.finish()));
    }

    let slice = readback.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());

    match rx.recv() {
        Ok(Ok(())) => {}
        _ => return Err(RendererError::GpuReadback),
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
