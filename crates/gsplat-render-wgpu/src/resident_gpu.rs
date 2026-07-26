//! GPU resources for the exact-count compact resident scene.

use bytemuck::Zeroable;
use gsplat_core::Camera;
use wgpu::util::DeviceExt;

use crate::cpu_order::DepthKeyPrecision;
use crate::data::{RESIDENT_SH_PLANES, ResidentChunkMeta};
use crate::direct_gpu_order::DirectGpuOrder;
use crate::gpu::{ResidentColorKernel, create_resident_color_params_buffer};
pub(crate) use crate::gpu::{
    create_resident_color_bind_group_layout, create_resident_color_pipeline,
};
pub(crate) use crate::gpu_error::ResidentGpuError;
#[cfg(not(target_arch = "wasm32"))]
use crate::raster::{SplatPipeline, create_splat_bind_group_layout, create_splat_pipeline};
pub(crate) use crate::scene::RESIDENT_COLOR_STORAGE_BINDINGS;
use crate::scene::{ResidentGpuBytePlan, ResidentSceneCpu};
use crate::{GpuSurfaceRenderParams, make_surface_render_params, wgpu_label};

pub struct ResidentGpuResources {
    pub order_buffer: wgpu::Buffer,
    pub position_alpha_buffer: wgpu::Buffer,
    pub covariance0_buffer: wgpu::Buffer,
    pub covariance1_buffer: wgpu::Buffer,
    // Retained because the color bind group references this plane.
    _color_auxiliary_buffer: wgpu::Buffer,
    // Retained because the color bind group references all four planes.
    _sh_buffers: [wgpu::Buffer; RESIDENT_SH_PLANES],
    // Retained because the color bind group references chunk-local DC/SH ranges.
    _chunk_metadata_buffer: wgpu::Buffer,
    pub resolved_color_buffer: wgpu::Buffer,
    pub draw_params_buffer: wgpu::Buffer,
    color_params_buffer: wgpu::Buffer,
    color_bind_group: wgpu::BindGroup,
    pub capacity: usize,
    pub sh_degree: u32,
    _byte_plan: ResidentGpuBytePlan,
    #[cfg(not(target_arch = "wasm32"))]
    last_resolved_camera_position: Option<[f32; 3]>,
    gpu_order: Option<ResidentGpuSceneOrder>,
}

pub(crate) struct ResidentGpuSceneOrder {
    pub(crate) sorter: DirectGpuOrder,
}

impl ResidentGpuResources {
    pub fn new(
        device: &wgpu::Device,
        color_layout: &wgpu::BindGroupLayout,
        scene: &ResidentSceneCpu,
    ) -> Result<Self, ResidentGpuError> {
        scene.validate_complete().map_err(|error| match error {
            crate::ResidentSceneError::UploadStagingReleased => {
                ResidentGpuError::UploadStagingUnavailable
            }
            _ => ResidentGpuError::IncompleteScene,
        })?;
        let staging = scene
            .upload_staging()
            .map_err(|_| ResidentGpuError::UploadStagingUnavailable)?;
        let byte_plan = ResidentGpuBytePlan::for_scene(scene)?.validate_limits(&device.limits())?;
        let capacity = scene.len();
        let _ = u32::try_from(capacity).map_err(|_| ResidentGpuError::AddressSpaceExceeded)?;

        let order_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-resident-order"),
            size: byte_plan.order.max(4),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let position_alpha_buffer = create_storage_init(
            device,
            "gsplat-resident-position-alpha",
            bytemuck::cast_slice(&staging.position_alpha),
            16,
        );
        let covariance0_buffer = create_storage_init(
            device,
            "gsplat-resident-covariance-0",
            bytemuck::cast_slice(&staging.covariance0),
            16,
        );
        let covariance1_buffer = create_storage_init(
            device,
            "gsplat-resident-covariance-1",
            bytemuck::cast_slice(&staging.covariance1),
            8,
        );
        let color_auxiliary_buffer = create_storage_init(
            device,
            "gsplat-resident-color-auxiliary",
            bytemuck::cast_slice(&staging.color_aux),
            8,
        );
        let sh_buffers = std::array::from_fn(|plane| {
            create_storage_init(
                device,
                match plane {
                    0 => "gsplat-resident-sh-0",
                    1 => "gsplat-resident-sh-1",
                    2 => "gsplat-resident-sh-2",
                    _ => "gsplat-resident-sh-3",
                },
                bytemuck::cast_slice(&staging.sh_planes[plane]),
                16,
            )
        });
        let chunk_metadata_buffer = create_storage_init(
            device,
            "gsplat-resident-chunk-metadata",
            bytemuck::cast_slice(&staging.chunks),
            std::mem::size_of::<ResidentChunkMeta>(),
        );
        let resolved_color_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-resident-resolved-color"),
            size: byte_plan.resolved_color.max(8),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | if cfg!(test) {
                    wgpu::BufferUsages::COPY_SRC
                } else {
                    wgpu::BufferUsages::empty()
                },
            mapped_at_creation: false,
        });
        let draw_params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-resident-draw-params"),
            contents: bytemuck::bytes_of(&GpuSurfaceRenderParams::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let color_params_buffer = create_resident_color_params_buffer(device);

        let color_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-resident-color-bind-group"),
            layout: color_layout,
            entries: &[
                entry(0, &position_alpha_buffer),
                entry(1, &color_auxiliary_buffer),
                entry(2, &sh_buffers[0]),
                entry(3, &sh_buffers[1]),
                entry(4, &sh_buffers[2]),
                entry(5, &sh_buffers[3]),
                entry(6, &chunk_metadata_buffer),
                entry(7, &resolved_color_buffer),
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: color_params_buffer.as_entire_binding(),
                },
            ],
        });

        Ok(Self {
            order_buffer,
            position_alpha_buffer,
            covariance0_buffer,
            covariance1_buffer,
            _color_auxiliary_buffer: color_auxiliary_buffer,
            _sh_buffers: sh_buffers,
            _chunk_metadata_buffer: chunk_metadata_buffer,
            resolved_color_buffer,
            draw_params_buffer,
            color_params_buffer,
            color_bind_group,
            capacity,
            sh_degree: u32::from(scene.sh_degree),
            _byte_plan: byte_plan,
            #[cfg(not(target_arch = "wasm32"))]
            last_resolved_camera_position: None,
            gpu_order: None,
        })
    }

    pub(crate) const fn resident_sh_plane_count(&self) -> u32 {
        self._byte_plan.sh_plane_count
    }

    pub(crate) const fn resident_sh_bytes_per_source(&self) -> u16 {
        (self._byte_plan.sh_plane_count as u16)
            * (std::mem::size_of::<crate::data::ResidentShPlane>() as u16)
    }

    pub fn prepare_cpu_order(
        &self,
        queue: &wgpu::Queue,
        sorted_indices: &[u32],
        camera: &Camera,
        width: u32,
        height: u32,
        upload_order: bool,
    ) -> Result<u32, ResidentGpuError> {
        if sorted_indices.len() > self.capacity {
            return Err(ResidentGpuError::OrderCapacityExceeded);
        }
        if upload_order && !sorted_indices.is_empty() {
            queue.write_buffer(&self.order_buffer, 0, bytemuck::cast_slice(sorted_indices));
        }
        let instance_count = u32::try_from(sorted_indices.len())
            .map_err(|_| ResidentGpuError::AddressSpaceExceeded)?;
        let mut params =
            make_surface_render_params(camera, width, height, instance_count, self.sh_degree);
        params.source_position_stride_words = 4;
        queue.write_buffer(&self.draw_params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(instance_count)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn create_gpu_order_candidate(
        &self,
        device: &wgpu::Device,
    ) -> Result<ResidentGpuSceneOrder, ResidentGpuError> {
        self.create_gpu_order_candidate_with_depth_key_precision(
            device,
            DepthKeyPrecision::ExactFull32,
        )
    }

    pub(crate) fn create_gpu_order_candidate_with_depth_key_precision(
        &self,
        device: &wgpu::Device,
        depth_key_precision: DepthKeyPrecision,
    ) -> Result<ResidentGpuSceneOrder, ResidentGpuError> {
        let count =
            u32::try_from(self.capacity).map_err(|_| ResidentGpuError::AddressSpaceExceeded)?;
        DirectGpuOrder::validate_resident_soa_dispatch_limits(device, count.max(1), count)
            .map_err(|error| ResidentGpuError::GpuOrderInitialization(error.to_string()))?;
        let sorter = DirectGpuOrder::new_resident_soa_with_depth_key_precision(
            device,
            &self.position_alpha_buffer,
            &self.draw_params_buffer,
            count.max(1),
            count,
            depth_key_precision,
        )
        .map_err(|error| ResidentGpuError::GpuOrderInitialization(error.to_string()))?;
        Ok(ResidentGpuSceneOrder { sorter })
    }

    pub(crate) fn publish_gpu_order(&mut self, prepared: ResidentGpuSceneOrder) {
        debug_assert!(self.gpu_order.is_none());
        self.gpu_order = Some(prepared);
    }

    pub(crate) fn gpu_order(&self) -> Option<&ResidentGpuSceneOrder> {
        self.gpu_order.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn gpu_order_depth_key_precision(&self) -> Option<DepthKeyPrecision> {
        self.gpu_order
            .as_ref()
            .map(|order| order.sorter.depth_key_precision())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn create_offscreen_draw_bind_group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
    ) -> wgpu::BindGroup {
        create_resident_draw_bind_group(
            device,
            layout,
            "gsplat-resident-offscreen-draw-bind-group",
            &self.order_buffer,
            &self.position_alpha_buffer,
            &self.covariance0_buffer,
            &self.covariance1_buffer,
            &self.resolved_color_buffer,
            &self.draw_params_buffer,
        )
    }

    /// Encodes one coherent all-point SH resolve if the camera position
    /// changed. Returns whether a compute pass was emitted.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn encode_color_resolve_if_needed(
        &mut self,
        queue: &wgpu::Queue,
        pipeline: &wgpu::ComputePipeline,
        encoder: &mut wgpu::CommandEncoder,
        camera: &Camera,
        max_workgroups_per_dimension: u32,
    ) -> Result<bool, ResidentGpuError> {
        let position = [
            camera.pose.position.x,
            camera.pose.position.y,
            camera.pose.position.z,
        ];
        if self.last_resolved_camera_position == Some(position) {
            return Ok(false);
        }
        self.encode_color_resolve_uncached(
            queue,
            pipeline,
            encoder,
            camera,
            max_workgroups_per_dimension,
        )?;
        self.last_resolved_camera_position = Some(position);
        Ok(true)
    }

    /// Encodes one exact all-point SH resolve without publishing cache state.
    ///
    /// This is the transactional primitive for a caller-owned encoder: if the
    /// encoder is discarded, a retry invokes this method again. Only a layer
    /// that owns submission completion may safely build a persistent cache on
    /// top of it.
    pub(crate) fn encode_color_resolve_uncached(
        &self,
        queue: &wgpu::Queue,
        pipeline: &wgpu::ComputePipeline,
        encoder: &mut wgpu::CommandEncoder,
        camera: &Camera,
        max_workgroups_per_dimension: u32,
    ) -> Result<(), ResidentGpuError> {
        let position = [
            camera.pose.position.x,
            camera.pose.position.y,
            camera.pose.position.z,
        ];
        ResidentColorKernel {
            pipeline,
            bind_group: &self.color_bind_group,
            params_buffer: &self.color_params_buffer,
            splat_count: self.capacity,
            sh_degree: self.sh_degree,
            max_workgroups_per_dimension,
        }
        .encode(queue, encoder, position)?;
        Ok(())
    }
}

fn create_storage_init(
    device: &wgpu::Device,
    label: &'static str,
    bytes: &[u8],
    minimum_size: usize,
) -> wgpu::Buffer {
    const PLACEHOLDER: [u8; 80] = [0; 80];
    debug_assert!(minimum_size <= PLACEHOLDER.len());
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: wgpu_label(label),
        contents: if bytes.is_empty() {
            &PLACEHOLDER[..minimum_size]
        } else {
            bytes
        },
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    })
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(not(target_arch = "wasm32"))]
fn create_resident_draw_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    order: &wgpu::Buffer,
    position_alpha: &wgpu::Buffer,
    covariance0: &wgpu::Buffer,
    covariance1: &wgpu::Buffer,
    resolved_color: &wgpu::Buffer,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label(label),
        layout,
        entries: &[
            entry(0, order),
            entry(1, position_alpha),
            entry(2, covariance0),
            entry(3, covariance1),
            entry(4, resolved_color),
            wgpu::BindGroupEntry {
                binding: 5,
                resource: params.as_entire_binding(),
            },
        ],
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn create_resident_draw_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    create_splat_bind_group_layout(device, "gsplat-resident-draw-bgl", 5)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn create_resident_draw_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    target_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    create_splat_pipeline(
        device,
        bind_group_layout,
        target_format,
        SplatPipeline {
            shader_label: "gsplat-resident-draw-shader",
            shader_source: include_str!("../shaders/splat_surface_resident.wgsl"),
            layout_label: "gsplat-resident-draw-pipeline-layout",
            pipeline_label: "gsplat-resident-draw-pipeline",
            topology: wgpu::PrimitiveTopology::TriangleStrip,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(
        not(target_arch = "wasm32"),
        feature = "diagnostic-resident-sh-mantissa8"
    ))]
    fn read_buffer(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for Resident SH8 readback");
        receiver.recv().expect("map callback").expect("map result");
        let bytes = slice.get_mapped_range().to_vec();
        buffer.unmap();
        bytes
    }

    #[cfg(all(
        not(target_arch = "wasm32"),
        feature = "diagnostic-resident-sh-mantissa8"
    ))]
    fn pack_rgb18e8(rgb: [f32; 3]) -> [u32; 2] {
        let nonnegative = rgb.map(|value| value.max(0.0));
        let maximum = nonnegative.into_iter().fold(0.0_f32, f32::max);
        if maximum == 0.0 {
            return [0, 0];
        }
        let exponent = maximum.log2().ceil().clamp(-126.0, 127.0) as i32;
        let exponent_code = (exponent + 127) as u32;
        let scale = 2.0_f32.powi(exponent);
        let quantize = |value: f32| ((value / scale).clamp(0.0, 1.0) * 262_143.0).round() as u32;
        let q = nonnegative.map(quantize);
        [
            (q[0] & 0x3ffff) | ((q[1] & 0x3fff) << 18),
            ((q[1] >> 14) & 0xf) | ((q[2] & 0x3ffff) << 4) | (exponent_code << 22),
        ]
    }

    #[cfg(all(
        not(target_arch = "wasm32"),
        feature = "diagnostic-resident-sh-mantissa8"
    ))]
    fn unpack_rgb18e8(words: [u32; 2]) -> ([u32; 3], u32) {
        (
            [
                words[0] & 0x3ffff,
                ((words[0] >> 18) & 0x3fff) | ((words[1] & 0xf) << 14),
                (words[1] >> 4) & 0x3ffff,
            ],
            words[1] >> 22,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .ok()?;
            let mut limits = wgpu::Limits::downlevel_defaults();
            limits.max_storage_buffers_per_shader_stage = RESIDENT_COLOR_STORAGE_BINDINGS;
            if !limits.check_limits(&adapter.limits()) {
                return None;
            }
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("resident-gpu-test-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::Performance,
                    trace: wgpu::Trace::Off,
                })
                .await
                .ok()
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn tiny_resident_scene() -> ResidentSceneCpu {
        let source = gsplat_core::SceneBuffers {
            positions: vec![gsplat_core::Vec3f::new(0.0, 0.0, 1.0)],
            opacity: vec![1.0],
            scale_xyz: vec![[-3.0; 3]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.0; 3]],
            sh_degree: 0,
            sh_rest: None,
        };
        ResidentSceneCpu::encode(&source).expect("tiny resident scene")
    }

    #[cfg(all(
        not(target_arch = "wasm32"),
        feature = "diagnostic-resident-sh-mantissa8"
    ))]
    #[test]
    fn production_color_shader_matches_cpu_sh8_decode_and_realizes_three_planes() {
        use crate::data::ShColorLayout;

        let Some((device, queue)) = test_device() else {
            panic!("Resident SH8 CPU/GPU parity requires an available test adapter");
        };
        let count = 3_usize;
        let positions = vec![
            gsplat_core::Vec3f::new(0.0, 0.0, 1.0),
            gsplat_core::Vec3f::new(1.0, 0.0, 2.0),
            gsplat_core::Vec3f::new(-0.5, 1.0, 1.5),
        ];
        let mut sh_rest = vec![0.0_f32; count * 45];
        for (index, value) in sh_rest.iter_mut().enumerate() {
            *value = ((index as f32 * 0.37).sin() * 0.65) + ((index % 5) as f32 - 2.0) * 0.03;
        }
        let source = gsplat_core::SceneBuffers {
            positions: positions.clone(),
            opacity: vec![1.0; count],
            scale_xyz: vec![[-3.0; 3]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[-0.2, 0.1, 0.35], [0.25, -0.15, 0.05], [0.4, 0.2, -0.1]],
            sh_degree: 3,
            sh_rest: Some(sh_rest),
        };
        let resident = ResidentSceneCpu::encode(&source).expect("SH8 resident scene");
        let mut decoded_rest = Vec::with_capacity(count * 45);
        let mut decoded_dc = Vec::with_capacity(count);
        for index in 0..count {
            decoded_dc.push(resident.decode_dc(index));
            for channel in 0..3 {
                decoded_rest.extend(resident.decode_sh_channel(index, channel));
            }
        }
        let decoded = gsplat_core::SceneBuffers {
            positions: positions.clone(),
            color_dc: decoded_dc,
            sh_rest: Some(decoded_rest),
            ..source.clone()
        };

        let color_layout = create_resident_color_bind_group_layout(&device);
        let color_pipeline = create_resident_color_pipeline(&device, &color_layout);
        let resources =
            ResidentGpuResources::new(&device, &color_layout, &resident).expect("GPU resources");
        assert_eq!(
            resources
                ._sh_buffers
                .iter()
                .map(wgpu::Buffer::size)
                .sum::<u64>(),
            48 * count as u64 + 16
        );

        let mut camera = Camera::default();
        camera.pose.position = gsplat_core::Vec3f::new(0.0, 0.0, 0.0);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-sh8-parity-encoder"),
        });
        resources
            .encode_color_resolve_uncached(
                &queue,
                &color_pipeline,
                &mut encoder,
                &camera,
                device.limits().max_compute_workgroups_per_dimension,
            )
            .expect("encode SH8 color resolve");
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-sh8-parity-readback"),
            size: (count * 8) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(
            &resources.resolved_color_buffer,
            0,
            &readback,
            0,
            (count * 8) as u64,
        );
        queue.submit(Some(encoder.finish()));
        let bytes = read_buffer(&device, &readback);
        let actual: Vec<[u32; 2]> = bytes
            .chunks_exact(8)
            .map(|bytes| {
                [
                    u32::from_ne_bytes(bytes[..4].try_into().expect("first word")),
                    u32::from_ne_bytes(bytes[4..].try_into().expect("second word")),
                ]
            })
            .collect();

        let layout = ShColorLayout::new(&decoded);
        let expected: Vec<[u32; 2]> = positions
            .iter()
            .enumerate()
            .map(|(index, position)| {
                let length =
                    (position.x * position.x + position.y * position.y + position.z * position.z)
                        .sqrt();
                let direction = [
                    position.x / length,
                    position.y / length,
                    position.z / length,
                ];
                let rgb = unsafe { crate::sh_color_unchecked(&decoded, index, direction, layout) };
                pack_rgb18e8(rgb)
            })
            .collect();
        for (actual, expected) in actual.into_iter().zip(expected) {
            let (actual_mantissas, actual_exponent) = unpack_rgb18e8(actual);
            let (expected_mantissas, expected_exponent) = unpack_rgb18e8(expected);
            assert_eq!(actual_exponent, expected_exponent);
            for (actual, expected) in actual_mantissas.into_iter().zip(expected_mantissas) {
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "CPU/GPU RGB18E8 mantissa mismatch: actual={actual}, expected={expected}"
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn complete_gpu_order_candidate_stays_unpublished_until_commit() {
        let Some((device, _queue)) = test_device() else {
            return;
        };
        let scene = tiny_resident_scene();
        let color_layout = create_resident_color_bind_group_layout(&device);
        let mut resources =
            ResidentGpuResources::new(&device, &color_layout, &scene).expect("resident resources");

        let candidate = resources
            .create_gpu_order_candidate(&device)
            .expect("candidate");
        assert!(resources.gpu_order().is_none());

        resources.publish_gpu_order(candidate);
        assert!(resources.gpu_order().is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn camera_position_cache_skips_rotation_only_and_empty_scene_resolves() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let color_layout = create_resident_color_bind_group_layout(&device);
        let color_pipeline = create_resident_color_pipeline(&device, &color_layout);
        let max_workgroups = device.limits().max_compute_workgroups_per_dimension;

        for scene in [
            tiny_resident_scene(),
            ResidentSceneCpu::encode(&gsplat_core::SceneBuffers::default())
                .expect("empty resident scene"),
        ] {
            let mut resources = ResidentGpuResources::new(&device, &color_layout, &scene)
                .expect("resident resources");
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("resident-color-cache-test-encoder"),
            });
            let mut camera = Camera::default();

            assert!(
                resources
                    .encode_color_resolve_if_needed(
                        &queue,
                        &color_pipeline,
                        &mut encoder,
                        &camera,
                        max_workgroups,
                    )
                    .expect("first color resolve")
            );
            camera.pose.rotation_xyzw = [0.0, 0.0, 1.0, 0.0];
            assert!(
                !resources
                    .encode_color_resolve_if_needed(
                        &queue,
                        &color_pipeline,
                        &mut encoder,
                        &camera,
                        max_workgroups,
                    )
                    .expect("rotation-only cache reuse")
            );
            camera.pose.position.x = 1.0;
            assert!(
                resources
                    .encode_color_resolve_if_needed(
                        &queue,
                        &color_pipeline,
                        &mut encoder,
                        &camera,
                        max_workgroups,
                    )
                    .expect("moved-camera color resolve")
            );
            queue.submit(Some(encoder.finish()));
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("resident color cache test poll");
        }
    }
}
