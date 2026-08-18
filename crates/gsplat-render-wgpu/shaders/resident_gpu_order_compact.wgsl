struct SortPair {
  key: u32,
  id: u32,
};

struct CompactParams {
  count: u32,
  tile_count: u32,
  block_count: u32,
  tile_sum_offset: u32,
};

struct OrderMeta {
  visible_count: u32,
  group_count: u32,
  block_count: u32,
  _pad0: u32,
  dispatch_x: u32,
  dispatch_y: u32,
  dispatch_z: u32,
  _pad1: u32,
  vertex_count: u32,
  instance_count: u32,
  first_vertex: u32,
  first_instance: u32,
};

@group(0) @binding(0)
var<storage, read_write> scratch: array<u32>;
@group(0) @binding(1)
var<storage, read> src_pairs: array<SortPair>;
@group(0) @binding(2)
var<storage, read_write> dst_pairs: array<SortPair>;
@group(0) @binding(3)
var<storage, read_write> order_meta: OrderMeta;
@group(0) @binding(4)
var<uniform> compact_params: CompactParams;

const WORKGROUP_SIZE: u32 = 64u;
const ITEMS_PER_THREAD: u32 = 4u;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const SCAN_BLOCK: u32 = 256u;

var<workgroup> partial: array<u32, 64>;

fn flag_at(index: u32) -> u32 {
  return scratch[index];
}

fn tile_sum_index(tile: u32) -> u32 {
  return compact_params.tile_sum_offset + tile;
}

fn block_sum_index(block: u32) -> u32 {
  return compact_params.tile_sum_offset + compact_params.tile_count + block;
}

@compute @workgroup_size(64)
fn compact_histogram(
  @builtin(local_invocation_id) local_id3: vec3<u32>,
  @builtin(workgroup_id) group_id3: vec3<u32>,
) {
  let local_id = local_id3.x;
  let group_id = group_id3.x;
  var local_sum = 0u;
  let first = group_id * TILE_SIZE + local_id * ITEMS_PER_THREAD;
  for (var item = 0u; item < ITEMS_PER_THREAD; item += 1u) {
    let index = first + item;
    if (index < compact_params.count) {
      local_sum += flag_at(index);
    }
  }
  partial[local_id] = local_sum;
  workgroupBarrier();

  if (local_id == 0u) {
    var total = 0u;
    for (var thread = 0u; thread < WORKGROUP_SIZE; thread += 1u) {
      total += partial[thread];
    }
    scratch[tile_sum_index(group_id)] = total;
  }
}

@compute @workgroup_size(64)
fn compact_prefix_block(
  @builtin(local_invocation_id) local_id3: vec3<u32>,
  @builtin(workgroup_id) group_id3: vec3<u32>,
) {
  if (local_id3.x == 0u) {
    let first = group_id3.x * SCAN_BLOCK;
    var running = 0u;
    for (var i = 0u; i < SCAN_BLOCK; i += 1u) {
      let tile = first + i;
      if (tile < compact_params.tile_count) {
        let count = scratch[tile_sum_index(tile)];
        scratch[tile_sum_index(tile)] = running;
        running += count;
      }
    }
    scratch[block_sum_index(group_id3.x)] = running;
  }
}

@compute @workgroup_size(64)
fn compact_prefix_top(@builtin(local_invocation_id) local_id3: vec3<u32>) {
  if (local_id3.x == 0u) {
    var running = 0u;
    for (var block = 0u; block < compact_params.block_count; block += 1u) {
      let offset = block_sum_index(block);
      let count = scratch[offset];
      scratch[offset] = running;
      running += count;
    }
    order_meta.visible_count = running;
  }
}

@compute @workgroup_size(64)
fn compact_prefix_add(
  @builtin(local_invocation_id) local_id3: vec3<u32>,
  @builtin(workgroup_id) group_id3: vec3<u32>,
) {
  if (local_id3.x == 0u) {
    let base = scratch[block_sum_index(group_id3.x)];
    let first = group_id3.x * SCAN_BLOCK;
    for (var i = 0u; i < SCAN_BLOCK; i += 1u) {
      let tile = first + i;
      if (tile < compact_params.tile_count) {
        let offset = tile_sum_index(tile);
        scratch[offset] = scratch[offset] + base;
      }
    }
  }
}

@compute @workgroup_size(64)
fn compact_scatter(
  @builtin(local_invocation_id) local_id3: vec3<u32>,
  @builtin(workgroup_id) group_id3: vec3<u32>,
) {
  let local_id = local_id3.x;
  let group_id = group_id3.x;
  let first = group_id * TILE_SIZE + local_id * ITEMS_PER_THREAD;
  var items: array<SortPair, 4>;
  var visible: array<bool, 4>;
  var local_count = 0u;
  for (var item = 0u; item < ITEMS_PER_THREAD; item += 1u) {
    let index = first + item;
    let keep = index < compact_params.count && flag_at(index) != 0u;
    visible[item] = keep;
    if (keep) {
      items[item] = src_pairs[index];
      local_count += 1u;
    }
  }
  partial[local_id] = local_count;
  workgroupBarrier();

  if (local_id == 0u) {
    var running = 0u;
    for (var thread = 0u; thread < WORKGROUP_SIZE; thread += 1u) {
      let count = partial[thread];
      partial[thread] = running;
      running += count;
    }
  }
  workgroupBarrier();

  var dest = scratch[tile_sum_index(group_id)] + partial[local_id];
  for (var item = 0u; item < ITEMS_PER_THREAD; item += 1u) {
    if (visible[item]) {
      dst_pairs[dest] = items[item];
      dest += 1u;
    }
  }
}

@compute @workgroup_size(1)
fn write_indirect_args() {
  let visible = order_meta.visible_count;
  var groups = 0u;
  if (visible > 0u) {
    groups = (visible + TILE_SIZE - 1u) / TILE_SIZE;
  }
  var blocks = 0u;
  if (groups > 0u) {
    blocks = (groups + SCAN_BLOCK - 1u) / SCAN_BLOCK;
  }
  order_meta.group_count = groups;
  order_meta.block_count = blocks;
  order_meta.dispatch_x = groups;
  order_meta.dispatch_y = 1u;
  order_meta.dispatch_z = 1u;
  order_meta.vertex_count = 6u;
  order_meta.instance_count = visible;
  order_meta.first_vertex = 0u;
  order_meta.first_instance = 0u;
}
