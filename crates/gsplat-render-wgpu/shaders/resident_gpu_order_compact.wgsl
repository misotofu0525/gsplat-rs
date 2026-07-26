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
  source_position_stride_words: u32,
  source_position_offset_words: u32,
};

struct DrawIndirectArgs {
  vertex_count: u32,
  instance_count: atomic<u32>,
  first_vertex: u32,
  first_instance: u32,
};

struct ResidentOrderControl {
  visible_count: u32,
  active_group_count: u32,
  dispatch_x: u32,
  dispatch_y: u32,
  dispatch_z: u32,
  _pad0: u32,
  _pad1: u32,
  _pad2: u32,
};

@group(0) @binding(0)
var<storage, read> key_source: array<u32>;
@group(0) @binding(1)
var<uniform> render_params: RenderParams;
@group(0) @binding(2)
var<storage, read_write> raw_keys: array<u32>;
@group(0) @binding(3)
var<storage, read_write> group_offsets: array<u32>;
@group(0) @binding(4)
var<storage, read_write> compact_keys: array<u32>;
@group(0) @binding(5)
var<storage, read_write> compact_ids: array<u32>;
@group(0) @binding(6)
var<storage, read_write> order_control: ResidentOrderControl;
@group(0) @binding(9)
var<storage, read_write> draw_args: DrawIndirectArgs;

const WORKGROUP_SIZE: u32 = 128u;
const ITEMS_PER_THREAD: u32 = 8u;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const MASK_WORD_COUNT: u32 = WORKGROUP_SIZE / 32u;

// Exact is the product default. B1 contract tests compile the same generator
// with 8 to retain a stable high-24 key without changing visibility or depth.
override DEPTH_KEY_LOW_BITS_TO_CLEAR: u32 = 0u;

var<workgroup> group_visible_count: atomic<u32>;
var<workgroup> visible_masks: array<atomic<u32>, 4>;
var<workgroup> visible_prior: u32;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
}

fn source_group_count() -> u32 {
  return (render_params.len + TILE_SIZE - 1u) / TILE_SIZE;
}

// Keep the depth-key contract byte-identical to CPU preprocessing and the
// Direct key generator.
fn canonical_depth(left: vec3<f32>, right: vec3<f32>) -> f32 {
  let xy = fma(left.y, right.y, left.x * right.x);
  return fma(left.z, right.z, xy);
}

fn visible_depth_key(depth: f32) -> u32 {
  let bits = bitcast<u32>(max(depth, 0.0));
  if (DEPTH_KEY_LOW_BITS_TO_CLEAR == 0u) {
    return bits;
  }
  let retained = max(bits >> DEPTH_KEY_LOW_BITS_TO_CLEAR, 1u);
  return retained << DEPTH_KEY_LOW_BITS_TO_CLEAR;
}

// Phase 1 writes one key per source element and only one count per 1024-source
// group. There is no N-element flag scan and no cross-workgroup atomic count.
@compute @workgroup_size(128)
fn generate_keys_and_group_counts(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= source_group_count()) {
    return;
  }

  if (lane == 0u) {
    atomicStore(&group_visible_count, 0u);
  }
  workgroupBarrier();

  var local_visible = 0u;
  let tile_begin = group * TILE_SIZE;
  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    let index = tile_begin + round * WORKGROUP_SIZE + lane;
    if (index < render_params.len) {
      let position_word = index * render_params.source_position_stride_words
        + render_params.source_position_offset_words;
      let position = vec3<f32>(
        bitcast<f32>(key_source[position_word]),
        bitcast<f32>(key_source[position_word + 1u]),
        bitcast<f32>(key_source[position_word + 2u]),
      );
      let relative = position - render_params.camera_pos.xyz;
      let depth = canonical_depth(render_params.view_rot_row2.xyz, relative);
      var key = 0u;
      if (depth >= render_params.near_plane && depth <= render_params.far_plane) {
        key = visible_depth_key(depth);
        local_visible += 1u;
      }
      raw_keys[index] = key;
    }
  }
  if (local_visible > 0u) {
    atomicAdd(&group_visible_count, local_visible);
  }
  workgroupBarrier();
  if (lane == 0u) {
    group_offsets[group] = atomicLoad(&group_visible_count);
  }
}

// Phase 2 consumes the exclusive scan of group counts. Within a group, the
// four-word visibility mask and round carry preserve exact source order.
@compute @workgroup_size(128)
fn compact_visible_keys_ids(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= source_group_count()) {
    return;
  }

  if (lane == 0u) {
    visible_prior = 0u;
  }
  workgroupBarrier();

  let tile_begin = group * TILE_SIZE;
  let word = lane >> 5u;
  let bit = lane & 31u;
  let lower_lane_mask = (1u << bit) - 1u;
  let group_base = group_offsets[group];

  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    if (lane < MASK_WORD_COUNT) {
      atomicStore(&visible_masks[lane], 0u);
    }
    workgroupBarrier();

    let index = tile_begin + round * WORKGROUP_SIZE + lane;
    var key = 0u;
    var visible = false;
    if (index < render_params.len) {
      key = raw_keys[index];
      visible = key != 0u;
      if (visible) {
        atomicOr(&visible_masks[word], 1u << bit);
      }
    }
    workgroupBarrier();

    if (visible) {
      var local_rank = visible_prior;
      for (var earlier_word = 0u; earlier_word < word; earlier_word += 1u) {
        local_rank += countOneBits(atomicLoad(&visible_masks[earlier_word]));
      }
      local_rank += countOneBits(
        atomicLoad(&visible_masks[word]) & lower_lane_mask,
      );
      let output_index = group_base + local_rank;
      compact_keys[output_index] = key;
      compact_ids[output_index] = index;
    }
    workgroupBarrier();

    if (lane == 0u) {
      var round_count = 0u;
      for (var mask_word = 0u; mask_word < MASK_WORD_COUNT; mask_word += 1u) {
        round_count += countOneBits(atomicLoad(&visible_masks[mask_word]));
      }
      visible_prior += round_count;
    }
    workgroupBarrier();
  }
}

// The scanned sentinel at group_offsets[group_count] is the exact visible
// count. Publish both draw and radix-indirect arguments without CPU readback.
@compute @workgroup_size(1)
fn finalize_visible_compaction() {
  let visible_count = group_offsets[source_group_count()];
  let active_group_count = (visible_count + TILE_SIZE - 1u) / TILE_SIZE;
  order_control.visible_count = visible_count;
  order_control.active_group_count = active_group_count;
  order_control.dispatch_x = active_group_count;
  order_control.dispatch_y = 1u;
  order_control.dispatch_z = 1u;
  atomicStore(&draw_args.instance_count, visible_count);
}
