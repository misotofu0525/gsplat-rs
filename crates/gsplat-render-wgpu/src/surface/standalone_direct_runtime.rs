//! Standalone Direct compatibility execution for a native/WebGPU Surface.
//!
//! This module owns the Direct-only pipeline, active scene, CPU/GPU order
//! preparation, draw encoding, and GPU-order telemetry. Surface acquisition,
//! submission, presentation, capture, and path-switch coordination remain in
//! `SurfacePresenter`.

use gsplat_core::{Camera, SceneBuffers};

use crate::data::CameraCovarianceTerms;
use crate::direct_gpu_order::GpuOrderTimestampRange;
use crate::direct_scene_gpu::{
    DirectGpuSceneOrder, DirectSceneResources, create_direct_bind_group_layout,
    create_direct_pipeline,
};
use crate::gpu_telemetry::{GpuOrderTelemetry, GpuOrderTelemetryPoll, GpuTelemetryTicket};
use crate::raster::{
    QUAD_VERTEX_COUNT, SplatDraw, SplatIndirectDraw, encode_splat_draw_into,
    encode_splat_indirect_draw_into,
};
use crate::{DirectSceneError, SurfacePresenterError, TimerInstant};

pub(crate) struct PreparedStandaloneDirectScene(Box<DirectSceneResources>);

impl PreparedStandaloneDirectScene {
    pub(crate) fn addressable_splat_count(&self) -> usize {
        self.0.capacity()
    }
}

enum PreparedDirectGpuOrder {
    AlreadyPrepared,
    Candidate(Box<DirectGpuSceneOrder>),
}

pub(crate) struct DirectGpuTelemetrySample(GpuTelemetryTicket);

impl DirectGpuTelemetrySample {
    pub(crate) const fn ticket(&self) -> u64 {
        self.0.ticket
    }
}

pub(crate) struct StandaloneDirectRuntime {
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    scene: Option<Box<DirectSceneResources>>,
    instance_count: u32,
    gpu_order_telemetry: GpuOrderTelemetry,
}

impl StandaloneDirectRuntime {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        timestamp_queries_enabled: bool,
    ) -> Self {
        let bind_group_layout = create_direct_bind_group_layout(device);
        let pipeline = create_direct_pipeline(device, &bind_group_layout, format);
        let gpu_order_telemetry = GpuOrderTelemetry::new(device, queue, timestamp_queries_enabled);
        Self {
            bind_group_layout,
            pipeline,
            scene: None,
            instance_count: 0,
            gpu_order_telemetry,
        }
    }

    pub(crate) fn prepare_scene_candidate(
        &self,
        device: &wgpu::Device,
        scene: &SceneBuffers,
        world_covariance_terms: &[CameraCovarianceTerms],
        alpha_values: &[f32],
    ) -> Result<PreparedStandaloneDirectScene, DirectSceneError> {
        DirectSceneResources::new(
            device,
            &self.bind_group_layout,
            scene,
            world_covariance_terms,
            alpha_values,
        )
        .map(Box::new)
        .map(PreparedStandaloneDirectScene)
    }

    pub(crate) fn publish_scene(&mut self, prepared: PreparedStandaloneDirectScene) {
        self.scene = Some(prepared.0);
        self.instance_count = 0;
    }

    pub(crate) fn clear_scene(&mut self) {
        self.scene = None;
        self.instance_count = 0;
    }

    pub(crate) const fn instance_count(&self) -> u32 {
        self.instance_count
    }

    pub(crate) fn invalidate_gpu_order_telemetry(&mut self) {
        self.gpu_order_telemetry.invalidate_generation();
    }

    pub(crate) fn gpu_order_timestamps_enabled(&self) -> bool {
        self.gpu_order_telemetry.timestamps_enabled()
    }

    pub(crate) fn poll_gpu_order_telemetry(
        &mut self,
        device: &wgpu::Device,
    ) -> GpuOrderTelemetryPoll {
        self.gpu_order_telemetry.poll(device)
    }

    pub(crate) fn prepare_cpu_order(
        &mut self,
        queue: &wgpu::Queue,
        sorted_indices: &[u32],
        camera: &Camera,
        width: u32,
        height: u32,
        upload_order: bool,
    ) -> Result<(), SurfacePresenterError> {
        let scene = self
            .scene
            .as_ref()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        self.instance_count =
            scene.prepare_cpu(queue, sorted_indices, camera, width, height, upload_order)?;
        Ok(())
    }

    pub(crate) fn encode_cpu_draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> Result<(), SurfacePresenterError> {
        let scene = self
            .scene
            .as_ref()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        encode_splat_draw_into(
            encoder,
            &SplatDraw {
                pass_label: "gsplat-surface-direct-pass",
                view,
                pipeline: &self.pipeline,
                bind_group: scene.cpu_bind_group(),
                clear: wgpu::Color::BLACK,
                vertex_count: QUAD_VERTEX_COUNT,
                instance_count: self.instance_count,
            },
        );
        Ok(())
    }

    pub(crate) fn gpu_order_is_prepared(&self) -> bool {
        self.scene
            .as_ref()
            .and_then(|scene| scene.gpu_order())
            .is_some()
    }

    fn create_gpu_order_candidate(
        &self,
        device: &wgpu::Device,
    ) -> Result<PreparedDirectGpuOrder, SurfacePresenterError> {
        let scene = self
            .scene
            .as_ref()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        if scene.gpu_order().is_some() {
            return Ok(PreparedDirectGpuOrder::AlreadyPrepared);
        }
        scene
            .create_gpu_order_candidate(device, &self.bind_group_layout)
            .map(Box::new)
            .map(PreparedDirectGpuOrder::Candidate)
            .map_err(SurfacePresenterError::from)
    }

    fn publish_gpu_order_candidate(&mut self, prepared: PreparedDirectGpuOrder) {
        match prepared {
            PreparedDirectGpuOrder::AlreadyPrepared => {}
            PreparedDirectGpuOrder::Candidate(order) => self
                .scene
                .as_mut()
                .expect("Direct GPU-order candidate requires an active Direct scene")
                .publish_gpu_order(*order),
        }
    }

    pub(crate) async fn prepare_gpu_order(
        &mut self,
        device: &wgpu::Device,
    ) -> Result<(), SurfacePresenterError> {
        if self.gpu_order_is_prepared() {
            return Ok(());
        }

        let (validation_scope, oom_scope, internal_scope) = (
            device.push_error_scope(wgpu::ErrorFilter::Validation),
            device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            device.push_error_scope(wgpu::ErrorFilter::Internal),
        );
        let prepared = self.create_gpu_order_candidate(device);
        let internal_error = internal_scope.pop().await;
        let oom_error = oom_scope.pop().await;
        let validation_error = validation_scope.pop().await;
        if let Some(error) = classify_direct_gpu_order_scope_errors(
            internal_error.map(|error| error.to_string()),
            oom_error.map(|error| error.to_string()),
            validation_error.map(|error| error.to_string()),
        ) {
            return Err(error);
        }
        self.publish_gpu_order_candidate(prepared?);
        Ok(())
    }

    pub(crate) fn prepare_gpu_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<(), SurfacePresenterError> {
        let scene = self
            .scene
            .as_mut()
            .ok_or(SurfacePresenterError::SceneNotLoaded)?;
        self.instance_count = scene.prepare_gpu(
            device,
            &self.bind_group_layout,
            queue,
            camera,
            width,
            height,
        )?;
        Ok(())
    }

    pub(crate) fn begin_gpu_order_sample(
        &mut self,
        camera_revision: u64,
        refresh_order: bool,
    ) -> Result<Option<DirectGpuTelemetrySample>, SurfacePresenterError> {
        if !refresh_order {
            return Ok(None);
        }
        let allow_timestamps = !self
            .scene
            .as_ref()
            .and_then(|scene| scene.gpu_order())
            .ok_or(SurfacePresenterError::GpuOrderUnsupported)?
            .is_empty();
        Ok(self
            .gpu_order_telemetry
            .begin_sample(camera_revision, allow_timestamps)
            .map(DirectGpuTelemetrySample))
    }

    pub(crate) fn encode_gpu_order_draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        queue: &wgpu::Queue,
        refresh_order: bool,
        telemetry_sample: Option<&DirectGpuTelemetrySample>,
    ) -> Result<(), SurfacePresenterError> {
        let timestamp_range = telemetry_sample.and_then(|sample| {
            sample
                .0
                .query_set
                .as_ref()
                .map(|query_set| GpuOrderTimestampRange {
                    query_set,
                    keygen_begin_index: 0,
                    keygen_end_index: 1,
                    radix_begin_index: 2,
                    radix_end_index: 3,
                })
        });
        let Self {
            pipeline,
            scene,
            gpu_order_telemetry,
            ..
        } = self;
        let order = scene
            .as_ref()
            .and_then(|scene| scene.gpu_order())
            .ok_or(SurfacePresenterError::GpuOrderUnsupported)?;
        if refresh_order {
            order.encode_with_timestamps(encoder, timestamp_range);
        }
        order.set_indirect_vertex_count(queue, QUAD_VERTEX_COUNT);
        encode_splat_indirect_draw_into(
            encoder,
            &SplatIndirectDraw {
                pass_label: "gsplat-surface-direct-gpu-order-draw-pass",
                view,
                pipeline,
                bind_group: order.bind_group(),
                clear: wgpu::Color::BLACK,
                indirect_args: order.indirect_args(),
            },
        );
        if let Some(sample) = telemetry_sample {
            gpu_order_telemetry.encode_readback(encoder, &sample.0, order.indirect_args());
        }
        Ok(())
    }

    pub(crate) fn arm_gpu_order_sample(
        &mut self,
        command_buffer: &wgpu::CommandBuffer,
        sample: DirectGpuTelemetrySample,
        completion_started: TimerInstant,
    ) {
        self.gpu_order_telemetry
            .arm(command_buffer, sample.0, completion_started);
    }
}

fn classify_direct_gpu_order_scope_errors(
    internal: Option<String>,
    out_of_memory: Option<String>,
    validation: Option<String>,
) -> Option<SurfacePresenterError> {
    out_of_memory
        .map(|error| format!("out of memory: {error}"))
        .or_else(|| internal.map(|error| format!("internal: {error}")))
        .or_else(|| validation.map(|error| format!("validation: {error}")))
        .map(DirectSceneError::GpuOrderInitialization)
        .map(SurfacePresenterError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_gpu_order_scope_success_has_no_error() {
        assert!(classify_direct_gpu_order_scope_errors(None, None, None).is_none());
    }

    #[test]
    fn direct_gpu_order_scope_error_priority_and_labels_are_preserved() {
        for (internal, oom, validation, expected) in [
            (
                Some("internal".to_owned()),
                Some("oom".to_owned()),
                Some("validation".to_owned()),
                "out of memory: oom",
            ),
            (
                Some("internal".to_owned()),
                None,
                Some("validation".to_owned()),
                "internal: internal",
            ),
            (
                None,
                None,
                Some("validation".to_owned()),
                "validation: validation",
            ),
        ] {
            assert!(matches!(
                classify_direct_gpu_order_scope_errors(internal, oom, validation),
                Some(SurfacePresenterError::DirectScene(
                    DirectSceneError::GpuOrderInitialization(message)
                )) if message == expected
            ));
        }
    }
}
