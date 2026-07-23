//! Rust layouts shared with render and compute kernels.

use std::mem::{align_of, offset_of, size_of};

use bytemuck::{Pod, Zeroable};

pub const RESIDENT_CHUNK_SPLATS: usize = 256;
pub const RESIDENT_COVARIANCE0_FLOATS: usize = 4;
pub const RESIDENT_COVARIANCE1_FLOATS: usize = 2;
pub const RESIDENT_COLOR_AUX_WORDS: usize = 2;
pub const RESIDENT_SH_PLANES: usize = 4;
pub const RESIDENT_SH_WORDS_PER_PLANE: usize = 4;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ResidentPositionAlpha {
    pub position_alpha: [f32; 4],
}

const _: () = {
    assert!(size_of::<ResidentPositionAlpha>() == 16);
    assert!(align_of::<ResidentPositionAlpha>() == 4);
    assert!(offset_of!(ResidentPositionAlpha, position_alpha) == 0);
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ResidentCovariance0 {
    /// Canonical world-covariance terms xx, xy, xz and yy.
    pub values: [f32; RESIDENT_COVARIANCE0_FLOATS],
}

const _: () = {
    assert!(size_of::<ResidentCovariance0>() == 16);
    assert!(align_of::<ResidentCovariance0>() == 4);
    assert!(offset_of!(ResidentCovariance0, values) == 0);
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ResidentCovariance1 {
    /// Canonical world-covariance terms yz and zz.
    pub values: [f32; RESIDENT_COVARIANCE1_FLOATS],
}

const _: () = {
    assert!(size_of::<ResidentCovariance1>() == 8);
    assert!(align_of::<ResidentCovariance1>() == 4);
    assert!(offset_of!(ResidentCovariance1, values) == 0);
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub struct ResidentColorAux {
    /// Chunk-local SH DC u16x3. The high half of the second word is reserved.
    pub words: [u32; RESIDENT_COLOR_AUX_WORDS],
}

const _: () = {
    assert!(size_of::<ResidentColorAux>() == 8);
    assert!(align_of::<ResidentColorAux>() == 4);
    assert!(offset_of!(ResidentColorAux, words) == 0);
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub struct ResidentShPlane {
    /// Four packed words. Across all four planes, 45 SH3 values use signed
    /// 11-bit quantization in coefficient-major RGB order. Three 5-bit
    /// per-point band-scale ratios occupy bits 495 through 509; the final two
    /// bits stay reserved. Lower degrees place their active scale ratios
    /// immediately after their last coefficient in the final active plane.
    pub words: [u32; RESIDENT_SH_WORDS_PER_PLANE],
}

const _: () = {
    assert!(size_of::<ResidentShPlane>() == 16);
    assert!(align_of::<ResidentShPlane>() == 4);
    assert!(offset_of!(ResidentShPlane, words) == 0);
};

/// Five `vec4<f32>` values. The fourth lane is reserved and kept zero so the
/// Rust layout exactly matches WGSL storage-buffer alignment.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ResidentChunkMeta {
    pub dc_min: [f32; 4],
    pub dc_extent: [f32; 4],
    pub sh_scale_l1: [f32; 4],
    pub sh_scale_l2: [f32; 4],
    pub sh_scale_l3: [f32; 4],
}

const _: () = {
    assert!(size_of::<ResidentChunkMeta>() == 80);
    assert!(align_of::<ResidentChunkMeta>() == 4);
    assert!(offset_of!(ResidentChunkMeta, dc_min) == 0);
    assert!(offset_of!(ResidentChunkMeta, dc_extent) == 16);
    assert!(offset_of!(ResidentChunkMeta, sh_scale_l1) == 32);
    assert!(offset_of!(ResidentChunkMeta, sh_scale_l2) == 48);
    assert!(offset_of!(ResidentChunkMeta, sh_scale_l3) == 64);
};

pub const RESIDENT_CHUNK_META_BYTES: usize = size_of::<ResidentChunkMeta>();

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GpuInstance {
    /// xy = center in NDC, zw = major axis in NDC.
    pub center_and_axis_u: [f32; 4],
    /// xy = minor axis in NDC, zw = reserved.
    pub axis_v_and_pad: [f32; 4],
    /// Premultiplied RGB + alpha.
    pub color_rgba: [f32; 4],
}

const _: () = {
    assert!(size_of::<GpuInstance>() == 48);
    assert!(align_of::<GpuInstance>() == 4);
    assert!(offset_of!(GpuInstance, center_and_axis_u) == 0);
    assert!(offset_of!(GpuInstance, axis_v_and_pad) == 16);
    assert!(offset_of!(GpuInstance, color_rgba) == 32);
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuSurfaceSourceElem {
    pub(crate) position: [f32; 4],
    pub(crate) covariance0: [f32; 4],
    pub(crate) covariance1: [f32; 4],
    pub(crate) color_dc: [f32; 4],
}

const _: () = {
    assert!(size_of::<GpuSurfaceSourceElem>() == 64);
    assert!(align_of::<GpuSurfaceSourceElem>() == 4);
    assert!(offset_of!(GpuSurfaceSourceElem, position) == 0);
    assert!(offset_of!(GpuSurfaceSourceElem, covariance0) == 16);
    assert!(offset_of!(GpuSurfaceSourceElem, covariance1) == 32);
    assert!(offset_of!(GpuSurfaceSourceElem, color_dc) == 48);
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuSurfaceRenderParams {
    pub(crate) camera_pos: [f32; 4],
    pub(crate) view_rot_row0: [f32; 4],
    pub(crate) view_rot_row1: [f32; 4],
    pub(crate) view_rot_row2: [f32; 4],
    pub(crate) vertical_fov_radians: f32,
    pub(crate) near_plane: f32,
    pub(crate) far_plane: f32,
    pub(crate) aspect: f32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) sh_degree: u32,
    pub(crate) len: u32,
    pub(crate) order_stride_words: u32,
    pub(crate) order_id_offset_words: u32,
    pub(crate) source_position_stride_words: u32,
    pub(crate) source_position_offset_words: u32,
}

const _: () = {
    assert!(size_of::<GpuSurfaceRenderParams>() == 112);
    assert!(align_of::<GpuSurfaceRenderParams>() == 4);
    assert!(offset_of!(GpuSurfaceRenderParams, camera_pos) == 0);
    assert!(offset_of!(GpuSurfaceRenderParams, view_rot_row0) == 16);
    assert!(offset_of!(GpuSurfaceRenderParams, view_rot_row1) == 32);
    assert!(offset_of!(GpuSurfaceRenderParams, view_rot_row2) == 48);
    assert!(offset_of!(GpuSurfaceRenderParams, vertical_fov_radians) == 64);
    assert!(offset_of!(GpuSurfaceRenderParams, near_plane) == 68);
    assert!(offset_of!(GpuSurfaceRenderParams, far_plane) == 72);
    assert!(offset_of!(GpuSurfaceRenderParams, aspect) == 76);
    assert!(offset_of!(GpuSurfaceRenderParams, width) == 80);
    assert!(offset_of!(GpuSurfaceRenderParams, height) == 84);
    assert!(offset_of!(GpuSurfaceRenderParams, sh_degree) == 88);
    assert!(offset_of!(GpuSurfaceRenderParams, len) == 92);
    assert!(offset_of!(GpuSurfaceRenderParams, order_stride_words) == 96);
    assert!(offset_of!(GpuSurfaceRenderParams, order_id_offset_words) == 100);
    assert!(offset_of!(GpuSurfaceRenderParams, source_position_stride_words) == 104);
    assert!(offset_of!(GpuSurfaceRenderParams, source_position_offset_words) == 108);
};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct GpuSortPair {
    pub(crate) key: u32,
    pub(crate) id: u32,
}

const _: () = {
    assert!(size_of::<GpuSortPair>() == 8);
    assert!(align_of::<GpuSortPair>() == 4);
    assert!(offset_of!(GpuSortPair, key) == 0);
    assert!(offset_of!(GpuSortPair, id) == 4);
};
