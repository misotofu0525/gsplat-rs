use crate::SurfacePresenterError;

/// Select an attachment that preserves the trained image-domain splat colors.
///
/// Resident/Direct SH evaluation produces the same display-encoded RGB values
/// as the source training images and the reference raster. An sRGB attachment
/// would encode those values a second time before presentation. Keep adapter
/// order within the portable 8-bit non-sRGB formats, and fail closed instead
/// of silently changing the release-gated image contract.
pub(crate) fn select_splat_surface_format(
    formats: &[wgpu::TextureFormat],
) -> Option<wgpu::TextureFormat> {
    formats.iter().copied().find(|format| {
        matches!(
            format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
        )
    })
}

/// Published Surface descriptor and its fail-closed reconfiguration state.
pub(crate) struct SurfaceConfigurationOwner {
    config: wgpu::SurfaceConfiguration,
    valid: bool,
    max_texture_dimension_2d: u32,
}

#[derive(Debug)]
struct ConfigurationTransactionFailure<E> {
    configure_error: E,
    rollback_error: Option<E>,
}

struct ConfigureScopeErrors {
    internal: Option<String>,
    out_of_memory: Option<String>,
    validation: Option<String>,
}

impl ConfigureScopeErrors {
    #[cfg(any(target_arch = "wasm32", test))]
    fn resize_error(self) -> Option<SurfacePresenterError> {
        classify_surface_configure_scope_errors(self.internal, self.out_of_memory, self.validation)
    }

    fn capture_error(self) -> Option<String> {
        self.internal.or(self.out_of_memory).or(self.validation)
    }
}

impl SurfaceConfigurationOwner {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn new_configured(
        surface: &wgpu::Surface<'_>,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        present_mode: wgpu::PresentMode,
        alpha_mode: wgpu::CompositeAlphaMode,
        max_texture_dimension_2d: u32,
    ) -> Result<Self, SurfacePresenterError> {
        let config = surface_configuration(format, width, height, present_mode, alpha_mode);
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(device, &config);
        if let Some(error) = error_scope.pop().await {
            return Err(SurfacePresenterError::SurfaceConfigure(error.to_string()));
        }

        Ok(Self {
            config,
            valid: true,
            max_texture_dimension_2d,
        })
    }

    pub(crate) const fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub(crate) const fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub(crate) const fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.config
    }

    pub(crate) fn ensure_valid(&self) -> Result<(), SurfacePresenterError> {
        if self.valid {
            Ok(())
        } else {
            Err(SurfacePresenterError::SurfaceConfigure(
                "surface is fail-closed after a resize rollback failure".into(),
            ))
        }
    }

    pub(crate) fn ensure_capture_valid(&self) -> Result<(), SurfacePresenterError> {
        if self.valid {
            Ok(())
        } else {
            Err(SurfacePresenterError::SurfaceCaptureState(
                "the Surface configuration is invalid".into(),
            ))
        }
    }

    pub(crate) fn reconfigure_current(&self, surface: &wgpu::Surface<'_>, device: &wgpu::Device) {
        surface.configure(device, self.config());
    }

    pub(crate) fn validate_size(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(), SurfacePresenterError> {
        if width == 0 || height == 0 {
            return Err(SurfacePresenterError::InvalidSurfaceSize);
        }
        if width > self.max_texture_dimension_2d || height > self.max_texture_dimension_2d {
            return Err(SurfacePresenterError::GpuDimensionsUnsupported {
                width,
                height,
                max_dimension: self.max_texture_dimension_2d,
            });
        }
        Ok(())
    }

    pub(crate) fn resize_required(&self, width: u32, height: u32) -> bool {
        self.size() != (width, height) || !self.valid
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn resize_native(
        &mut self,
        surface: &wgpu::Surface<'_>,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) {
        let candidate = self.resize_candidate(width, height);
        surface.configure(device, &candidate);
        self.publish(candidate);
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn resize_transactionally(
        &mut self,
        surface: &wgpu::Surface<'_>,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<(), SurfacePresenterError> {
        let previous = self.config.clone();
        let candidate = self.resize_candidate(width, height);
        let resize_error = Self::configure_scoped(surface, device, &candidate)
            .await
            .resize_error();
        if resize_error.is_none() {
            return self
                .finish_transaction::<SurfacePresenterError>(candidate, None, None)
                .map_err(|_| unreachable!("a successful configure cannot fail publication"));
        }

        let rollback_error = Self::configure_scoped(surface, device, &previous)
            .await
            .resize_error();
        self.finish_transaction(candidate, resize_error, rollback_error)
            .map_err(resize_transaction_failure_to_error)
    }

    pub(crate) async fn ensure_copy_src(
        &mut self,
        surface: &wgpu::Surface<'_>,
        device: &wgpu::Device,
    ) -> Result<(), SurfacePresenterError> {
        if self.config.usage.contains(wgpu::TextureUsages::COPY_SRC) {
            return Ok(());
        }

        let previous = self.config.clone();
        let candidate = self.copy_src_candidate();
        let configure_error = Self::configure_scoped(surface, device, &candidate)
            .await
            .capture_error();
        if configure_error.is_none() {
            return self
                .finish_transaction::<String>(candidate, None, None)
                .map_err(|_| unreachable!("a successful configure cannot fail publication"));
        }

        let rollback_error = Self::configure_scoped(surface, device, &previous)
            .await
            .capture_error();
        self.finish_transaction(candidate, configure_error, rollback_error)
            .map_err(copy_src_transaction_failure_to_error)
    }

    pub(crate) fn update_frame_latency(
        &mut self,
        surface: &wgpu::Surface<'_>,
        device: &wgpu::Device,
        latency: u32,
    ) -> bool {
        let latency = latency.clamp(1, 4);
        if self.config.desired_maximum_frame_latency == latency {
            return false;
        }

        let candidate = self.frame_latency_candidate(latency);
        surface.configure(device, &candidate);
        self.config = candidate;
        true
    }

    fn resize_candidate(&self, width: u32, height: u32) -> wgpu::SurfaceConfiguration {
        let mut candidate = self.config.clone();
        candidate.width = width;
        candidate.height = height;
        candidate
    }

    fn copy_src_candidate(&self) -> wgpu::SurfaceConfiguration {
        let mut candidate = self.config.clone();
        candidate.usage |= wgpu::TextureUsages::COPY_SRC;
        candidate
    }

    fn frame_latency_candidate(&self, latency: u32) -> wgpu::SurfaceConfiguration {
        let mut candidate = self.config.clone();
        candidate.desired_maximum_frame_latency = latency;
        candidate
    }

    fn publish(&mut self, candidate: wgpu::SurfaceConfiguration) {
        self.config = candidate;
        self.valid = true;
    }

    fn finish_transaction<E>(
        &mut self,
        candidate: wgpu::SurfaceConfiguration,
        configure_error: Option<E>,
        rollback_error: Option<E>,
    ) -> Result<(), ConfigurationTransactionFailure<E>> {
        let Some(configure_error) = configure_error else {
            debug_assert!(rollback_error.is_none());
            self.publish(candidate);
            return Ok(());
        };

        self.valid = rollback_error.is_none();
        Err(ConfigurationTransactionFailure {
            configure_error,
            rollback_error,
        })
    }

    async fn configure_scoped(
        surface: &wgpu::Surface<'_>,
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
    ) -> ConfigureScopeErrors {
        let (validation_scope, oom_scope, internal_scope) = (
            device.push_error_scope(wgpu::ErrorFilter::Validation),
            device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        surface.configure(device, config);
        ConfigureScopeErrors {
            internal: internal_scope.pop().await.map(|error| error.to_string()),
            out_of_memory: oom_scope.pop().await.map(|error| error.to_string()),
            validation: validation_scope.pop().await.map(|error| error.to_string()),
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn resize_transaction_failure_to_error(
    failure: ConfigurationTransactionFailure<SurfacePresenterError>,
) -> SurfacePresenterError {
    match failure {
        ConfigurationTransactionFailure {
            configure_error,
            rollback_error: None,
        } => configure_error,
        ConfigurationTransactionFailure {
            configure_error,
            rollback_error: Some(rollback_error),
        } => SurfacePresenterError::SurfaceResizeRollbackFailed {
            resize_error: configure_error.to_string(),
            rollback_error: rollback_error.to_string(),
        },
    }
}

fn copy_src_transaction_failure_to_error(
    failure: ConfigurationTransactionFailure<String>,
) -> SurfacePresenterError {
    match failure {
        ConfigurationTransactionFailure {
            configure_error,
            rollback_error: None,
        } => SurfacePresenterError::SurfaceCaptureUnsupported(format!(
            "COPY_SRC reconfiguration failed: {configure_error}"
        )),
        ConfigurationTransactionFailure {
            configure_error,
            rollback_error: Some(rollback_error),
        } => SurfacePresenterError::SurfaceCaptureState(format!(
            "COPY_SRC reconfiguration failed: {configure_error}; restoring the previous Surface configuration also failed: {rollback_error}"
        )),
    }
}

fn surface_configuration(
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    present_mode: wgpu::PresentMode,
    alpha_mode: wgpu::CompositeAlphaMode,
) -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width,
        height,
        present_mode,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn classify_surface_configure_scope_errors(
    internal: Option<String>,
    out_of_memory: Option<String>,
    validation: Option<String>,
) -> Option<SurfacePresenterError> {
    out_of_memory
        .map(|_| SurfacePresenterError::SurfaceOutOfMemory)
        .or_else(|| {
            internal
                .map(|error| SurfacePresenterError::SurfaceConfigure(format!("internal: {error}")))
        })
        .or_else(|| {
            validation.map(|error| {
                SurfacePresenterError::SurfaceConfigure(format!("validation: {error}"))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splat_surface_format_uses_non_srgb_image_domain_attachment() {
        assert_eq!(
            select_splat_surface_format(&[
                wgpu::TextureFormat::Bgra8UnormSrgb,
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureFormat::Rgba16Float,
            ]),
            Some(wgpu::TextureFormat::Bgra8Unorm)
        );
        assert_eq!(
            select_splat_surface_format(&[
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureFormat::Bgra8Unorm,
            ]),
            Some(wgpu::TextureFormat::Rgba8Unorm),
            "adapter order remains authoritative within exact formats"
        );
    }

    #[test]
    fn splat_surface_format_rejects_double_encoding_and_hdr_substitution() {
        assert_eq!(
            select_splat_surface_format(&[
                wgpu::TextureFormat::Bgra8UnormSrgb,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                wgpu::TextureFormat::Rgba16Float,
            ]),
            None
        );
        assert_eq!(select_splat_surface_format(&[]), None);
    }

    fn owner(valid: bool) -> SurfaceConfigurationOwner {
        SurfaceConfigurationOwner {
            config: surface_configuration(
                wgpu::TextureFormat::Bgra8UnormSrgb,
                1_920,
                1_080,
                wgpu::PresentMode::Fifo,
                wgpu::CompositeAlphaMode::Opaque,
            ),
            valid,
            max_texture_dimension_2d: 4_096,
        }
    }

    #[test]
    fn size_admission_and_candidates_preserve_published_configuration() {
        let configuration = owner(true);
        assert_eq!(configuration.size(), (1_920, 1_080));
        assert_eq!(configuration.format(), wgpu::TextureFormat::Bgra8UnormSrgb);
        assert!(configuration.ensure_valid().is_ok());
        assert!(!configuration.resize_required(1_920, 1_080));
        assert!(configuration.resize_required(2_412, 1_080));
        assert!(matches!(
            configuration.validate_size(0, 1_080),
            Err(SurfacePresenterError::InvalidSurfaceSize)
        ));
        assert!(matches!(
            configuration.validate_size(4_097, 1_080),
            Err(SurfacePresenterError::GpuDimensionsUnsupported {
                width: 4_097,
                height: 1_080,
                max_dimension: 4_096,
            })
        ));

        let resize = configuration.resize_candidate(2_412, 1_080);
        assert_eq!((resize.width, resize.height), (2_412, 1_080));
        assert_eq!(configuration.size(), (1_920, 1_080));
        let copy_src = configuration.copy_src_candidate();
        assert!(copy_src.usage.contains(wgpu::TextureUsages::COPY_SRC));
        assert_eq!(copy_src.format, configuration.format());
        let latency = configuration.frame_latency_candidate(4);
        assert_eq!(latency.desired_maximum_frame_latency, 4);
        assert_eq!(configuration.config().desired_maximum_frame_latency, 2);
    }

    #[test]
    fn transaction_commit_rollback_and_rollback_failure_publish_exact_state() {
        let mut configuration = owner(true);
        let candidate = configuration.resize_candidate(2_412, 1_080);
        configuration
            .finish_transaction::<String>(candidate, None, None)
            .expect("successful configure");
        assert_eq!(configuration.size(), (2_412, 1_080));
        assert!(configuration.ensure_valid().is_ok());

        let candidate = configuration.resize_candidate(2_622, 1_206);
        let failure = configuration
            .finish_transaction::<String>(candidate, Some("resize".into()), None)
            .expect_err("failed configure");
        assert_eq!(failure.configure_error, "resize");
        assert!(failure.rollback_error.is_none());
        assert_eq!(configuration.size(), (2_412, 1_080));
        assert!(configuration.ensure_valid().is_ok());

        let candidate = configuration.resize_candidate(2_622, 1_206);
        let failure = configuration
            .finish_transaction::<String>(candidate, Some("resize".into()), Some("rollback".into()))
            .expect_err("failed configure and rollback");
        assert_eq!(failure.configure_error, "resize");
        assert_eq!(failure.rollback_error.as_deref(), Some("rollback"));
        assert_eq!(configuration.size(), (2_412, 1_080));
        assert!(configuration.resize_required(2_412, 1_080));
        assert!(matches!(
            configuration.ensure_valid(),
            Err(SurfacePresenterError::SurfaceConfigure(ref message))
                if message == "surface is fail-closed after a resize rollback failure"
        ));
        assert!(matches!(
            configuration.ensure_capture_valid(),
            Err(SurfacePresenterError::SurfaceCaptureState(ref message))
                if message == "the Surface configuration is invalid"
        ));
    }

    #[test]
    fn resize_scope_errors_keep_existing_priority_and_strings() {
        assert!(matches!(
            ConfigureScopeErrors {
                internal: Some("internal".into()),
                out_of_memory: Some("oom".into()),
                validation: Some("validation".into()),
            }
            .resize_error(),
            Some(SurfacePresenterError::SurfaceOutOfMemory)
        ));
        assert!(matches!(
            ConfigureScopeErrors {
                internal: None,
                out_of_memory: None,
                validation: Some("invalid resize".into()),
            }
            .resize_error(),
            Some(SurfacePresenterError::SurfaceConfigure(message))
                if message == "validation: invalid resize"
        ));
        assert!(
            ConfigureScopeErrors {
                internal: None,
                out_of_memory: None,
                validation: None,
            }
            .resize_error()
            .is_none()
        );

        let resize_error = resize_transaction_failure_to_error(ConfigurationTransactionFailure {
            configure_error: SurfacePresenterError::SurfaceConfigure("validation: resize".into()),
            rollback_error: Some(SurfacePresenterError::SurfaceConfigure(
                "internal: rollback".into(),
            )),
        });
        assert!(matches!(
            resize_error,
            SurfacePresenterError::SurfaceResizeRollbackFailed {
                ref resize_error,
                ref rollback_error,
            } if resize_error == "surface configure failed: validation: resize"
                && rollback_error == "surface configure failed: internal: rollback"
        ));

        let capture_error =
            copy_src_transaction_failure_to_error(ConfigurationTransactionFailure {
                configure_error: "validation".into(),
                rollback_error: None,
            });
        assert!(matches!(
            capture_error,
            SurfacePresenterError::SurfaceCaptureUnsupported(ref message)
                if message == "COPY_SRC reconfiguration failed: validation"
        ));

        let capture_error =
            copy_src_transaction_failure_to_error(ConfigurationTransactionFailure {
                configure_error: "validation".into(),
                rollback_error: Some("rollback".into()),
            });
        assert!(matches!(
            capture_error,
            SurfacePresenterError::SurfaceCaptureState(ref message)
                if message == "COPY_SRC reconfiguration failed: validation; restoring the previous Surface configuration also failed: rollback"
        ));
    }
}
