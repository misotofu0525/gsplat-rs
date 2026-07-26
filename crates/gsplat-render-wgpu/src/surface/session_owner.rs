//! Surface resource owner used by [`crate::SurfaceRenderSession`].
//!
//! This adapter selects and owns either the standalone Direct/Paged presenter
//! or the product Packed host. It forwards only Surface/resource operations;
//! frame scheduling, policy, telemetry interpretation, evidence publication,
//! and the presented-frame commit remain owned by the session facade.

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use crate::surface_presenter::{SurfacePresenter, SurfacePresenterHost};
use crate::{
    GeometryPath, Renderer, SurfaceGpuOrderProducer, SurfacePresenterError,
    SurfaceRasterExecutionPlan,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::RendererError;

#[cfg(not(target_arch = "wasm32"))]
use super::SurfaceFrameCapture;

pub(crate) enum SessionSurfaceOwner {
    Standalone(Box<SurfacePresenter>),
    ExactPacked(Box<SurfacePresenterHost>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionSurfaceConstruction {
    StandalonePresenter,
    ExactPackedHost,
}

impl SessionSurfaceConstruction {
    const fn for_geometry(path: GeometryPath) -> Self {
        match path {
            GeometryPath::PackedAtlas => Self::ExactPackedHost,
            GeometryPath::SortedIndexDirect | GeometryPath::PagedActiveAtlas => {
                Self::StandalonePresenter
            }
        }
    }

    #[cfg(test)]
    const fn creates_standalone_presenter_graph(self) -> bool {
        matches!(self, Self::StandalonePresenter)
    }
}

impl SessionSurfaceOwner {
    pub(crate) fn standalone(presenter: SurfacePresenter) -> Self {
        Self::Standalone(Box::new(presenter))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn from_window<T>(
        renderer: &Renderer,
        target: T,
        width: u32,
        height: u32,
    ) -> Result<Self, SurfacePresenterError>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        match SessionSurfaceConstruction::for_geometry(renderer.geometry_path()) {
            SessionSurfaceConstruction::ExactPackedHost => Ok(Self::ExactPacked(Box::new(
                SurfacePresenterHost::from_window(target, width, height, renderer).await?,
            ))),
            SessionSurfaceConstruction::StandalonePresenter => Ok(Self::Standalone(Box::new(
                SurfacePresenter::from_window(target, width, height, renderer).await?,
            ))),
        }
    }

    /// Creates the path-appropriate owner from raw native handles.
    ///
    /// # Safety
    ///
    /// The caller must keep both raw handles valid until the owner is dropped.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) unsafe fn from_raw_handles(
        renderer: &Renderer,
        raw_display_handle: wgpu::rwh::RawDisplayHandle,
        raw_window_handle: wgpu::rwh::RawWindowHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, SurfacePresenterError> {
        match SessionSurfaceConstruction::for_geometry(renderer.geometry_path()) {
            SessionSurfaceConstruction::ExactPackedHost => {
                Ok(Self::ExactPacked(Box::new(unsafe {
                    SurfacePresenterHost::from_raw_handles(
                        raw_display_handle,
                        raw_window_handle,
                        width,
                        height,
                        renderer,
                    )?
                })))
            }
            SessionSurfaceConstruction::StandalonePresenter => {
                Ok(Self::Standalone(Box::new(unsafe {
                    SurfacePresenter::from_raw_handles(
                        raw_display_handle,
                        raw_window_handle,
                        width,
                        height,
                        renderer,
                    )?
                })))
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn from_canvas(
        renderer: &Renderer,
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<Self, SurfacePresenterError> {
        match SessionSurfaceConstruction::for_geometry(renderer.geometry_path()) {
            SessionSurfaceConstruction::ExactPackedHost => Ok(Self::ExactPacked(Box::new(
                SurfacePresenterHost::from_canvas(canvas, width, height, renderer).await?,
            ))),
            SessionSurfaceConstruction::StandalonePresenter => Ok(Self::Standalone(Box::new(
                SurfacePresenter::from_canvas(canvas, width, height, renderer).await?,
            ))),
        }
    }

    pub(crate) const fn geometry_path(&self) -> GeometryPath {
        match self {
            Self::Standalone(presenter) => presenter.geometry_path(),
            Self::ExactPacked(_) => GeometryPath::PackedAtlas,
        }
    }

    pub(crate) fn surface_size(&self) -> (u32, u32) {
        match self {
            Self::Standalone(presenter) => presenter.surface_size(),
            Self::ExactPacked(host) => host.surface_size(),
        }
    }

    pub(crate) fn adapter_info(&self) -> &wgpu::AdapterInfo {
        match self {
            Self::Standalone(presenter) => presenter.adapter_info(),
            Self::ExactPacked(host) => host.adapter_info(),
        }
    }

    pub(crate) fn addressable_splat_count(&self) -> usize {
        match self {
            Self::Standalone(presenter) => presenter.addressable_splat_count(),
            Self::ExactPacked(host) => host.addressable_splat_count(),
        }
    }

    pub(crate) const fn gpu_order_producer(&self) -> SurfaceGpuOrderProducer {
        SurfaceGpuOrderProducer::PostSort
    }

    pub(crate) fn adapter_max_storage_buffers_per_shader_stage(&self) -> u32 {
        match self {
            Self::Standalone(presenter) => presenter.adapter_max_storage_buffers_per_shader_stage(),
            Self::ExactPacked(host) => host.adapter_max_storage_buffers_per_shader_stage(),
        }
    }

    pub(crate) fn adapter_max_storage_buffer_binding_size(&self) -> u64 {
        match self {
            Self::Standalone(presenter) => presenter.adapter_max_storage_buffer_binding_size(),
            Self::ExactPacked(host) => host.adapter_max_storage_buffer_binding_size(),
        }
    }

    pub(crate) fn exact_runtime_context(
        &self,
    ) -> (
        std::sync::Arc<wgpu::Device>,
        std::sync::Arc<wgpu::Queue>,
        wgpu::TextureFormat,
        bool,
    ) {
        match self {
            Self::Standalone(_) => {
                unreachable!("Exact Surface runtime requires the Packed host owner")
            }
            Self::ExactPacked(host) => host.exact_runtime_context(),
        }
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.resize(width, height),
            Self::ExactPacked(host) => host.resize(width, height),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn resize_async(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.resize_async(width, height).await,
            Self::ExactPacked(host) => host.resize_async(width, height).await,
        }
    }

    pub(crate) fn set_frame_latency(&mut self, latency: u32) {
        match self {
            Self::Standalone(presenter) => presenter.set_frame_latency(latency),
            Self::ExactPacked(host) => {
                host.set_frame_latency(latency);
            }
        }
    }

    pub(crate) fn last_presented_size(&self) -> Option<(u32, u32)> {
        match self {
            Self::Standalone(presenter) => presenter.last_presented_size(),
            Self::ExactPacked(host) => host.last_presented_size(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn request_surface_capture(&mut self) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.request_surface_capture(),
            Self::ExactPacked(host) => host.request_surface_capture(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cancel_surface_capture(&mut self) -> bool {
        match self {
            Self::Standalone(presenter) => presenter.cancel_surface_capture(),
            Self::ExactPacked(host) => host.cancel_surface_capture(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn take_surface_capture(
        &mut self,
    ) -> Result<SurfaceFrameCapture, SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.take_surface_capture(),
            Self::ExactPacked(host) => host.take_surface_capture(),
        }
    }

    pub(crate) fn raster_execution_plan(&self) -> SurfaceRasterExecutionPlan {
        match self {
            Self::Standalone(presenter) => presenter.raster_execution_plan(),
            Self::ExactPacked(_) => SurfaceRasterExecutionPlan::ProjectedQuadsExact,
        }
    }

    pub(crate) fn internal_render_size(&self) -> (u32, u32) {
        match self {
            Self::Standalone(presenter) => presenter.internal_render_size(),
            Self::ExactPacked(host) => host.surface_size(),
        }
    }

    pub(crate) async fn prepare_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.prepare_gpu_order_producer(producer).await,
            Self::ExactPacked(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
        }
    }

    pub(crate) fn set_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(_) => match producer {
                SurfaceGpuOrderProducer::PostSort => Ok(()),
                SurfaceGpuOrderProducer::Preproject => {
                    Err(SurfacePresenterError::PreprojectProducerIncompatible)
                }
            },
            Self::ExactPacked(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
        }
    }

    pub(crate) const fn projected_contributor_indirect_draw_enabled(&self) -> bool {
        false
    }

    pub(crate) fn set_raster_execution_plan(
        &mut self,
        plan: SurfaceRasterExecutionPlan,
    ) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.set_raster_execution_plan(plan),
            Self::ExactPacked(_) => Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported),
        }
    }

    pub(crate) fn set_geometry_path(
        &mut self,
        path: GeometryPath,
        renderer: &Renderer,
    ) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.set_geometry_path(path, renderer),
            Self::ExactPacked(_) => Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported),
        }
    }

    pub(crate) async fn prepare_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.prepare_gpu_order().await,
            Self::ExactPacked(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
        }
    }

    pub(crate) fn prepare_direct_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.prepare_direct_gpu_order(),
            Self::ExactPacked(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn pump_receipt_callbacks(&self, timeout: Duration) -> Result<bool, RendererError> {
        match self {
            Self::Standalone(presenter) => presenter.pump_receipt_callbacks(timeout),
            Self::ExactPacked(host) => host.pump_receipt_callbacks(timeout),
        }
    }

    pub(crate) fn last_frame_presented(&self) -> bool {
        match self {
            Self::Standalone(presenter) => presenter.last_frame_presented(),
            Self::ExactPacked(host) => host.last_frame_presented(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_packed_construction_skips_standalone_presenter_resources() {
        let packed = SessionSurfaceConstruction::for_geometry(GeometryPath::PackedAtlas);
        assert_eq!(packed, SessionSurfaceConstruction::ExactPackedHost);
        assert!(!packed.creates_standalone_presenter_graph());

        for path in [
            GeometryPath::SortedIndexDirect,
            GeometryPath::PagedActiveAtlas,
        ] {
            let construction = SessionSurfaceConstruction::for_geometry(path);
            assert_eq!(
                construction,
                SessionSurfaceConstruction::StandalonePresenter
            );
            assert!(construction.creates_standalone_presenter_graph());
        }
    }

    #[test]
    fn session_surface_owner_has_no_implicit_presenter_deref() {
        let source = include_str!("session_owner.rs");
        assert!(!source.contains(concat!("impl ", "Deref for SessionSurfaceOwner")));
        assert!(!source.contains(concat!("impl ", "DerefMut for SessionSurfaceOwner")));
        assert_eq!(
            source
                .matches(concat!(
                    "Self::ExactPacked(host) => ",
                    "host.resize(width, height)"
                ))
                .count(),
            1
        );
    }

    #[test]
    fn session_surface_owner_contains_only_resource_and_forwarding_responsibilities() {
        let source = include_str!("session_owner.rs");
        for forbidden in [
            concat!("Adaptive", "OrderPolicy"),
            concat!("Adaptive", "ProjectedDrawPolicy"),
            concat!("Compatibility", "EvidenceStore"),
            concat!("Surface", "CurrentStats"),
            concat!("Native", "CpuOrderWorker"),
            concat!("Surface", "FramePlan"),
            concat!("render_", "frame_sync"),
            concat!("render_", "exact_frame"),
            concat!("render_", "direct_gpu_order"),
            concat!("render_", "sorted_indices"),
            concat!("render_", "cpu_sorted_indices_tracked"),
            concat!("poll_", "cpu_order_completion_telemetry"),
            concat!("poll_", "gpu_order_telemetry"),
            concat!("poll_", "projected_draw_telemetry"),
            concat!("poll_", "gpu_producer_telemetry"),
        ] {
            assert!(
                !source.contains(forbidden),
                "Session Surface owner retained policy/frame responsibility {forbidden}"
            );
        }
    }
}
