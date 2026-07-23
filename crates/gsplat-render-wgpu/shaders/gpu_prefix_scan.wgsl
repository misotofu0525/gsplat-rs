struct ScanParams {
  count: u32,
  _pad0: u32,
  _pad1: u32,
  _pad2: u32,
};

@group(0) @binding(0)
var<storage, read_write> scan_data: array<u32>;
@group(0) @binding(1)
var<storage, read_write> block_sums: array<u32>;
@group(0) @binding(2)
var<uniform> scan_params: ScanParams;

const SCAN_WORKGROUP_SIZE: u32 = 256u;
const SCAN_ITEMS_PER_GROUP: u32 = 512u;

var<workgroup> scan_temp: array<u32, 512>;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
}

@compute @workgroup_size(256)
fn scan_blocks(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  let group_count = (scan_params.count + SCAN_ITEMS_PER_GROUP - 1u) /
    SCAN_ITEMS_PER_GROUP;
  if (group >= group_count) {
    return;
  }

  let base = group * SCAN_ITEMS_PER_GROUP;
  let index0 = base + lane;
  let index1 = index0 + SCAN_WORKGROUP_SIZE;
  var value0 = 0u;
  var value1 = 0u;
  if (index0 < scan_params.count) {
    value0 = scan_data[index0];
  }
  if (index1 < scan_params.count) {
    value1 = scan_data[index1];
  }
  scan_temp[lane] = value0;
  scan_temp[lane + SCAN_WORKGROUP_SIZE] = value1;

  var offset = 1u;
  for (var nodes = SCAN_ITEMS_PER_GROUP >> 1u; nodes > 0u; nodes >>= 1u) {
    workgroupBarrier();
    if (lane < nodes) {
      let left = offset * (2u * lane + 1u) - 1u;
      let right = offset * (2u * lane + 2u) - 1u;
      scan_temp[right] += scan_temp[left];
    }
    offset <<= 1u;
  }

  if (lane == 0u) {
    block_sums[group] = scan_temp[SCAN_ITEMS_PER_GROUP - 1u];
    scan_temp[SCAN_ITEMS_PER_GROUP - 1u] = 0u;
  }

  for (var nodes = 1u; nodes < SCAN_ITEMS_PER_GROUP; nodes <<= 1u) {
    offset >>= 1u;
    workgroupBarrier();
    if (lane < nodes) {
      let left = offset * (2u * lane + 1u) - 1u;
      let right = offset * (2u * lane + 2u) - 1u;
      let left_value = scan_temp[left];
      scan_temp[left] = scan_temp[right];
      scan_temp[right] += left_value;
    }
  }
  workgroupBarrier();

  if (index0 < scan_params.count) {
    scan_data[index0] = scan_temp[lane];
  }
  if (index1 < scan_params.count) {
    scan_data[index1] = scan_temp[lane + SCAN_WORKGROUP_SIZE];
  }
}

@compute @workgroup_size(256)
fn add_block_offsets(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let group = logical_workgroup_id(wg_id, num_wg);
  let group_count = (scan_params.count + SCAN_ITEMS_PER_GROUP - 1u) /
    SCAN_ITEMS_PER_GROUP;
  if (group >= group_count) {
    return;
  }

  let offset = block_sums[group];
  let base = group * SCAN_ITEMS_PER_GROUP;
  let index0 = base + lane;
  let index1 = index0 + SCAN_WORKGROUP_SIZE;
  if (index0 < scan_params.count) {
    scan_data[index0] += offset;
  }
  if (index1 < scan_params.count) {
    scan_data[index1] += offset;
  }
}
