use super::SurfaceConfigurationOwner;
use crate::SurfacePresenterError;

/// Acquisition and presentation state for one WGPU Surface host.
///
/// Configuration admission and recovery actions stay delegated to the
/// configuration owner.
pub(crate) struct SurfaceLifecycle {
    last_frame_presented: bool,
    last_presented_size: Option<(u32, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SurfaceAcquireAttempt {
    Initial,
    Retry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SurfaceAcquireDecision {
    ReconfigureAndRetry,
    Unavailable,
    Fail,
}

impl SurfaceLifecycle {
    pub(crate) const fn new() -> Self {
        Self {
            last_frame_presented: false,
            last_presented_size: None,
        }
    }

    pub(crate) fn begin_frame(&mut self) {
        self.last_frame_presented = false;
        self.last_presented_size = None;
    }

    pub(crate) const fn last_frame_presented(&self) -> bool {
        self.last_frame_presented
    }

    pub(crate) const fn last_presented_size(&self) -> Option<(u32, u32)> {
        self.last_presented_size
    }

    pub(crate) fn acquire(
        &self,
        surface: &wgpu::Surface<'_>,
        device: &wgpu::Device,
        configuration: &SurfaceConfigurationOwner,
    ) -> Result<Option<wgpu::SurfaceTexture>, SurfacePresenterError> {
        configuration.ensure_valid()?;

        let mut attempt = SurfaceAcquireAttempt::Initial;
        loop {
            match surface.get_current_texture() {
                Ok(frame) => return Ok(Some(frame)),
                Err(error) => match surface_acquire_decision(&error, attempt) {
                    SurfaceAcquireDecision::ReconfigureAndRetry => {
                        configuration.reconfigure_current(surface, device);
                        attempt = SurfaceAcquireAttempt::Retry;
                    }
                    SurfaceAcquireDecision::Unavailable => return Ok(None),
                    SurfaceAcquireDecision::Fail => {
                        return Err(surface_error_to_presenter(error));
                    }
                },
            }
        }
    }

    pub(crate) fn present(&mut self, frame: wgpu::SurfaceTexture) -> (u32, u32) {
        let size = (frame.texture.width(), frame.texture.height());
        self.present_with(size, || frame.present())
    }

    fn present_with(&mut self, size: (u32, u32), present: impl FnOnce()) -> (u32, u32) {
        self.last_presented_size = Some(size);
        present();
        self.last_frame_presented = true;
        size
    }
}

fn surface_acquire_decision(
    error: &wgpu::SurfaceError,
    attempt: SurfaceAcquireAttempt,
) -> SurfaceAcquireDecision {
    match (attempt, error) {
        (_, wgpu::SurfaceError::Timeout) => SurfaceAcquireDecision::Unavailable,
        (
            SurfaceAcquireAttempt::Initial,
            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated,
        ) => SurfaceAcquireDecision::ReconfigureAndRetry,
        (_, _) => SurfaceAcquireDecision::Fail,
    }
}

fn surface_error_to_presenter(error: wgpu::SurfaceError) -> SurfacePresenterError {
    match error {
        wgpu::SurfaceError::OutOfMemory => SurfacePresenterError::SurfaceOutOfMemory,
        other => SurfacePresenterError::SurfaceAcquire(format!("{other:?}")),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities(present_modes: Vec<wgpu::PresentMode>) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            present_modes,
            ..Default::default()
        }
    }

    #[test]
    fn present_mode_priority_is_mailbox_then_fifo_then_first_then_fifo() {
        assert_eq!(
            select_present_mode(&capabilities(vec![
                wgpu::PresentMode::Fifo,
                wgpu::PresentMode::Mailbox,
            ])),
            wgpu::PresentMode::Mailbox
        );
        assert_eq!(
            select_present_mode(&capabilities(vec![
                wgpu::PresentMode::Immediate,
                wgpu::PresentMode::Fifo,
            ])),
            wgpu::PresentMode::Fifo
        );
        assert_eq!(
            select_present_mode(&capabilities(vec![wgpu::PresentMode::Immediate])),
            wgpu::PresentMode::Immediate
        );
        assert_eq!(
            select_present_mode(&capabilities(Vec::new())),
            wgpu::PresentMode::Fifo
        );
    }

    #[test]
    fn frame_attempt_and_present_receipt_are_owned_by_lifecycle() {
        let mut lifecycle = SurfaceLifecycle::new();
        assert!(!lifecycle.last_frame_presented());
        assert_eq!(lifecycle.last_presented_size(), None);

        let mut primitive_presented = false;
        assert_eq!(
            lifecycle.present_with((2412, 1080), || primitive_presented = true),
            (2412, 1080)
        );
        assert!(primitive_presented);
        assert!(lifecycle.last_frame_presented());
        assert_eq!(lifecycle.last_presented_size(), Some((2412, 1080)));

        lifecycle.begin_frame();
        assert!(!lifecycle.last_frame_presented());
        assert_eq!(lifecycle.last_presented_size(), None);
    }

    #[test]
    fn surface_error_and_retry_decisions_match_the_existing_protocol() {
        for error in [wgpu::SurfaceError::Lost, wgpu::SurfaceError::Outdated] {
            assert_eq!(
                surface_acquire_decision(&error, SurfaceAcquireAttempt::Initial),
                SurfaceAcquireDecision::ReconfigureAndRetry
            );
            assert_eq!(
                surface_acquire_decision(&error, SurfaceAcquireAttempt::Retry),
                SurfaceAcquireDecision::Fail
            );
        }
        assert_eq!(
            surface_acquire_decision(&wgpu::SurfaceError::Timeout, SurfaceAcquireAttempt::Initial),
            SurfaceAcquireDecision::Unavailable
        );
        assert_eq!(
            surface_acquire_decision(&wgpu::SurfaceError::Timeout, SurfaceAcquireAttempt::Retry),
            SurfaceAcquireDecision::Unavailable
        );
        for error in [wgpu::SurfaceError::OutOfMemory, wgpu::SurfaceError::Other] {
            assert_eq!(
                surface_acquire_decision(&error, SurfaceAcquireAttempt::Initial),
                SurfaceAcquireDecision::Fail
            );
            assert_eq!(
                surface_acquire_decision(&error, SurfaceAcquireAttempt::Retry),
                SurfaceAcquireDecision::Fail
            );
        }

        assert!(matches!(
            surface_error_to_presenter(wgpu::SurfaceError::OutOfMemory),
            SurfacePresenterError::SurfaceOutOfMemory
        ));
        assert!(matches!(
            surface_error_to_presenter(wgpu::SurfaceError::Lost),
            SurfacePresenterError::SurfaceAcquire(ref message) if message == "Lost"
        ));
        assert!(matches!(
            surface_error_to_presenter(wgpu::SurfaceError::Outdated),
            SurfacePresenterError::SurfaceAcquire(ref message) if message == "Outdated"
        ));
        assert!(matches!(
            surface_error_to_presenter(wgpu::SurfaceError::Other),
            SurfacePresenterError::SurfaceAcquire(ref message) if message == "Other"
        ));
    }

    #[test]
    fn lifecycle_has_no_raw_surface_configuration_or_configure_action() {
        let source = include_str!("lifecycle.rs");
        let raw_configuration_type = ["wgpu::Surface", "Configuration"].concat();
        let raw_configure_call = [".", "configure("].concat();
        assert!(!source.contains(&raw_configuration_type));
        assert!(!source.contains(&raw_configure_call));
    }
}
