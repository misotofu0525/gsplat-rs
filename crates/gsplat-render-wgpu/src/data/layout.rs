//! Rust layouts shared with render and compute kernels.

use std::mem::{align_of, offset_of, size_of};

use bytemuck::{Pod, Zeroable};

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
