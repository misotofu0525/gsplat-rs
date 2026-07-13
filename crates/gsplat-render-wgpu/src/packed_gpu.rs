//! GPU resources for the Phase B packed atlas path.

use bytemuck::{Pod, Zeroable};
use gsplat_core::Camera;
use wgpu::util::DeviceExt;

use crate::packed_atlas::{
    HOT_RECORD_BYTES, HOT_RECORD_U32_WORDS, PackedAtlasCpuBuffers, PackedSceneCpu,
    atlas_dimensions, pack_scene, sh_sidecar_atlas_dimensions,
};
use crate::{
    DirectSceneError, GpuSurfaceRenderParams, PackedScenePath, make_surface_render_params,
    packed_scene_preflight, wgpu_label,
};

pub const PACKED_QUAD_VERTEX_COUNT: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GpuPackedRenderParams {
    camera_pos: [f32; 4],
    view_rot_row0: [f32; 4],
    view_rot_row1: [f32; 4],
    view_rot_row2: [f32; 4],
    bounds_min: [f32; 4],
    bounds_extent: [f32; 4],
    sh_scales: [f32; 4],
    log_scale_min: f32,
    log_scale_extent: f32,
    vertical_fov_radians: f32,
    near_plane: f32,
    far_plane: f32,
    aspect: f32,
    width: u32,
    height: u32,
    sh_degree: u32,
    len: u32,
    atlas_width: u32,
    _pad0: u32,
}

pub struct PackedAtlasResources {
    pub sorted_indices_buffer: wgpu::Buffer,
    pub params_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub capacity: usize,
    pub atlas_width: u32,
    #[allow(dead_code)]
    pub atlas_height: u32,
    pub splat_count: usize,
    #[allow(dead_code)]
    /// Exact bytes requested by the hot-buffer and SH-texture descriptors.
    /// Driver-private allocator padding is not observable through wgpu.
    pub declared_attribute_resource_bytes: u64,
    #[allow(dead_code)]
    pub measured_hot_storage_bytes: u64,
    pub sh_degree: u32,
    pub bounds_min: [f32; 3],
    pub bounds_extent: [f32; 3],
    pub log_scale_min: f32,
    pub log_scale_extent: f32,
    pub sh_scales: [f32; 3],
    hot_buffer: wgpu::Buffer,
    /// CPU mirror of tightly packed hot words; color refresh mutates word 4.
    hot_words: Vec<u32>,
    /// Retained for degree-3 byte accounting / future streaming residency.
    _sh_coeffs: wgpu::Texture,
}

fn create_uint_texture(
    device: &wgpu::Device,
    label: &'static str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: wgpu_label(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_texture_u8_rgba(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    data: &[u8],
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

/// SH sidecar stored as tightly packed RGBA8Uint texels (12 texels = 48 bytes
/// per splat) in row-major order across a 2048-wide atlas.
fn sh_sidecar_atlas_size(splat_count: usize) -> (u32, u32) {
    sh_sidecar_atlas_dimensions(splat_count)
}

impl PackedAtlasResources {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        packed: &PackedSceneCpu,
    ) -> Result<Self, DirectSceneError> {
        let limits = device.limits();
        let preflight = packed_scene_preflight(
            packed.splat_count,
            packed.sh_degree,
            limits.max_texture_dimension_2d,
            u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size),
        )?;
        if preflight.path != PackedScenePath::PackedAtlas
            || !preflight.sorted_indices_fits_storage_binding
            || !preflight.hot_record_fits_storage_binding
        {
            return Err(DirectSceneError::PackedResourceLimitExceeded(Box::new(
                preflight,
            )));
        }
        let (width, height) = atlas_dimensions(packed.splat_count);
        let capacity = packed.splat_count.max(1);
        let mut hot_words = PackedAtlasCpuBuffers::hot_storage_words(packed);
        if hot_words.is_empty() {
            // wgpu rejects zero-sized buffers; keep one empty hot record slot.
            hot_words.resize(HOT_RECORD_U32_WORDS, 0);
        }
        let hot_storage_bytes = (capacity as u64) * (HOT_RECORD_BYTES as u64);

        let sorted_indices_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-packed-sorted-indices"),
            size: (capacity as u64) * (std::mem::size_of::<u32>() as u64),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-packed-params"),
            contents: bytemuck::bytes_of(&GpuPackedRenderParams::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Hot draw path uses a tightly packed storage buffer (20 B/splat:
        // pos/opacity + three u10 log scales/flags + rotation + color). This
        // stays under the common 128 MiB storage-binding limit through
        // Nandi-scale hot records while avoiding multi-texture decode on
        // mobile TBDR GPUs.
        // Degree-3 SH remains out of storage bindings (CPU refresh / texture sidecar).
        let hot_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-packed-hot-records"),
            contents: bytemuck::cast_slice(&hot_words),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let (sh_width, sh_height) = sh_sidecar_atlas_size(packed.splat_count);
        let sh_coeffs = create_uint_texture(
            device,
            "gsplat-packed-sh-coeffs",
            sh_width,
            sh_height,
            wgpu::TextureFormat::Rgba8Uint,
        );

        // Expand 48-byte sidecars into 12 consecutive RGBA8Uint texels per slot.
        let mut sh_texels = vec![0_u8; (sh_width as usize) * (sh_height as usize) * 4];
        for slot in 0..packed.splat_count {
            for texel in 0..12_usize {
                let linear = slot * 12 + texel;
                let x = linear as u32 % sh_width;
                let y = linear as u32 / sh_width;
                let dst_index = (y * sh_width + x) as usize * 4;
                let sidecar = packed.sh_sidecars[slot].as_texel_bytes();
                sh_texels[dst_index..dst_index + 4]
                    .copy_from_slice(&sidecar[texel * 4..texel * 4 + 4]);
            }
        }
        write_texture_u8_rgba(queue, &sh_coeffs, sh_width, sh_height, &sh_texels);

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-packed-bind-group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: sorted_indices_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: hot_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });

        let sh_scales = packed.sh_scales;
        let bounds_extent = [
            packed.bounds.max[0] - packed.bounds.min[0],
            packed.bounds.max[1] - packed.bounds.min[1],
            packed.bounds.max[2] - packed.bounds.min[2],
        ];

        Ok(Self {
            sorted_indices_buffer,
            params_buffer,
            bind_group,
            capacity,
            atlas_width: width,
            atlas_height: height,
            splat_count: packed.splat_count,
            declared_attribute_resource_bytes: hot_storage_bytes
                + u64::from(sh_width) * u64::from(sh_height) * 4,
            measured_hot_storage_bytes: hot_storage_bytes,
            sh_degree: u32::from(packed.sh_degree),
            bounds_min: packed.bounds.min,
            bounds_extent,
            log_scale_min: packed.log_scale_range.min,
            log_scale_extent: (packed.log_scale_range.max - packed.log_scale_range.min).max(1e-6),
            sh_scales,
            hot_buffer,
            hot_words,
            _sh_coeffs: sh_coeffs,
        })
    }

    pub fn from_scene(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        scene: &gsplat_core::SceneBuffers,
    ) -> Result<Self, DirectSceneError> {
        let packed = pack_scene(scene);
        Self::new(device, queue, bind_group_layout, &packed)
    }

    /// Upload view-evaluated RGB10 colors into the hot color word (index 4).
    #[allow(dead_code)]
    pub fn write_hot_colors(&mut self, queue: &wgpu::Queue, colors: &[u32]) {
        self.write_hot_colors_range(queue, colors, 0, colors.len());
    }

    /// Upload a half-open splat range `[start, end)` into the hot color words.
    pub fn write_hot_colors_range(
        &mut self,
        queue: &wgpu::Queue,
        colors: &[u32],
        start: usize,
        end: usize,
    ) {
        if colors.len() != self.splat_count || start > end || end > self.splat_count {
            debug_assert_eq!(colors.len(), self.splat_count);
            debug_assert!(start <= end && end <= self.splat_count);
            return;
        }
        if start == end {
            return;
        }
        self.write_hot_colors_at(queue, start, &colors[start..end]);
    }

    /// Upload view-evaluated RGB10 colors at a global atlas-slot offset.
    pub fn write_hot_colors_at(&mut self, queue: &wgpu::Queue, global_slot: usize, colors: &[u32]) {
        let end = global_slot.saturating_add(colors.len());
        if colors.is_empty() || end > self.capacity {
            debug_assert!(end <= self.capacity);
            return;
        }
        // Hot layout is 5×u32: pos0, pos1, scale/flags, rotation, color.
        const COLOR_WORD: usize = 4;
        for (offset, &color) in colors.iter().enumerate() {
            self.hot_words[(global_slot + offset) * HOT_RECORD_U32_WORDS + COLOR_WORD] = color;
        }
        let word_start = global_slot * HOT_RECORD_U32_WORDS;
        let word_end = end * HOT_RECORD_U32_WORDS;
        let byte_offset = (word_start * std::mem::size_of::<u32>()) as u64;
        queue.write_buffer(
            &self.hot_buffer,
            byte_offset,
            bytemuck::cast_slice(&self.hot_words[word_start..word_end]),
        );
    }

    /// Upload tightly packed hot records starting at `global_slot`.
    ///
    /// `words` must contain `splat_count * HOT_RECORD_U32_WORDS` values.
    pub fn write_hot_records_at(&mut self, queue: &wgpu::Queue, global_slot: usize, words: &[u32]) {
        if words.is_empty() {
            return;
        }
        debug_assert_eq!(words.len() % HOT_RECORD_U32_WORDS, 0);
        let splat_count = words.len() / HOT_RECORD_U32_WORDS;
        let end = global_slot.saturating_add(splat_count);
        if end > self.capacity {
            debug_assert!(end <= self.capacity);
            return;
        }
        let word_start = global_slot * HOT_RECORD_U32_WORDS;
        let word_end = end * HOT_RECORD_U32_WORDS;
        self.hot_words[word_start..word_end].copy_from_slice(words);
        let byte_offset = (word_start * std::mem::size_of::<u32>()) as u64;
        queue.write_buffer(
            &self.hot_buffer,
            byte_offset,
            bytemuck::cast_slice(&self.hot_words[word_start..word_end]),
        );
    }

    /// Clear a half-open global slot range to zeros.
    pub fn clear_hot_records_range(
        &mut self,
        queue: &wgpu::Queue,
        global_slot_start: usize,
        splat_count: usize,
    ) {
        if splat_count == 0 {
            return;
        }
        let end = global_slot_start.saturating_add(splat_count);
        if end > self.capacity {
            debug_assert!(end <= self.capacity);
            return;
        }
        let word_start = global_slot_start * HOT_RECORD_U32_WORDS;
        let word_end = end * HOT_RECORD_U32_WORDS;
        for word in &mut self.hot_words[word_start..word_end] {
            *word = 0;
        }
        let byte_offset = (word_start * std::mem::size_of::<u32>()) as u64;
        queue.write_buffer(
            &self.hot_buffer,
            byte_offset,
            bytemuck::cast_slice(&self.hot_words[word_start..word_end]),
        );
    }

    pub fn prepare(
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
        let base =
            make_surface_render_params(camera, width, height, instance_count, self.sh_degree);
        let params = GpuPackedRenderParams {
            camera_pos: base.camera_pos,
            view_rot_row0: base.view_rot_row0,
            view_rot_row1: base.view_rot_row1,
            view_rot_row2: base.view_rot_row2,
            bounds_min: [
                self.bounds_min[0],
                self.bounds_min[1],
                self.bounds_min[2],
                0.0,
            ],
            bounds_extent: [
                self.bounds_extent[0],
                self.bounds_extent[1],
                self.bounds_extent[2],
                0.0,
            ],
            sh_scales: [self.sh_scales[0], self.sh_scales[1], self.sh_scales[2], 0.0],
            log_scale_min: self.log_scale_min,
            log_scale_extent: self.log_scale_extent,
            vertical_fov_radians: base.vertical_fov_radians,
            near_plane: base.near_plane,
            far_plane: base.far_plane,
            aspect: base.aspect,
            width: base.width,
            height: base.height,
            sh_degree: base.sh_degree,
            len: base.len,
            atlas_width: self.atlas_width,
            _pad0: 0,
        };
        let _ = GpuSurfaceRenderParams::zeroed();
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(instance_count)
    }
}

pub fn create_packed_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label("gsplat-packed-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

pub fn create_packed_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    target_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: wgpu_label("gsplat-packed-shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../shaders/splat_surface_packed.wgsl").into(),
        ),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: wgpu_label("gsplat-packed-pipeline-layout"),
        bind_group_layouts: &[bind_group_layout],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: wgpu_label("gsplat-packed-pipeline"),
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
                format: target_format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
