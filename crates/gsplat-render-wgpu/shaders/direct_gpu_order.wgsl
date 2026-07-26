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

struct SortPair {
  key: u32,
  id: u32,
};

struct PassParams {
  shift: u32,
  count: u32,
  group_count: u32,
  _pad: u32,
};

struct DrawIndirectArgs {
  vertex_count: u32,
  instance_count: atomic<u32>,
  first_vertex: u32,
  first_instance: u32,
};

@group(0) @binding(0)
var<storage, read> key_source: array<u32>;
@group(0) @binding(1)
var<uniform> render_params: RenderParams;
@group(0) @binding(2)
var<storage, read_write> generated_keys: array<u32>;
@group(0) @binding(3)
var<storage, read_write> generated_ids: array<u32>;
@group(0) @binding(9)
var<storage, read_write> draw_args: DrawIndirectArgs;

@group(0) @binding(4)
var<storage, read> radix_keys_src: array<u32>;
@group(0) @binding(5)
var<storage, read> radix_payload_src: array<u32>;
@group(0) @binding(6)
var<storage, read_write> radix_payload_dst: array<u32>;
@group(0) @binding(7)
var<storage, read_write> radix_prefix: array<u32>;
@group(0) @binding(8)
var<uniform> pass_params: PassParams;

@group(0) @binding(10)
var<storage, read> packed_keys: array<u32>;
@group(0) @binding(11)
var<storage, read> packed_ids: array<u32>;
@group(0) @binding(12)
var<storage, read_write> packed_pairs: array<SortPair>;
@group(0) @binding(13)
var<storage, read_write> radix_keys_dst: array<u32>;

const WORKGROUP_SIZE: u32 = 128u;
const ITEMS_PER_THREAD: u32 = 8u;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const RADIX: u32 = 16u;
const MASK_WORDS_PER_DIGIT: u32 = WORKGROUP_SIZE / 32u;
const MASK_WORD_COUNT: u32 = RADIX * MASK_WORDS_PER_DIGIT;

// Exact is the product default. B1 contract tests compile the same generator
// with 8 to retain a stable high-24 key without changing visibility or depth.
override DEPTH_KEY_LOW_BITS_TO_CLEAR: u32 = 0u;

// Histogram uses only workgroup atomics. Every logical group writes a unique
// digit-major row in radix_prefix, so no cross-workgroup synchronization is
// needed. The prefix buffer is scanned in-place by a separate pipeline.
var<workgroup> histogram_counts: array<atomic<u32>, 16>;

// Stable scatter ranks one round of 128 input elements at a time. The mask
// for a digit records which lanes contain that digit; popcount of earlier
// words/bits gives the rank within the round, while digit_prior contains all
// matching elements from earlier rounds in this tile.
var<workgroup> digit_masks: array<atomic<u32>, MASK_WORD_COUNT>;
var<workgroup> digit_prior: array<u32, 16>;
var<workgroup> visible_in_group: atomic<u32>;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
}

fn descending_digit_slot(key: u32) -> u32 {
  let digit = (key >> pass_params.shift) & 15u;
  return 15u - digit;
}

// Keep the depth-key contract independent of a backend's native `dot`
// contraction choice. CPU preprocessing uses the identical x-product then
// y/z fused-multiply-add sequence.
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

@compute @workgroup_size(128)
fn generate_pairs(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  let tile_begin = group * TILE_SIZE;
  if (lane == 0u) {
    atomicStore(&visible_in_group, 0u);
  }
  workgroupBarrier();

  var local_visible = 0u;

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
        // Positive finite IEEE-754 values have the same ordering as their bits.
        key = visible_depth_key(depth);
        local_visible += 1u;
      }
      generated_keys[index] = key;
      generated_ids[index] = index;
    }
  }
  if (local_visible > 0u) {
    atomicAdd(&visible_in_group, local_visible);
  }
  workgroupBarrier();
  if (lane == 0u) {
    atomicAdd(&draw_args.instance_count, atomicLoad(&visible_in_group));
  }
}

@compute @workgroup_size(128)
fn histogram(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= pass_params.group_count) {
    return;
  }

  if (lane < RADIX) {
    atomicStore(&histogram_counts[lane], 0u);
  }
  workgroupBarrier();

  let tile_begin = group * TILE_SIZE;
  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    let index = tile_begin + round * WORKGROUP_SIZE + lane;
    if (index < pass_params.count) {
      let digit_slot = descending_digit_slot(radix_keys_src[index]);
      atomicAdd(&histogram_counts[digit_slot], 1u);
    }
  }
  workgroupBarrier();

  if (lane < RADIX) {
    radix_prefix[lane * pass_params.group_count + group] =
      atomicLoad(&histogram_counts[lane]);
  }
}

// Rust dispatches this once with keys as the payload and once with IDs. That
// keeps the SoA buffers while staying within the four-storage-buffer downlevel
// baseline; both payloads use the exact same stable output-index calculation.
@compute @workgroup_size(128)
fn scatter(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= pass_params.group_count) {
    return;
  }

  if (lane < RADIX) {
    digit_prior[lane] = 0u;
  }
  workgroupBarrier();

  let tile_begin = group * TILE_SIZE;
  let word = lane >> 5u;
  let bit = lane & 31u;
  let lower_lane_mask = (1u << bit) - 1u;

  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    if (lane < MASK_WORD_COUNT) {
      atomicStore(&digit_masks[lane], 0u);
    }
    workgroupBarrier();

    let index = tile_begin + round * WORKGROUP_SIZE + lane;
    let valid = index < pass_params.count;
    var key = 0u;
    var payload = 0u;
    var digit_slot = 0u;
    if (valid) {
      key = radix_keys_src[index];
      payload = radix_payload_src[index];
      digit_slot = descending_digit_slot(key);
      atomicOr(
        &digit_masks[digit_slot * MASK_WORDS_PER_DIGIT + word],
        1u << bit,
      );
    }
    workgroupBarrier();

    if (valid) {
      let mask_base = digit_slot * MASK_WORDS_PER_DIGIT;
      var local_rank = digit_prior[digit_slot];
      for (var earlier_word = 0u; earlier_word < word; earlier_word += 1u) {
        local_rank += countOneBits(
          atomicLoad(&digit_masks[mask_base + earlier_word]),
        );
      }
      local_rank += countOneBits(
        atomicLoad(&digit_masks[mask_base + word]) & lower_lane_mask,
      );

      let global_base = radix_prefix[
        digit_slot * pass_params.group_count + group
      ];
      let output_index = global_base + local_rank;
      radix_payload_dst[output_index] = payload;
    }
    workgroupBarrier();

    if (lane < RADIX) {
      let mask_base = lane * MASK_WORDS_PER_DIGIT;
      var round_count = 0u;
      for (var mask_word = 0u; mask_word < MASK_WORDS_PER_DIGIT; mask_word += 1u) {
        round_count += countOneBits(
          atomicLoad(&digit_masks[mask_base + mask_word]),
        );
      }
      digit_prior[lane] += round_count;
    }
    workgroupBarrier();
  }
}

// Exact Resident SoA fast path. Packed scenes already negotiate at least
// eight storage bindings for color resolution, so one dispatch can move the
// key and source-ID payload together. The four-binding Direct fallback keeps
// using `scatter` twice. Both entry points use the identical stable rank.
@compute @workgroup_size(128)
fn scatter_keys_ids(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= pass_params.group_count) {
    return;
  }

  if (lane < RADIX) {
    digit_prior[lane] = 0u;
  }
  workgroupBarrier();

  let tile_begin = group * TILE_SIZE;
  let word = lane >> 5u;
  let bit = lane & 31u;
  let lower_lane_mask = (1u << bit) - 1u;

  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    if (lane < MASK_WORD_COUNT) {
      atomicStore(&digit_masks[lane], 0u);
    }
    workgroupBarrier();

    let index = tile_begin + round * WORKGROUP_SIZE + lane;
    let valid = index < pass_params.count;
    var key = 0u;
    var source_id = 0u;
    var digit_slot = 0u;
    if (valid) {
      key = radix_keys_src[index];
      source_id = radix_payload_src[index];
      digit_slot = descending_digit_slot(key);
      atomicOr(
        &digit_masks[digit_slot * MASK_WORDS_PER_DIGIT + word],
        1u << bit,
      );
    }
    workgroupBarrier();

    if (valid) {
      let mask_base = digit_slot * MASK_WORDS_PER_DIGIT;
      var local_rank = digit_prior[digit_slot];
      for (var earlier_word = 0u; earlier_word < word; earlier_word += 1u) {
        local_rank += countOneBits(
          atomicLoad(&digit_masks[mask_base + earlier_word]),
        );
      }
      local_rank += countOneBits(
        atomicLoad(&digit_masks[mask_base + word]) & lower_lane_mask,
      );

      let global_base = radix_prefix[
        digit_slot * pass_params.group_count + group
      ];
      let output_index = global_base + local_rank;
      radix_keys_dst[output_index] = key;
      radix_payload_dst[output_index] = source_id;
    }
    workgroupBarrier();

    if (lane < RADIX) {
      let mask_base = lane * MASK_WORDS_PER_DIGIT;
      var round_count = 0u;
      for (var mask_word = 0u; mask_word < MASK_WORDS_PER_DIGIT; mask_word += 1u) {
        round_count += countOneBits(
          atomicLoad(&digit_masks[mask_base + mask_word]),
        );
      }
      digit_prior[lane] += round_count;
    }
    workgroupBarrier();
  }
}

// Compatibility output for the existing Direct renderer. The next integration
// step can bind final_ids directly with stride=1 and remove this allocation/pass.
@compute @workgroup_size(128)
fn pack_pairs(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  let tile_begin = group * TILE_SIZE;
  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    let index = tile_begin + round * WORKGROUP_SIZE + lane;
    if (index < pass_params.count) {
      packed_pairs[index] = SortPair(packed_keys[index], packed_ids[index]);
    }
  }
}
