// Stable full32 base-16 radix over an externally produced dynamic prefix.
// The producer owns C and the indirect dispatch command in `prefix_control`;
// this shader never regenerates, truncates, or reads beyond that prefix.

struct PassParams {
  shift: u32,
  capacity_count: u32,
  capacity_group_count: u32,
  _pad: u32,
};

struct PrefixControl {
  count: u32,
  active_group_count: u32,
  dispatch_x: u32,
  dispatch_y: u32,
  dispatch_z: u32,
  dispatch_limit: u32,
  capacity_count: u32,
  _pad0: u32,
};

@group(0) @binding(0)
var<storage, read> radix_keys_src: array<u32>;
@group(0) @binding(1)
var<storage, read> radix_ids_src: array<u32>;
@group(0) @binding(2)
var<storage, read_write> radix_ids_dst: array<u32>;
@group(0) @binding(3)
var<storage, read_write> radix_prefix: array<u32>;
@group(0) @binding(4)
var<uniform> pass_params: PassParams;
@group(0) @binding(5)
var<storage, read_write> radix_keys_dst: array<u32>;
@group(0) @binding(6)
var<storage, read> prefix_control: PrefixControl;

const WORKGROUP_SIZE: u32 = 128u;
const ITEMS_PER_THREAD: u32 = 8u;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const RADIX: u32 = 16u;
const MASK_WORDS_PER_DIGIT: u32 = WORKGROUP_SIZE / 32u;
const MASK_WORD_COUNT: u32 = RADIX * MASK_WORDS_PER_DIGIT;

var<workgroup> histogram_counts: array<atomic<u32>, 16>;
var<workgroup> digit_masks: array<atomic<u32>, MASK_WORD_COUNT>;
var<workgroup> digit_prior: array<u32, 16>;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
}

// Prefix scan order is slot-major, then workgroup-major. Reversing the digit
// maps a normal ascending exclusive scan to a descending key order while all
// stable passes remain LSD-first.
fn descending_digit_slot(key: u32) -> u32 {
  let digit = (key >> pass_params.shift) & 15u;
  return 15u - digit;
}

@compute @workgroup_size(128)
fn histogram_dynamic(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= prefix_control.active_group_count) {
    return;
  }
  if (lane < RADIX) {
    atomicStore(&histogram_counts[lane], 0u);
  }
  workgroupBarrier();

  let tile_begin = group * TILE_SIZE;
  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    let index = tile_begin + round * WORKGROUP_SIZE + lane;
    if (index < prefix_control.count) {
      atomicAdd(
        &histogram_counts[descending_digit_slot(radix_keys_src[index])],
        1u,
      );
    }
  }
  workgroupBarrier();
  if (lane < RADIX) {
    radix_prefix[lane * pass_params.capacity_group_count + group] =
      atomicLoad(&histogram_counts[lane]);
  }
}

@compute @workgroup_size(128)
fn scatter_dynamic(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= prefix_control.active_group_count) {
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
    let valid = index < prefix_control.count;
    var key = 0u;
    var source_id = 0u;
    var digit_slot = 0u;
    if (valid) {
      key = radix_keys_src[index];
      source_id = radix_ids_src[index];
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
      let output_index = radix_prefix[
        digit_slot * pass_params.capacity_group_count + group
      ] + local_rank;
      radix_keys_dst[output_index] = key;
      radix_ids_dst[output_index] = source_id;
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
