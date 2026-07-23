// Stable post-projection compaction for exact Resident hardware quads.

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

@group(0) @binding(0)
var<storage, read> projected_center_source: array<vec4<f32>>;
@group(0) @binding(1)
var<storage, read> contributor_group_offsets: array<u32>;
@group(0) @binding(2)
var<storage, read_write> contributor_ranks: array<u32>;
@group(0) @binding(3)
var<storage, read_write> contributor_args: DrawIndirectArgs;
@group(0) @binding(4)
var<uniform> params: RenderParams;

const WORKGROUP_SIZE: u32 = 128u;
const MASK_WORD_COUNT: u32 = WORKGROUP_SIZE / 32u;
const ALPHA_THRESHOLD: f32 = 1.0 / 255.0;

var<workgroup> contributor_masks: array<atomic<u32>, 4>;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
}

@compute @workgroup_size(128)
fn compact_contributor_ranks(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  let capacity_group_count =
    (params.len + WORKGROUP_SIZE - 1u) / WORKGROUP_SIZE;
  // Keep rectangular Dispatch2d overshoot out of both group offsets and the
  // rank plane. The predicate is uniform for the whole workgroup, so it is
  // safe before the first workgroup barrier.
  if (group >= capacity_group_count) {
    return;
  }
  let rank = group * WORKGROUP_SIZE + lane;
  let word = lane >> 5u;
  let bit = lane & 31u;
  let lower_lane_mask = (1u << bit) - 1u;

  if (lane < MASK_WORD_COUNT) {
    atomicStore(&contributor_masks[lane], 0u);
  }
  workgroupBarrier();

  var contributes = false;
  if (rank < params.len) {
    let alpha = projected_center_source[rank].z;
    // Invalid projected entries use alpha zero. NaN stays fail-open so the
    // optimization never silently removes an ambiguous source value.
    contributes = !(alpha < ALPHA_THRESHOLD);
    if (contributes) {
      atomicOr(&contributor_masks[word], 1u << bit);
    }
  }
  workgroupBarrier();

  if (contributes) {
    var local_rank = 0u;
    for (var earlier_word = 0u; earlier_word < word; earlier_word += 1u) {
      local_rank += countOneBits(atomicLoad(&contributor_masks[earlier_word]));
    }
    local_rank += countOneBits(
      atomicLoad(&contributor_masks[word]) & lower_lane_mask,
    );
    contributor_ranks[contributor_group_offsets[group] + local_rank] = rank;
  }
}

@compute @workgroup_size(1)
fn finalize_contributor_args() {
  let sentinel = arrayLength(&contributor_group_offsets) - 1u;
  atomicStore(
    &contributor_args.instance_count,
    contributor_group_offsets[sentinel],
  );
}
