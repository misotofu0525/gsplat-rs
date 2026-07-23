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

struct CompactEntry {
  tile_id: u32,
  source_id: u32,
};

struct Status {
  total_entries: atomic<u32>,
  overflow: atomic<u32>,
  written_entries: atomic<u32>,
  reserved: atomic<u32>,
};

@group(0) @binding(0)
var<storage, read> projected_center: array<vec4<f32>>;
@group(0) @binding(1)
var<storage, read> projected_conic: array<vec4<f32>>;
@group(0) @binding(2)
var<storage, read> projected_bbox: array<vec4<u32>>;
@group(0) @binding(3)
var<storage, read> source_order_btf: array<u32>;
@group(0) @binding(4)
var<uniform> params: TiledParams;
@group(0) @binding(5)
var<storage, read_write> work_counts: array<u32>;
@group(0) @binding(6)
var<storage, read_write> work_offsets: array<u32>;
@group(0) @binding(7)
var<storage, read_write> entries: array<CompactEntry>;
@group(0) @binding(8)
var<storage, read_write> status: Status;
@group(0) @binding(9)
var<storage, read_write> tile_counts: array<atomic<u32>>;

const WORKGROUP_SIZE: u32 = 128u;
const TILE_SIZE: u32 = 16u;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
}

fn work_index(lane: u32, wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return logical_workgroup_id(wg_id, num_wg) * WORKGROUP_SIZE + lane;
}

fn source_for_front_to_back_rank(rank: u32) -> u32 {
  return source_order_btf[params.work_count - 1u - rank];
}

fn conic_q(conic: vec3<f32>, delta: vec2<f32>) -> f32 {
  return conic.x * delta.x * delta.x
    + 2.0 * conic.y * delta.x * delta.y
    + conic.z * delta.y * delta.y;
}

// Conservative O(1) ellipse/rectangle test over the continuous rectangle of
// pixel centers in this tile. The continuous minimum cannot exceed the
// discrete pixel-center minimum, so a rejection is exact; a false positive
// merely creates an extra entry that the raster pass later rejects.
fn tile_may_contribute(source_id: u32, tile_x: u32, tile_y: u32) -> bool {
  let bbox = projected_bbox[source_id];
  let min_x = max(tile_x * TILE_SIZE, bbox.x);
  let min_y = max(tile_y * TILE_SIZE, bbox.y);
  let max_x = min(min(tile_x * TILE_SIZE + TILE_SIZE, params.width), bbox.z);
  let max_y = min(min(tile_y * TILE_SIZE + TILE_SIZE, params.height), bbox.w);
  if (min_x >= max_x || min_y >= max_y) {
    return false;
  }

  let center = projected_center[source_id].xy;
  let packed_conic = projected_conic[source_id];
  let conic = packed_conic.xyz;
  let q_limit = packed_conic.w;
  let lo = vec2<f32>(f32(min_x) + 0.5, f32(min_y) + 0.5);
  let hi = vec2<f32>(f32(max_x) - 0.5, f32(max_y) - 0.5);
  if (all(center >= lo) && all(center <= hi)) {
    return true;
  }

  var min_q = 3.402823466e+38;
  let safe_xx = max(conic.x, 1e-20);
  let safe_yy = max(conic.z, 1e-20);
  let y_at_lo_x = clamp(
    center.y - conic.y * (lo.x - center.x) / safe_yy,
    lo.y,
    hi.y,
  );
  let y_at_hi_x = clamp(
    center.y - conic.y * (hi.x - center.x) / safe_yy,
    lo.y,
    hi.y,
  );
  let x_at_lo_y = clamp(
    center.x - conic.y * (lo.y - center.y) / safe_xx,
    lo.x,
    hi.x,
  );
  let x_at_hi_y = clamp(
    center.x - conic.y * (hi.y - center.y) / safe_xx,
    lo.x,
    hi.x,
  );
  min_q = min(min_q, conic_q(conic, vec2<f32>(lo.x, y_at_lo_x) - center));
  min_q = min(min_q, conic_q(conic, vec2<f32>(hi.x, y_at_hi_x) - center));
  min_q = min(min_q, conic_q(conic, vec2<f32>(x_at_lo_y, lo.y) - center));
  min_q = min(min_q, conic_q(conic, vec2<f32>(x_at_hi_y, hi.y) - center));
  return max(min_q, 0.0) <= q_limit;
}

fn contribution_count(source_id: u32) -> u32 {
  let bbox = projected_bbox[source_id];
  if (bbox.x >= bbox.z || bbox.y >= bbox.w) {
    return 0u;
  }
  let min_tile_x = bbox.x / TILE_SIZE;
  let min_tile_y = bbox.y / TILE_SIZE;
  let max_tile_x = (bbox.z - 1u) / TILE_SIZE;
  let max_tile_y = (bbox.w - 1u) / TILE_SIZE;
  var count = 0u;
  for (var tile_y = min_tile_y; tile_y <= max_tile_y; tile_y += 1u) {
    for (var tile_x = min_tile_x; tile_x <= max_tile_x; tile_x += 1u) {
      if (tile_may_contribute(source_id, tile_x, tile_y)) {
        count += 1u;
      }
    }
  }
  return count;
}

@compute @workgroup_size(128)
fn count_entries(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let rank = work_index(lane, wg_id, num_wg);
  if (rank >= params.work_count) {
    return;
  }
  let source_id = source_for_front_to_back_rank(rank);
  var count = 0u;
  if (source_id < params.source_count) {
    count = contribution_count(source_id);
  } else {
    atomicStore(&status.overflow, 1u);
  }
  work_counts[rank] = count;
  let previous = atomicAdd(&status.total_entries, count);
  if (previous > 0xffffffffu - count) {
    atomicStore(&status.overflow, 1u);
  }
}

@compute @workgroup_size(128)
fn copy_work_counts(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let rank = work_index(lane, wg_id, num_wg);
  if (rank < params.work_count) {
    work_offsets[rank] = work_counts[rank];
  }
}

@compute @workgroup_size(1)
fn finalize_entry_count() {
  if (params.work_count == 0u) {
    atomicStore(&status.total_entries, 0u);
    return;
  }
  let last = params.work_count - 1u;
  let offset = work_offsets[last];
  let count = work_counts[last];
  if (offset > 0xffffffffu - count || atomicLoad(&status.total_entries) != offset + count) {
    atomicStore(&status.overflow, 1u);
  } else {
    atomicStore(&status.total_entries, offset + count);
  }
}

@compute @workgroup_size(128)
fn scatter_entries(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let rank = work_index(lane, wg_id, num_wg);
  if (rank >= params.work_count) {
    return;
  }
  let source_id = source_for_front_to_back_rank(rank);
  if (source_id >= params.source_count) {
    atomicStore(&status.overflow, 1u);
    return;
  }
  let bbox = projected_bbox[source_id];
  if (bbox.x >= bbox.z || bbox.y >= bbox.w) {
    return;
  }
  let min_tile_x = bbox.x / TILE_SIZE;
  let min_tile_y = bbox.y / TILE_SIZE;
  let max_tile_x = (bbox.z - 1u) / TILE_SIZE;
  let max_tile_y = (bbox.w - 1u) / TILE_SIZE;
  var local_index = 0u;
  for (var tile_y = min_tile_y; tile_y <= max_tile_y; tile_y += 1u) {
    for (var tile_x = min_tile_x; tile_x <= max_tile_x; tile_x += 1u) {
      if (!tile_may_contribute(source_id, tile_x, tile_y)) {
        continue;
      }
      let output_index = work_offsets[rank] + local_index;
      let tile_id = tile_y * params.tiles_x + tile_x;
      if (output_index >= params.entry_capacity || tile_id >= params.tile_count) {
        atomicStore(&status.overflow, 1u);
      } else {
        entries[output_index] = CompactEntry(tile_id, source_id);
        atomicAdd(&tile_counts[tile_id], 1u);
        atomicAdd(&status.written_entries, 1u);
      }
      local_index += 1u;
    }
  }
}

@compute @workgroup_size(128)
fn copy_tile_counts(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let index = work_index(lane, wg_id, num_wg);
  if (index < params.tile_count) {
    work_offsets[index] = atomicLoad(&tile_counts[index]);
  }
}

@compute @workgroup_size(1)
fn finalize_tile_offsets() {
  work_offsets[params.tile_count] = params.entry_count;
  atomicStore(&status.total_entries, params.entry_count);
  if (atomicLoad(&status.written_entries) != params.entry_count) {
    atomicStore(&status.overflow, 1u);
  }
}
