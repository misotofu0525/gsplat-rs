struct QuantizedSource {
  pos_xy: u32,
  pos_z_alpha: u32,
  rotation: u32,
  scale_rgb: u32,
  color_dc: u32,
  _pad0: u32,
  _pad1: u32,
  _pad2: u32,
};

struct RenderParams {
  camera_pos: vec4<f32>,
  view_rot_row0: vec4<f32>,
  view_rot_row1: vec4<f32>,
  view_rot_row2: vec4<f32>,
  vertical_fov_radians: f32,
  near_plane: f32,
  far_plane: f32,
  aspect: f32,
  width: u32,
  height: u32,
  sh_degree: u32,
  len: u32,
  order_stride_words: u32,
  order_id_offset_words: u32,
  _render_pad0: u32,
  _render_pad1: u32,
};

struct SortPair {
  key: u32,
  id: u32,
};

@group(0) @binding(0)
var<storage, read> key_source: array<QuantizedSource>;
@group(0) @binding(1)
var<uniform> render_params: RenderParams;
@group(0) @binding(2)
var<storage, read_write> generated_pairs: array<SortPair>;

const WORKGROUP_SIZE: u32 = 64u;
const ITEMS_PER_THREAD: u32 = 4u;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;

@compute @workgroup_size(64)
fn generate_pairs(
  @builtin(local_invocation_id) local_id3: vec3<u32>,
  @builtin(workgroup_id) group_id3: vec3<u32>,
) {
  let first = group_id3.x * TILE_SIZE + local_id3.x * ITEMS_PER_THREAD;
  for (var item = 0u; item < ITEMS_PER_THREAD; item += 1u) {
    let index = first + item;
    if (index < render_params.len) {
      let pos_xy = unpack2x16float(key_source[index].pos_xy);
      let pos_za = unpack2x16float(key_source[index].pos_z_alpha);
      let position = vec3<f32>(pos_xy.x, pos_xy.y, pos_za.x);
      let relative = position - render_params.camera_pos.xyz;
      let depth = dot(render_params.view_rot_row2.xyz, relative);
      var key = 0u;
      if (depth >= render_params.near_plane && depth <= render_params.far_plane) {
        key = bitcast<u32>(max(depth, 0.0));
      }
      generated_pairs[index] = SortPair(key, index);
    }
  }
}
