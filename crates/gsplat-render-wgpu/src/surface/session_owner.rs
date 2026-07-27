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

#[cfg(test)]
use crate::gpu_telemetry::{
    CpuOrderCompletionTelemetry, CpuOrderTelemetryPoll, FrameInstanceCounts, TelemetrySubmission,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::RendererError;

#[cfg(any(
    not(target_arch = "wasm32"),
    feature = "diagnostic-surface-capture-receipt"
))]
use super::SurfaceFrameCapture;

pub(crate) enum SessionSurfaceOwner {
    Standalone(Box<SurfacePresenter>),
    ExactPacked(Box<SurfacePresenterHost>),
    #[cfg(test)]
    Test(Box<TestSessionSurfaceOwner>),
}

#[cfg(test)]
pub(crate) struct TestSessionSurfaceOwner {
    frame_presented: std::collections::VecDeque<bool>,
    last_frame_presented: bool,
    last_presented_size: Option<(u32, u32)>,
    size: (u32, u32),
    addressable_splat_count: usize,
    adapter_info: wgpu::AdapterInfo,
    geometry_path: GeometryPath,
    raster_execution_plan: SurfaceRasterExecutionPlan,
    cpu_completion_telemetry: Option<CpuOrderCompletionTelemetry>,
    telemetry_polls: usize,
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

    #[cfg(test)]
    pub(crate) fn test_direct(
        addressable_splat_count: usize,
        frame_presented: impl IntoIterator<Item = bool>,
    ) -> Self {
        Self::test_direct_with_telemetry(addressable_splat_count, frame_presented, false)
    }

    #[cfg(test)]
    pub(crate) fn test_direct_with_cpu_telemetry(
        addressable_splat_count: usize,
        frame_presented: impl IntoIterator<Item = bool>,
    ) -> Self {
        Self::test_direct_with_telemetry(addressable_splat_count, frame_presented, true)
    }

    #[cfg(test)]
    fn test_direct_with_telemetry(
        addressable_splat_count: usize,
        frame_presented: impl IntoIterator<Item = bool>,
        cpu_completion_telemetry: bool,
    ) -> Self {
        Self::Test(Box::new(TestSessionSurfaceOwner {
            frame_presented: frame_presented.into_iter().collect(),
            last_frame_presented: false,
            last_presented_size: None,
            size: (64, 64),
            addressable_splat_count,
            adapter_info: wgpu::AdapterInfo {
                name: "injected-session-surface".into(),
                vendor: 0,
                device: 0,
                device_type: wgpu::DeviceType::Other,
                device_pci_bus_id: String::new(),
                driver: "test".into(),
                driver_info: String::new(),
                backend: wgpu::Backend::Noop,
                subgroup_min_size: 1,
                subgroup_max_size: 1,
                transient_saves_memory: false,
            },
            geometry_path: GeometryPath::SortedIndexDirect,
            raster_execution_plan: SurfaceRasterExecutionPlan::GlobalQuads,
            cpu_completion_telemetry: cpu_completion_telemetry
                .then(CpuOrderCompletionTelemetry::default),
            telemetry_polls: 0,
        }))
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
            #[cfg(test)]
            Self::Test(test) => test.geometry_path,
        }
    }

    pub(crate) fn surface_size(&self) -> (u32, u32) {
        match self {
            Self::Standalone(presenter) => presenter.surface_size(),
            Self::ExactPacked(host) => host.surface_size(),
            #[cfg(test)]
            Self::Test(test) => test.size,
        }
    }

    pub(crate) fn adapter_info(&self) -> &wgpu::AdapterInfo {
        match self {
            Self::Standalone(presenter) => presenter.adapter_info(),
            Self::ExactPacked(host) => host.adapter_info(),
            #[cfg(test)]
            Self::Test(test) => &test.adapter_info,
        }
    }

    pub(crate) fn addressable_splat_count(&self) -> usize {
        match self {
            Self::Standalone(presenter) => presenter.addressable_splat_count(),
            Self::ExactPacked(host) => host.addressable_splat_count(),
            #[cfg(test)]
            Self::Test(test) => test.addressable_splat_count,
        }
    }

    pub(crate) const fn gpu_order_producer(&self) -> SurfaceGpuOrderProducer {
        SurfaceGpuOrderProducer::PostSort
    }

    pub(crate) fn adapter_max_storage_buffers_per_shader_stage(&self) -> u32 {
        match self {
            Self::Standalone(presenter) => presenter.adapter_max_storage_buffers_per_shader_stage(),
            Self::ExactPacked(host) => host.adapter_max_storage_buffers_per_shader_stage(),
            #[cfg(test)]
            Self::Test(_) => {
                wgpu::Limits::downlevel_defaults().max_storage_buffers_per_shader_stage
            }
        }
    }

    pub(crate) fn adapter_max_storage_buffer_binding_size(&self) -> u64 {
        match self {
            Self::Standalone(presenter) => presenter.adapter_max_storage_buffer_binding_size(),
            Self::ExactPacked(host) => host.adapter_max_storage_buffer_binding_size(),
            #[cfg(test)]
            Self::Test(_) => {
                u64::from(wgpu::Limits::downlevel_defaults().max_storage_buffer_binding_size)
            }
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
            #[cfg(test)]
            Self::Test(_) => unreachable!("test Direct owner has no Exact runtime"),
        }
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.resize(width, height),
            Self::ExactPacked(host) => host.resize(width, height),
            #[cfg(test)]
            Self::Test(test) => {
                test.size = (width, height);
                Ok(())
            }
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
            #[cfg(test)]
            Self::Test(_) => {
                let _ = latency;
            }
        }
    }

    pub(crate) fn last_presented_size(&self) -> Option<(u32, u32)> {
        match self {
            Self::Standalone(presenter) => presenter.last_presented_size(),
            Self::ExactPacked(host) => host.last_presented_size(),
            #[cfg(test)]
            Self::Test(test) => test.last_presented_size,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn request_surface_capture(&mut self) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.request_surface_capture(),
            Self::ExactPacked(host) => host.request_surface_capture(),
            #[cfg(test)]
            Self::Test(_) => Err(SurfacePresenterError::SurfaceCaptureUnsupported(
                "injected test Surface has no capture".into(),
            )),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn request_surface_capture_async(
        &mut self,
    ) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.request_surface_capture_async().await,
            Self::ExactPacked(host) => host.request_surface_capture_async().await,
            #[cfg(test)]
            Self::Test(_) => Err(SurfacePresenterError::SurfaceCaptureUnsupported(
                "injected test Surface has no capture".into(),
            )),
        }
    }

    pub(crate) fn cancel_surface_capture(&mut self) -> bool {
        match self {
            Self::Standalone(presenter) => presenter.cancel_surface_capture(),
            Self::ExactPacked(host) => host.cancel_surface_capture(),
            #[cfg(test)]
            Self::Test(_) => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn take_surface_capture(
        &mut self,
    ) -> Result<SurfaceFrameCapture, SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.take_surface_capture(),
            Self::ExactPacked(host) => host.take_surface_capture(),
            #[cfg(test)]
            Self::Test(_) => Err(SurfacePresenterError::SurfaceCaptureUnsupported(
                "injected test Surface has no capture".into(),
            )),
        }
    }

    #[cfg(all(target_arch = "wasm32", feature = "diagnostic-surface-capture-receipt"))]
    pub(crate) async fn take_surface_capture_async(
        &mut self,
    ) -> Result<SurfaceFrameCapture, SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.take_surface_capture_async().await,
            Self::ExactPacked(host) => host.take_surface_capture_async().await,
            #[cfg(test)]
            Self::Test(_) => Err(SurfacePresenterError::SurfaceCaptureUnsupported(
                "injected test Surface has no capture".into(),
            )),
        }
    }

    #[cfg(all(target_arch = "wasm32", feature = "diagnostic-surface-capture-receipt"))]
    pub(crate) fn request_diagnostic_queue_terminal(&mut self) -> bool {
        match self {
            Self::ExactPacked(host) => host.request_diagnostic_queue_terminal(),
            Self::Standalone(_) => false,
            #[cfg(test)]
            Self::Test(_) => false,
        }
    }

    #[cfg(all(target_arch = "wasm32", feature = "diagnostic-surface-capture-receipt"))]
    pub(crate) fn poll_diagnostic_queue_terminal(&mut self) -> Option<f64> {
        match self {
            Self::ExactPacked(host) => host.poll_diagnostic_queue_terminal(),
            Self::Standalone(_) => None,
            #[cfg(test)]
            Self::Test(_) => None,
        }
    }

    pub(crate) fn raster_execution_plan(&self) -> SurfaceRasterExecutionPlan {
        match self {
            Self::Standalone(presenter) => presenter.raster_execution_plan(),
            Self::ExactPacked(_) => SurfaceRasterExecutionPlan::ProjectedQuadsExact,
            #[cfg(test)]
            Self::Test(test) => test.raster_execution_plan,
        }
    }

    pub(crate) fn internal_render_size(&self) -> (u32, u32) {
        match self {
            Self::Standalone(presenter) => presenter.internal_render_size(),
            Self::ExactPacked(host) => host.surface_size(),
            #[cfg(test)]
            Self::Test(test) => test.size,
        }
    }

    pub(crate) async fn prepare_gpu_order_producer(
        &mut self,
        producer: SurfaceGpuOrderProducer,
    ) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.prepare_gpu_order_producer(producer).await,
            Self::ExactPacked(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
            #[cfg(test)]
            Self::Test(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
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
            #[cfg(test)]
            Self::Test(_) => match producer {
                SurfaceGpuOrderProducer::PostSort => Ok(()),
                SurfaceGpuOrderProducer::Preproject => {
                    Err(SurfacePresenterError::PreprojectProducerIncompatible)
                }
            },
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
            #[cfg(test)]
            Self::Test(test) => {
                test.raster_execution_plan = plan;
                Ok(())
            }
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
            #[cfg(test)]
            Self::Test(test) => match path {
                GeometryPath::SortedIndexDirect | GeometryPath::PagedActiveAtlas => {
                    if test.geometry_path != path {
                        test.geometry_path = path;
                        if let Some(telemetry) = test.cpu_completion_telemetry.as_mut() {
                            telemetry.invalidate_generation();
                        }
                    }
                    Ok(())
                }
                GeometryPath::PackedAtlas => {
                    Err(SurfacePresenterError::SurfaceGeometrySwitchUnsupported)
                }
            },
        }
    }

    pub(crate) async fn prepare_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.prepare_gpu_order().await,
            Self::ExactPacked(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
            #[cfg(test)]
            Self::Test(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
        }
    }

    pub(crate) fn prepare_direct_gpu_order(&mut self) -> Result<(), SurfacePresenterError> {
        match self {
            Self::Standalone(presenter) => presenter.prepare_direct_gpu_order(),
            Self::ExactPacked(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
            #[cfg(test)]
            Self::Test(_) => Err(SurfacePresenterError::GpuOrderUnsupported),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn pump_receipt_callbacks(&self, timeout: Duration) -> Result<bool, RendererError> {
        match self {
            Self::Standalone(presenter) => presenter.pump_receipt_callbacks(timeout),
            Self::ExactPacked(host) => host.pump_receipt_callbacks(timeout),
            #[cfg(test)]
            Self::Test(_) => {
                let _ = timeout;
                Ok(true)
            }
        }
    }

    pub(crate) fn last_frame_presented(&self) -> bool {
        match self {
            Self::Standalone(presenter) => presenter.last_frame_presented(),
            Self::ExactPacked(host) => host.last_frame_presented(),
            #[cfg(test)]
            Self::Test(test) => test.last_frame_presented,
        }
    }

    #[cfg(test)]
    pub(crate) fn take_test_frame_presented(&mut self) -> Option<bool> {
        let Self::Test(test) = self else {
            return None;
        };
        let presented = test
            .frame_presented
            .pop_front()
            .expect("test Surface frame outcome");
        test.last_frame_presented = presented;
        test.last_presented_size = presented.then_some(test.size);
        Some(presented)
    }

    #[cfg(test)]
    pub(crate) fn begin_test_cpu_completion(
        &mut self,
        camera_revision: u64,
        preprocess_ms: f32,
        sort_ms: f32,
        counts: FrameInstanceCounts,
    ) -> TelemetrySubmission {
        let Self::Test(test) = self else {
            panic!("CPU completion injection requires the test Surface owner");
        };
        let Some(telemetry) = test.cpu_completion_telemetry.as_mut() else {
            return TelemetrySubmission::NotRequested;
        };
        telemetry
            .begin_submitted_sample_for_test(camera_revision, preprocess_ms, sort_ms, counts)
            .map_or(TelemetrySubmission::RingBusy, TelemetrySubmission::Issued)
    }

    #[cfg(test)]
    pub(crate) fn complete_test_cpu_completion(
        &mut self,
        ticket: u64,
        frame_complete_ms: f32,
    ) -> bool {
        let Self::Test(test) = self else {
            panic!("CPU completion injection requires the test Surface owner");
        };
        test.cpu_completion_telemetry
            .as_mut()
            .is_some_and(|telemetry| {
                telemetry.complete_submitted_sample_for_test(ticket, frame_complete_ms)
            })
    }

    #[cfg(test)]
    pub(crate) fn poll_test_cpu_completion_telemetry(&mut self) -> CpuOrderTelemetryPoll {
        let Self::Test(test) = self else {
            panic!("CPU completion injection requires the test Surface owner");
        };
        test.telemetry_polls += 1;
        test.cpu_completion_telemetry.as_mut().map_or(
            CpuOrderTelemetryPoll {
                completed: Vec::new(),
                failures: Vec::new(),
            },
            CpuOrderCompletionTelemetry::poll,
        )
    }

    #[cfg(test)]
    pub(crate) fn push_test_frame_presented(&mut self, presented: bool) {
        let Self::Test(test) = self else {
            panic!("frame outcome injection requires the test Surface owner");
        };
        test.frame_presented.push_back(presented);
    }

    #[cfg(test)]
    pub(crate) fn record_test_telemetry_poll(&mut self) {
        let Self::Test(test) = self else {
            return;
        };
        test.telemetry_polls += 1;
    }

    #[cfg(test)]
    pub(crate) fn test_telemetry_polls(&self) -> usize {
        let Self::Test(test) = self else {
            panic!("telemetry poll inspection requires the test Surface owner");
        };
        test.telemetry_polls
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
