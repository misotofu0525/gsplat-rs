//! Surface instance / present-mode helpers used by SurfacePresenter.

use crate::error::SurfacePresenterError;

pub(crate) fn select_present_mode(caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        return wgpu::PresentMode::Mailbox;
    }
    if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
        return wgpu::PresentMode::Fifo;
    }

    caps.present_modes
        .first()
        .copied()
        .unwrap_or(wgpu::PresentMode::Fifo)
}

pub(crate) fn surface_error_to_presenter(err: wgpu::SurfaceError) -> SurfacePresenterError {
    match err {
        wgpu::SurfaceError::OutOfMemory => SurfacePresenterError::SurfaceOutOfMemory,
        other => SurfacePresenterError::SurfaceAcquire(format!("{other:?}")),
    }
}

pub(crate) fn fit_surface_size(width: u32, height: u32, max_dimension: u32) -> (u32, u32) {
    let max_input_dimension = width.max(height).max(1);
    if max_input_dimension <= max_dimension {
        return (width, height);
    }

    let scale = max_dimension as f32 / max_input_dimension as f32;
    let scaled_width = ((width as f32) * scale).round() as u32;
    let scaled_height = ((height as f32) * scale).round() as u32;
    (scaled_width.max(1), scaled_height.max(1))
}

#[cfg(target_os = "android")]
pub(crate) fn create_surface_instance() -> wgpu::Instance {
    wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        flags: wgpu::InstanceFlags::empty(),
        ..Default::default()
    })
}

#[cfg(not(target_os = "android"))]
pub(crate) fn create_surface_instance() -> wgpu::Instance {
    wgpu::Instance::default()
}
