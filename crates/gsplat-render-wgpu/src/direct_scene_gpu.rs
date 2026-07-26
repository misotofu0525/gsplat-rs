//! GPU scene resources for the wide-f32 Direct compatibility path.
//!
//! This module owns Direct source/order buffers and the matching draw
//! pipeline. Surface and offscreen hosts consume the same narrow resource
//! contract without owning its internal buffers or GPU sorter.

use bytemuck::Zeroable;
use gsplat_core::{Camera, SceneBuffers};
use wgpu::util::DeviceExt;

use crate::data::{
    CameraCovarianceTerms, GpuSurfaceRenderParams, GpuSurfaceSourceElem, SplatSetView,
};
use crate::direct_gpu_order::{DirectGpuOrder, GpuOrderTimestampRange};
use crate::scene::{DirectSceneError, DirectScenePath, direct_scene_preflight};
use crate::{make_surface_render_params, raster, wgpu_label};

pub(crate) struct DirectSceneResources {
    sorted_indices_buffer: wgpu::Buffer,
    params_buffer: wgpu::Buffer,
    cpu_bind_group: wgpu::BindGroup,
    capacity: usize,
    count: usize,
    sh_degree: u32,
    source_buffer: wgpu::Buffer,
    sh_rest_buffer: wgpu::Buffer,
    gpu_order: Option<DirectGpuSceneOrder>,
}

pub(crate) struct DirectGpuSceneOrder {
    sorter: DirectGpuOrder,
    bind_group: wgpu::BindGroup,
}

impl DirectGpuSceneOrder {
    pub(crate) fn is_empty(&self) -> bool {
        self.sorter.is_empty()
    }

    pub(crate) fn encode_with_timestamps(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<GpuOrderTimestampRange<'_>>,
    ) {
        self.sorter.encode_with_timestamps(encoder, timestamps);
    }

    pub(crate) fn set_indirect_vertex_count(&self, queue: &wgpu::Queue, vertex_count: u32) {
        self.sorter.set_indirect_vertex_count(queue, vertex_count);
    }

    pub(crate) const fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    pub(crate) fn indirect_args(&self) -> &wgpu::Buffer {
        self.sorter.indirect_args()
    }
}

impl DirectSceneResources {
    pub(crate) fn new(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        scene: &SceneBuffers,
        world_covariance_terms: &[CameraCovarianceTerms],
        alpha_values: &[f32],
    ) -> Result<Self, DirectSceneError> {
        let preflight = direct_scene_preflight(scene.len(), scene.sh_degree, &device.limits())?;
        if preflight.path != DirectScenePath::Direct {
            return Err(DirectSceneError::ResourceLimitExceeded(Box::new(preflight)));
        }
        let capacity = scene.len().max(1);
        let sorted_indices_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-direct-sorted-indices"),
            size: (capacity as u64) * (std::mem::size_of::<u32>() as u64),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-params"),
            contents: bytemuck::bytes_of(&GpuSurfaceRenderParams::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let source_elems = make_surface_source_elems(SplatSetView::new(
            &scene.positions,
            &scene.color_dc,
            world_covariance_terms,
            alpha_values,
        ));
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-source"),
            contents: bytemuck::cast_slice(&source_elems),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let sh_rest_fallback = [0.0_f32];
        let sh_rest = scene.sh_rest.as_deref().unwrap_or(&sh_rest_fallback);
        let sh_rest_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-sh-rest"),
            contents: bytemuck::cast_slice(sh_rest),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let cpu_bind_group = create_direct_bind_group(
            device,
            bind_group_layout,
            "gsplat-direct-cpu-order-bind-group",
            &sorted_indices_buffer,
            &source_buffer,
            &sh_rest_buffer,
            &params_buffer,
        );

        Ok(Self {
            sorted_indices_buffer,
            params_buffer,
            cpu_bind_group,
            capacity,
            count: scene.len(),
            sh_degree: scene.sh_degree as u32,
            source_buffer,
            sh_rest_buffer,
            gpu_order: None,
        })
    }

    pub(crate) const fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) const fn cpu_bind_group(&self) -> &wgpu::BindGroup {
        &self.cpu_bind_group
    }

    pub(crate) fn prepare_cpu(
        &self,
        queue: &wgpu::Queue,
        sorted_indices: &[u32],
        camera: &Camera,
        width: u32,
        height: u32,
        upload_order: bool,
    ) -> Result<u32, DirectSceneError> {
        if sorted_indices.len() > self.capacity {
            return Err(DirectSceneError::SortedIndexCapacityExceeded);
        }
        if upload_order && !sorted_indices.is_empty() {
            queue.write_buffer(
                &self.sorted_indices_buffer,
                0,
                bytemuck::cast_slice(sorted_indices),
            );
        }
        let instance_count = sorted_indices.len() as u32;
        let params =
            make_surface_render_params(camera, width, height, instance_count, self.sh_degree);
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(instance_count)
    }

    pub(crate) fn create_gpu_order_candidate(
        &self,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Result<DirectGpuSceneOrder, DirectSceneError> {
        let count =
            u32::try_from(self.count).map_err(|_| DirectSceneError::SortedIndexCapacityExceeded)?;
        let capacity = u32::try_from(self.capacity)
            .map_err(|_| DirectSceneError::SortedIndexCapacityExceeded)?;
        DirectGpuOrder::validate_soa_dispatch_limits(device, capacity, count)?;
        let sorter = DirectGpuOrder::new_soa(
            device,
            &self.source_buffer,
            &self.params_buffer,
            capacity,
            count,
        )?;
        let bind_group = create_direct_bind_group(
            device,
            bind_group_layout,
            "gsplat-direct-gpu-order-bind-group",
            sorter.final_ids(),
            &self.source_buffer,
            &self.sh_rest_buffer,
            &self.params_buffer,
        );
        Ok(DirectGpuSceneOrder { sorter, bind_group })
    }

    pub(crate) fn publish_gpu_order(&mut self, prepared: DirectGpuSceneOrder) {
        debug_assert!(self.gpu_order.is_none());
        self.gpu_order = Some(prepared);
    }

    fn ensure_gpu_order(
        &mut self,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Result<(), DirectSceneError> {
        if self.gpu_order.is_some() {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (device, bind_group_layout);
            Err(DirectSceneError::GpuOrderInitialization(
                "browser GPU ordering must be prepared asynchronously before selection".into(),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (validation_scope, oom_scope, internal_scope) = (
                device.push_error_scope(wgpu::ErrorFilter::Validation),
                device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
                device.push_error_scope(wgpu::ErrorFilter::Internal),
            );
            let prepared = self.create_gpu_order_candidate(device, bind_group_layout);
            let internal_error = pollster::block_on(internal_scope.pop());
            let oom_error = pollster::block_on(oom_scope.pop());
            let validation_error = pollster::block_on(validation_scope.pop());
            let scope_error = oom_error.or(internal_error).or(validation_error);
            if let Some(error) = scope_error {
                return Err(DirectSceneError::GpuOrderInitialization(error.to_string()));
            }
            self.publish_gpu_order(prepared?);
            Ok(())
        }
    }

    pub(crate) fn prepare_gpu(
        &mut self,
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        queue: &wgpu::Queue,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> Result<u32, DirectSceneError> {
        self.ensure_gpu_order(device, bind_group_layout)?;
        let instance_count =
            u32::try_from(self.count).map_err(|_| DirectSceneError::SortedIndexCapacityExceeded)?;
        let params =
            make_surface_render_params(camera, width, height, instance_count, self.sh_degree);
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(instance_count)
    }

    pub(crate) fn gpu_order(&self) -> Option<&DirectGpuSceneOrder> {
        self.gpu_order.as_ref()
    }
}

fn make_surface_source_elems(splats: SplatSetView<'_>) -> Vec<GpuSurfaceSourceElem> {
    if splats.is_empty() {
        return vec![GpuSurfaceSourceElem::zeroed()];
    }

    (0..splats.len())
        .map(|i| {
            let position = splats.positions()[i];
            let color_dc = splats.color_dc().get(i).copied().unwrap_or([0.0, 0.0, 0.0]);
            let cov =
                splats
                    .world_covariance_terms()
                    .get(i)
                    .copied()
                    .unwrap_or(CameraCovarianceTerms {
                        xx: 0.0,
                        xy: 0.0,
                        xz: 0.0,
                        yy: 0.0,
                        yz: 0.0,
                        zz: 0.0,
                    });
            let alpha = splats.alpha_values().get(i).copied().unwrap_or(0.0);
            GpuSurfaceSourceElem {
                position: [position.x, position.y, position.z, 0.0],
                covariance0: [cov.xx, cov.xy, cov.xz, cov.yy],
                covariance1: [cov.yz, cov.zz, alpha, 0.0],
                color_dc: [color_dc[0], color_dc[1], color_dc[2], 0.0],
            }
        })
        .collect()
}

fn create_direct_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    order_buffer: &wgpu::Buffer,
    source_buffer: &wgpu::Buffer,
    sh_rest_buffer: &wgpu::Buffer,
    params_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: order_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: source_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: sh_rest_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params_buffer.as_entire_binding(),
            },
        ],
    })
}

pub(crate) fn create_direct_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    raster::create_splat_bind_group_layout(device, "gsplat-direct-bgl", 3)
}

pub(crate) fn create_direct_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    raster::create_splat_pipeline(
        device,
        bind_group_layout,
        format,
        raster::SplatPipeline {
            shader_label: "gsplat-direct-shader",
            shader_source: include_str!("../shaders/splat_surface_direct.wgsl"),
            layout_label: "gsplat-direct-pipeline-layout",
            pipeline_label: "gsplat-direct-pipeline",
            topology: wgpu::PrimitiveTopology::TriangleStrip,
        },
    )
}
