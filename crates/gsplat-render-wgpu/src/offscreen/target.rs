use crate::{RENDER_TARGET_FORMAT, RendererError, wgpu_label};

pub(crate) struct OffscreenTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
}

impl OffscreenTarget {
    pub(crate) fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        max_texture_dimension_2d: u32,
    ) -> Result<Self, RendererError> {
        let (texture, view) =
            create_output_target(device, width, height, max_texture_dimension_2d)?;
        Ok(Self {
            texture,
            view,
            size: (width, height),
        })
    }

    pub(crate) const fn size(&self) -> (u32, u32) {
        self.size
    }

    pub(crate) const fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub(crate) fn ensure_size(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        max_texture_dimension_2d: u32,
    ) -> Result<(), RendererError> {
        if self.size == (width, height) {
            return Ok(());
        }

        let replacement = Self::new(device, width, height, max_texture_dimension_2d)?;
        *self = replacement;
        Ok(())
    }
}

fn create_output_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    max_texture_dimension_2d: u32,
) -> Result<(wgpu::Texture, wgpu::TextureView), RendererError> {
    if width > max_texture_dimension_2d || height > max_texture_dimension_2d {
        return Err(RendererError::GpuDimensionsUnsupported {
            width,
            height,
            max_dimension: max_texture_dimension_2d,
        });
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
