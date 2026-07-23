struct PassParams {
  shift: u32,
  count: u32,
  group_count: u32,
  _pad: u32,
};

@group(0) @binding(4)
var<storage, read> radix_keys_src: array<u32>;
@group(0) @binding(5)
var<storage, read> radix_ids_src: array<u32>;
@group(0) @binding(6)
var<storage, read_write> radix_ids_dst: array<u32>;
@group(0) @binding(7)
var<storage, read_write> radix_prefix: array<u32>;
@group(0) @binding(8)
var<uniform> pass_params: PassParams;
@group(0) @binding(13)
var<storage, read_write> radix_keys_dst: array<u32>;

const WORKGROUP_SIZE: u32 = 128u;
const ITEMS_PER_THREAD: u32 = 8u;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const RADIX: u32 = 256u;
const MASK_WORDS_PER_DIGIT: u32 = WORKGROUP_SIZE / 32u;

// Each entry point uses only workgroup-local atomics. The flattened prefix
// buffer is digit-major, so the existing hierarchical exclusive scan turns
// every histogram count into the global stable destination for that
// (descending digit, workgroup) pair.
var<workgroup> histogram_counts: array<atomic<u32>, 256>;

// One 128-lane bit mask per byte value records the exact input order in a
// round. `digit_prior` carries counts from earlier rounds in the same 1024
// element tile. Together they preserve workgroup, round, then lane order.
// There are 256 * 4 = 1024 mask words. WGSL guarantees zero initialization for
// workgroup atomic composites; contributing lanes atomically remove their own
// bit after every round, so no bulk clear is needed.
var<workgroup> digit_masks: array<atomic<u32>, 1024>;
var<workgroup> digit_prior: array<atomic<u32>, 256>;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
}

fn descending_digit_slot(key: u32) -> u32 {
  let digit = (key >> pass_params.shift) & 255u;
  return 255u - digit;
}

@compute @workgroup_size(128)
fn histogram_radix8(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= pass_params.group_count) {
    return;
  }

  let tile_begin = group * TILE_SIZE;
  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    let index = tile_begin + round * WORKGROUP_SIZE + lane;
    if (index < pass_params.count) {
      let digit_slot = descending_digit_slot(radix_keys_src[index]);
      atomicAdd(&histogram_counts[digit_slot], 1u);
    }
  }
  workgroupBarrier();

  for (var digit = lane; digit < RADIX; digit += WORKGROUP_SIZE) {
    radix_prefix[digit * pass_params.group_count + group] =
      atomicLoad(&histogram_counts[digit]);
  }
}

// Resident scenes negotiate enough storage bindings to move the full 32-bit
// key and source ID together. Four stable byte passes therefore retain the
// complete key while halving dispatch/scan/scatter rounds versus base 16.
@compute @workgroup_size(128)
fn scatter_keys_ids_radix8(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= pass_params.group_count) {
    return;
  }

  let tile_begin = group * TILE_SIZE;
  let word = lane >> 5u;
  let bit = lane & 31u;
  let lower_lane_mask = (1u << bit) - 1u;

  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    let index = tile_begin + round * WORKGROUP_SIZE + lane;
    let valid = index < pass_params.count;
    var key = 0u;
    var source_id = 0u;
    var digit_slot = 0u;
    var own_mask = 0u;
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
      var local_rank = atomicLoad(&digit_prior[digit_slot]);
      for (var earlier_word = 0u; earlier_word < word; earlier_word += 1u) {
        local_rank += countOneBits(
          atomicLoad(&digit_masks[mask_base + earlier_word]),
        );
      }
      own_mask = atomicLoad(&digit_masks[mask_base + word]);
      local_rank += countOneBits(own_mask & lower_lane_mask);

      let global_base = radix_prefix[
        digit_slot * pass_params.group_count + group
      ];
      let output_index = global_base + local_rank;
      radix_keys_dst[output_index] = key;
      radix_ids_dst[output_index] = source_id;
    }
    workgroupBarrier();

    if (valid && bit == firstTrailingBit(own_mask)) {
      // One elected lane per non-empty mask word advances the digit by the
      // complete word population and clears that word for the next round.
      atomicAdd(&digit_prior[digit_slot], countOneBits(own_mask));
      atomicStore(
        &digit_masks[digit_slot * MASK_WORDS_PER_DIGIT + word],
        0u,
      );
    }
    workgroupBarrier();
  }
}
