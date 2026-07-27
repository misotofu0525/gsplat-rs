use crate::{SurfacePresenterError, wgpu_label};

/// Exact renderer-owned presentation target captured immediately before its
/// successful Surface presentation.
///
/// Capture is a diagnostic-only opt-in. Ordinary Surface presenters never ask
/// the swapchain for copy usage and do not allocate a readback buffer.
/// This mechanical readback DTO deliberately carries no renderer profile or
/// frame identity; `SurfaceRenderSession` owns that presentation-sequence join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceFrameCapture {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

pub(crate) struct SurfaceCapture {
    copy_src_supported: bool,
    pending: Option<PendingSurfaceCapture>,
}

pub(crate) struct PreparedSurfaceCapture {
    pending: PendingSurfaceCapture,
}

struct PendingSurfaceCapture {
    buffer: wgpu::Buffer,
    source: CaptureSource,
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,
    format: wgpu::TextureFormat,
    progress: CaptureProgress,
}

enum CaptureSource {
    Surface,
    Intermediate {
        texture: wgpu::Texture,
        present_pipeline: wgpu::RenderPipeline,
        present_bind_group: wgpu::BindGroup,
    },
}

#[derive(Default)]
struct CaptureProgress {
    encoded: bool,
    presented: bool,
}

impl CaptureProgress {
    const fn copy_required(&self) -> bool {
        !self.presented
    }

    fn mark_encoded(&mut self) {
        self.encoded = true;
    }

    fn mark_presented(&mut self) {
        if self.encoded {
            self.presented = true;
        }
    }

    #[cfg(any(
        not(target_arch = "wasm32"),
        feature = "diagnostic-surface-capture-receipt"
    ))]
    const fn ready(&self) -> bool {
        self.encoded && self.presented
    }
}

impl SurfaceCapture {
    pub(crate) const fn new(copy_src_supported: bool) -> Self {
        Self {
            copy_src_supported,
            pending: None,
        }
    }

    pub(crate) const fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn render_target_texture<'a>(
        &'a self,
        surface_texture: &'a wgpu::Texture,
    ) -> &'a wgpu::Texture {
        match self.pending.as_ref().map(|pending| &pending.source) {
            Some(CaptureSource::Intermediate { texture, .. }) => texture,
            Some(CaptureSource::Surface) | None => surface_texture,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn prepare_request(
        &self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Result<PreparedSurfaceCapture, SurfacePresenterError> {
        pollster::block_on(self.prepare_request_async(device, width, height, format))
    }

    /// Prepares one unpublished readback without blocking the browser event
    /// loop. The allocation and its error scopes complete before the Surface
    /// can be reconfigured or the capture can be published.
    pub(crate) async fn prepare_request_async(
        &self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Result<PreparedSurfaceCapture, SurfacePresenterError> {
        if self.pending.is_some() {
            return Err(SurfacePresenterError::SurfaceCaptureState(
                "a previous capture has not been taken".into(),
            ));
        }
        if !surface_capture_format_supported(format) {
            return Err(SurfacePresenterError::SurfaceCaptureUnsupported(format!(
                "format {:?} is not an RGBA8/BGRA8 format",
                format
            )));
        }
        let (padded_bytes_per_row, buffer_size) = surface_capture_layout(width, height)?;
        validate_surface_capture_buffer_size(buffer_size, device.limits().max_buffer_size)?;

        // Allocate the unpublished readback resource first. Device creation
        // errors are asynchronous on every wgpu backend, so a plain
        // create_buffer call could otherwise turn this Result-returning API
        // into an uncaptured validation/OOM and leave a reconfigured Surface
        // without a usable pending capture.
        let (validation_scope, oom_scope, internal_scope) = (
            device.push_error_scope(wgpu::ErrorFilter::Validation),
            device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-surface-frame-capture"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let source = if self.copy_src_supported {
            CaptureSource::Surface
        } else {
            create_intermediate_capture_source(device, width, height, format)
        };
        let (internal_error, oom_error, validation_error) = (
            internal_scope.pop().await,
            oom_scope.pop().await,
            validation_scope.pop().await,
        );
        if let Some(error) = internal_error.or(oom_error).or(validation_error) {
            return Err(SurfacePresenterError::SurfaceCaptureUnsupported(format!(
                "capture resource allocation failed: {error}"
            )));
        }

        Ok(PreparedSurfaceCapture {
            pending: PendingSurfaceCapture {
                buffer,
                source,
                width,
                height,
                padded_bytes_per_row,
                format,
                progress: CaptureProgress::default(),
            },
        })
    }

    pub(crate) fn publish(&mut self, prepared: PreparedSurfaceCapture) {
        debug_assert!(self.pending.is_none());
        self.pending = Some(prepared.pending);
    }

    pub(crate) const fn prepared_requires_surface_copy_src(
        prepared: &PreparedSurfaceCapture,
    ) -> bool {
        matches!(prepared.pending.source, CaptureSource::Surface)
    }

    pub(crate) fn cancel(&mut self) -> bool {
        self.pending.take().is_some()
    }

    pub(crate) fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture) {
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        if !pending.progress.copy_required() {
            return;
        }
        // The request reconfigures this exact Surface without changing its
        // dimensions or format before any drawable is acquired.
        debug_assert_eq!(
            (texture.width(), texture.height()),
            (pending.width, pending.height)
        );
        debug_assert_eq!(texture.format(), pending.format);
        let source_texture = match &pending.source {
            CaptureSource::Surface => texture,
            CaptureSource::Intermediate { texture, .. } => texture,
        };
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: source_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &pending.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pending.padded_bytes_per_row),
                    rows_per_image: Some(pending.height),
                },
            },
            wgpu::Extent3d {
                width: pending.width,
                height: pending.height,
                depth_or_array_layers: 1,
            },
        );
        if let CaptureSource::Intermediate {
            present_pipeline,
            present_bind_group,
            ..
        } = &pending.source
        {
            let surface_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: wgpu_label("gsplat-surface-capture-present-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(present_pipeline);
            pass.set_bind_group(0, present_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        pending.progress.mark_encoded();
    }

    pub(crate) fn mark_presented(&mut self) {
        if let Some(pending) = self.pending.as_mut() {
            // A prior submitted copy is not publishable until the exact frame
            // carrying the latest copy has reached Surface presentation. If a
            // post-submit diagnostic fails before this point, the next retry
            // encodes another copy because `presented` remains false.
            pending.progress.mark_presented();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn take(
        &mut self,
        device: &wgpu::Device,
    ) -> Result<SurfaceFrameCapture, SurfacePresenterError> {
        use std::sync::mpsc;

        let pending = self.pending.as_ref().ok_or_else(|| {
            SurfacePresenterError::SurfaceCaptureState("no capture was requested".into())
        })?;
        if !pending.progress.ready() {
            return Err(SurfacePresenterError::SurfaceCaptureState(
                "the requested presentation-target copy has not completed a presentation".into(),
            ));
        }
        let pending = self.pending.take().expect("validated pending capture");
        let slice = pending.buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        match rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => return Err(SurfacePresenterError::SurfaceCaptureReadback),
        }
        let mapped = slice.get_mapped_range();
        let rgba8 = unpack_surface_capture_rows(
            &mapped,
            pending.width,
            pending.height,
            pending.padded_bytes_per_row,
            pending.format,
        );
        drop(mapped);
        pending.buffer.unmap();
        let rgba8 = rgba8.map_err(|_| SurfacePresenterError::SurfaceCaptureReadback)?;
        Ok(SurfaceFrameCapture {
            width: pending.width,
            height: pending.height,
            rgba8,
        })
    }

    /// Asynchronously consumes one presented browser capture. The map callback
    /// wakes this future; it never blocks or spins the JavaScript event loop.
    #[cfg(all(target_arch = "wasm32", feature = "diagnostic-surface-capture-receipt"))]
    pub(crate) async fn take_async(
        &mut self,
    ) -> Result<SurfaceFrameCapture, SurfacePresenterError> {
        use std::future::poll_fn;
        use std::sync::{Arc, Mutex};
        use std::task::{Poll, Waker};

        struct MapState {
            result: Option<Result<(), wgpu::BufferAsyncError>>,
            waker: Option<Waker>,
        }

        let pending = self.pending.as_ref().ok_or_else(|| {
            SurfacePresenterError::SurfaceCaptureState("no capture was requested".into())
        })?;
        if !pending.progress.ready() {
            return Err(SurfacePresenterError::SurfaceCaptureState(
                "the requested presentation-target copy has not completed a presentation".into(),
            ));
        }
        let pending = self.pending.take().expect("validated pending capture");
        let slice = pending.buffer.slice(..);
        let state = Arc::new(Mutex::new(MapState {
            result: None,
            waker: None,
        }));
        let callback_state = Arc::clone(&state);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let mut state = callback_state.lock().expect("capture map state poisoned");
            state.result = Some(result);
            if let Some(waker) = state.waker.take() {
                waker.wake();
            }
        });
        poll_fn(|cx| {
            let mut state = state.lock().expect("capture map state poisoned");
            if let Some(result) = state.result.take() {
                Poll::Ready(result)
            } else {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
        .map_err(|_| SurfacePresenterError::SurfaceCaptureReadback)?;

        let mapped = slice.get_mapped_range();
        let rgba8 = unpack_surface_capture_rows(
            &mapped,
            pending.width,
            pending.height,
            pending.padded_bytes_per_row,
            pending.format,
        );
        drop(mapped);
        pending.buffer.unmap();
        let rgba8 = rgba8.map_err(|_| SurfacePresenterError::SurfaceCaptureReadback)?;
        Ok(SurfaceFrameCapture {
            width: pending.width,
            height: pending.height,
            rgba8,
        })
    }
}

fn create_intermediate_capture_source(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> CaptureSource {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: wgpu_label("gsplat-surface-capture-intermediate-target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label("gsplat-surface-capture-present-bind-group-layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: wgpu_label("gsplat-surface-capture-present-shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../../shaders/surface_capture_present.wgsl").into(),
        ),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: wgpu_label("gsplat-surface-capture-present-pipeline-layout"),
        bind_group_layouts: &[&layout],
        immediate_size: 0,
    });
    let present_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: wgpu_label("gsplat-surface-capture-present-pipeline"),
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
                blend: None,
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
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let present_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("gsplat-surface-capture-present-bind-group"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    });
    CaptureSource::Intermediate {
        texture,
        present_pipeline,
        present_bind_group,
    }
}

fn surface_capture_format_supported(format: wgpu::TextureFormat) -> bool {
    matches!(
        format,
        wgpu::TextureFormat::Rgba8Unorm
            | wgpu::TextureFormat::Rgba8UnormSrgb
            | wgpu::TextureFormat::Bgra8Unorm
            | wgpu::TextureFormat::Bgra8UnormSrgb
    )
}

fn surface_capture_layout(width: u32, height: u32) -> Result<(u32, u64), SurfacePresenterError> {
    let unpadded = width.checked_mul(4).ok_or_else(|| {
        SurfacePresenterError::SurfaceCaptureUnsupported("row byte size overflow".into())
    })?;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded
        .checked_add(align - 1)
        .map(|value| value / align * align)
        .ok_or_else(|| {
            SurfacePresenterError::SurfaceCaptureUnsupported("aligned row size overflow".into())
        })?;
    let size = u64::from(padded)
        .checked_mul(u64::from(height))
        .ok_or_else(|| {
            SurfacePresenterError::SurfaceCaptureUnsupported("readback size overflow".into())
        })?;
    if size == 0 {
        return Err(SurfacePresenterError::SurfaceCaptureUnsupported(
            "capture dimensions must be non-zero".into(),
        ));
    }
    Ok((padded, size))
}

fn validate_surface_capture_buffer_size(
    buffer_size: u64,
    max_buffer_size: u64,
) -> Result<(), SurfacePresenterError> {
    if buffer_size > max_buffer_size {
        return Err(SurfacePresenterError::SurfaceCaptureUnsupported(format!(
            "readback buffer requires {buffer_size} bytes but the device limit is {max_buffer_size}"
        )));
    }
    Ok(())
}

#[cfg(any(
    not(target_arch = "wasm32"),
    feature = "diagnostic-surface-capture-receipt"
))]
fn unpack_surface_capture_rows(
    mapped: &[u8],
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,
    format: wgpu::TextureFormat,
) -> Result<Vec<u8>, SurfacePresenterError> {
    if !surface_capture_format_supported(format) {
        return Err(SurfacePresenterError::SurfaceCaptureUnsupported(format!(
            "format {format:?} cannot be converted to RGBA8"
        )));
    }
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| {
            SurfacePresenterError::SurfaceCaptureState("RGBA row size overflow".into())
        })?;
    let padded = usize::try_from(padded_bytes_per_row).map_err(|_| {
        SurfacePresenterError::SurfaceCaptureState("padded row size overflow".into())
    })?;
    let height = usize::try_from(height).map_err(|_| {
        SurfacePresenterError::SurfaceCaptureState("capture height overflow".into())
    })?;
    let output_len = row_bytes.checked_mul(height).ok_or_else(|| {
        SurfacePresenterError::SurfaceCaptureState("RGBA output size overflow".into())
    })?;
    let required = padded
        .checked_mul(height)
        .ok_or_else(|| SurfacePresenterError::SurfaceCaptureState("mapped size overflow".into()))?;
    if mapped.len() < required || padded < row_bytes {
        return Err(SurfacePresenterError::SurfaceCaptureState(format!(
            "mapped readback has {} bytes, expected at least {required}",
            mapped.len()
        )));
    }
    let mut rgba8 = vec![0_u8; output_len];
    for row in 0..height {
        let source = &mapped[row * padded..row * padded + row_bytes];
        let target = &mut rgba8[row * row_bytes..(row + 1) * row_bytes];
        target.copy_from_slice(source);
    }
    if matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        for pixel in rgba8.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }
    Ok(rgba8)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn capture_layout_obeys_texture_copy_row_alignment() {
        let (row, size) = surface_capture_layout(3, 2).expect("layout");
        assert_eq!(row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        assert_eq!(size, u64::from(row) * 2);
        assert!(surface_capture_layout(0, 1).is_err());
        assert!(surface_capture_layout(1, 0).is_err());
    }

    #[test]
    fn capture_layout_rejects_row_and_alignment_overflow() {
        assert!(matches!(
            surface_capture_layout(u32::MAX, 1),
            Err(SurfacePresenterError::SurfaceCaptureUnsupported(message))
                if message == "row byte size overflow"
        ));
        assert!(matches!(
            surface_capture_layout(u32::MAX / 4, 1),
            Err(SurfacePresenterError::SurfaceCaptureUnsupported(message))
                if message == "aligned row size overflow"
        ));
    }

    #[test]
    fn capture_buffer_limit_is_inclusive() {
        let (_, size) = surface_capture_layout(3, 2).expect("layout");
        assert!(validate_surface_capture_buffer_size(size, size).is_ok());
        assert!(matches!(
            validate_surface_capture_buffer_size(size, size - 1),
            Err(SurfacePresenterError::SurfaceCaptureUnsupported(message))
                if message.contains("device limit")
        ));
    }

    #[test]
    fn capture_unpack_removes_padding_and_canonicalizes_bgra() {
        let padded = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let mut mapped = vec![0_u8; padded as usize * 2];
        mapped[0..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let second = padded as usize;
        mapped[second..second + 8].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        let rgba =
            unpack_surface_capture_rows(&mapped, 2, 2, padded, wgpu::TextureFormat::Bgra8UnormSrgb)
                .expect("unpack");
        assert_eq!(
            rgba,
            [3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]
        );
    }

    #[test]
    fn capture_unpack_preserves_rgba_and_rejects_invalid_input() {
        let mapped = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            unpack_surface_capture_rows(&mapped, 2, 1, 8, wgpu::TextureFormat::Rgba8Unorm)
                .expect("RGBA unpack"),
            mapped
        );
        assert!(matches!(
            unpack_surface_capture_rows(&mapped[..7], 2, 1, 8, wgpu::TextureFormat::Rgba8Unorm),
            Err(SurfacePresenterError::SurfaceCaptureState(message))
                if message == "mapped readback has 7 bytes, expected at least 8"
        ));
        assert!(matches!(
            unpack_surface_capture_rows(&mapped, 2, 1, 8, wgpu::TextureFormat::Rgba16Float),
            Err(SurfacePresenterError::SurfaceCaptureUnsupported(message))
                if message == "format Rgba16Float cannot be converted to RGBA8"
        ));
    }

    #[test]
    fn capture_progress_requires_copy_then_successful_present() {
        let mut progress = CaptureProgress::default();
        assert!(progress.copy_required());
        assert!(!progress.ready());

        progress.mark_presented();
        assert!(progress.copy_required());
        assert!(!progress.ready());

        progress.mark_encoded();
        assert!(progress.copy_required());
        assert!(!progress.ready());

        progress.mark_encoded();
        assert!(progress.copy_required());
        progress.mark_presented();
        assert!(!progress.copy_required());
        assert!(progress.ready());
    }
}
