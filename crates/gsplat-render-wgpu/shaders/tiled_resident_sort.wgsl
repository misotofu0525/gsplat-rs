struct CompactEntry {
  tile_id: u32,
  source_id: u32,
};

struct TiledParams {
  width: u32,
  height: u32,
  tiles_x: u32,
  tiles_y: u32,
  source_count: u32,
  work_count: u32,
  tile_count: u32,
  entry_capacity: u32,
  entry_count: u32,
  _pad0: u32,
  _pad1: u32,
  _pad2: u32,
};

struct PassParams {
  shift: u32,
  count: u32,
  group_count: u32,
  _pad: u32,
};

@group(0) @binding(0)
var<storage, read> entries: array<CompactEntry>;
@group(0) @binding(1)
var<uniform> tiled_params: TiledParams;
@group(0) @binding(2)
var<storage, read_write> generated_keys: array<u32>;
@group(0) @binding(3)
var<storage, read_write> generated_ids: array<u32>;
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

const WORKGROUP_SIZE: u32 = 128u;
const ITEMS_PER_THREAD: u32 = 8u;
const ITEMS_PER_GROUP: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const RADIX: u32 = 16u;
const MASK_WORDS_PER_DIGIT: u32 = WORKGROUP_SIZE / 32u;
const MASK_WORD_COUNT: u32 = RADIX * MASK_WORDS_PER_DIGIT;
var<workgroup> histogram_counts: array<atomic<u32>, 16>;
var<workgroup> digit_masks: array<atomic<u32>, 64>;
var<workgroup> digit_prior: array<u32, 16>;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
}

fn item_index(lane: u32, round: u32, wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return logical_workgroup_id(wg_id, num_wg) * ITEMS_PER_GROUP + round * WORKGROUP_SIZE + lane;
}

fn digit_slot(key: u32) -> u32 {
  return (key >> pass_params.shift) & 15u;
}

@compute @workgroup_size(128)
fn init_tile_keys(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    let index = item_index(lane, round, wg_id, num_wg);
    if (index < tiled_params.entry_count) {
      generated_keys[index] = entries[index].tile_id;
      generated_ids[index] = index;
    }
  }
}

@compute @workgroup_size(128)
fn histogram(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= pass_params.group_count) { return; }
  if (lane < RADIX) { atomicStore(&histogram_counts[lane], 0u); }
  workgroupBarrier();
  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    let index = group * ITEMS_PER_GROUP + round * WORKGROUP_SIZE + lane;
    if (index < pass_params.count) {
      atomicAdd(&histogram_counts[digit_slot(radix_keys_src[index])], 1u);
    }
  }
  workgroupBarrier();
  if (lane < RADIX) {
    radix_prefix[lane * pass_params.group_count + group] = atomicLoad(&histogram_counts[lane]);
  }
}

@compute @workgroup_size(128)
fn scatter(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  if (group >= pass_params.group_count) { return; }
  if (lane < RADIX) { digit_prior[lane] = 0u; }
  workgroupBarrier();
  let word = lane >> 5u;
  let bit = lane & 31u;
  let lower_lane_mask = (1u << bit) - 1u;
  for (var round = 0u; round < ITEMS_PER_THREAD; round += 1u) {
    if (lane < MASK_WORD_COUNT) { atomicStore(&digit_masks[lane], 0u); }
    workgroupBarrier();
    let index = group * ITEMS_PER_GROUP + round * WORKGROUP_SIZE + lane;
    let valid = index < pass_params.count;
    var payload = 0u;
    var slot = 0u;
    if (valid) {
      payload = radix_payload_src[index];
      slot = digit_slot(radix_keys_src[index]);
      atomicOr(&digit_masks[slot * MASK_WORDS_PER_DIGIT + word], 1u << bit);
    }
    workgroupBarrier();
    if (valid) {
      let mask_base = slot * MASK_WORDS_PER_DIGIT;
      var local_rank = digit_prior[slot];
      for (var earlier_word = 0u; earlier_word < word; earlier_word += 1u) {
        local_rank += countOneBits(atomicLoad(&digit_masks[mask_base + earlier_word]));
      }
      local_rank += countOneBits(atomicLoad(&digit_masks[mask_base + word]) & lower_lane_mask);
      let output_index = radix_prefix[slot * pass_params.group_count + group] + local_rank;
      radix_payload_dst[output_index] = payload;
    }
    workgroupBarrier();
    if (lane < RADIX) {
      let mask_base = lane * MASK_WORDS_PER_DIGIT;
      var round_count = 0u;
      for (var mask_word = 0u; mask_word < MASK_WORDS_PER_DIGIT; mask_word += 1u) {
        round_count += countOneBits(atomicLoad(&digit_masks[mask_base + mask_word]));
      }
      digit_prior[lane] += round_count;
    }
    workgroupBarrier();
  }
}
