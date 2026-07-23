struct ProjectedSplat {
  screen_center_depth_opacity: vec4<f32>,
  inverse_conic_and_qmax: vec4<f32>,
  color_rgb_pad: vec4<f32>,
  bbox_min_max: vec4<u32>,
  source_depth_pad: vec4<u32>,
};

struct TileContribution {
  tile_id: u32,
  depth_key: u32,
  source_id: u32,
  projected_index: u32,
};

struct TiledParams {
  width: u32,
  height: u32,
  tiles_x: u32,
  tiles_y: u32,
  splat_count: u32,
  tile_count: u32,
  entry_capacity: u32,
  entry_count: u32,
};

struct TiledStatus {
  total_entries: atomic<u32>,
  overflow: atomic<u32>,
  written_entries: atomic<u32>,
  reserved: atomic<u32>,
};

@group(0) @binding(0)
var<storage, read> splats: array<ProjectedSplat>;
@group(0) @binding(1)
var<uniform> params: TiledParams;
@group(0) @binding(2)
var<storage, read_write> splat_counts: array<u32>;
@group(0) @binding(3)
var<storage, read_write> splat_offsets: array<u32>;
@group(0) @binding(4)
var<storage, read_write> entries: array<TileContribution>;
@group(0) @binding(5)
var<storage, read_write> status: TiledStatus;
@group(0) @binding(6)
var<storage, read_write> tile_counts: array<atomic<u32>>;

const WORKGROUP_SIZE: u32 = 128u;
const TILE_WIDTH: u32 = 16u;
const TILE_HEIGHT: u32 = 16u;
const ALPHA_THRESHOLD: f32 = 1.0 / 255.0;
const MAX_ALPHA: f32 = 0.99;

fn logical_workgroup_id(wg_id: vec3<u32>, num_wg: vec3<u32>) -> u32 {
  return wg_id.x + wg_id.y * num_wg.x;
}

fn source_index(
  lane: u32,
  wg_id: vec3<u32>,
  num_wg: vec3<u32>,
) -> u32 {
  return logical_workgroup_id(wg_id, num_wg) * WORKGROUP_SIZE + lane;
}

fn sample_alpha(splat: ProjectedSplat, x: u32, y: u32) -> f32 {
  let bbox = splat.bbox_min_max;
  if (x < bbox.x || x >= bbox.z || y < bbox.y || y >= bbox.w) {
    return 0.0;
  }
  let dx = f32(x) + 0.5 - splat.screen_center_depth_opacity.x;
  let dy = f32(y) + 0.5 - splat.screen_center_depth_opacity.y;
  let conic = splat.inverse_conic_and_qmax.xyz;
  let q = conic.x * dx * dx + 2.0 * conic.y * dx * dy + conic.z * dy * dy;
  return min(MAX_ALPHA, splat.screen_center_depth_opacity.w * exp(-0.5 * q));
}

fn nearest_pixel_index(center_coordinate: f32, min_value: u32, max_value: u32) -> u32 {
  return u32(clamp(
    round(center_coordinate - 0.5),
    f32(min_value),
    f32(max_value - 1u),
  ));
}

// Exact discrete-pixel membership. For every coordinate on the shorter axis,
// minimize the positive-definite quadratic analytically on the other axis and
// sample the nearest clamped pixel center.
fn tile_has_contribution(
  splat: ProjectedSplat,
  tile_x: u32,
  tile_y: u32,
) -> bool {
  let min_x = tile_x * TILE_WIDTH;
  let min_y = tile_y * TILE_HEIGHT;
  let max_x = min(min_x + TILE_WIDTH, params.width);
  let max_y = min(min_y + TILE_HEIGHT, params.height);
  if (min_x >= max_x || min_y >= max_y) {
    return false;
  }

  let center = splat.screen_center_depth_opacity.xy;
  let conic = splat.inverse_conic_and_qmax.xyz;
  if (max_x - min_x <= max_y - min_y) {
    for (var x = min_x; x < max_x; x += 1u) {
      let dx = f32(x) + 0.5 - center.x;
      let optimal_y = center.y - conic.y * dx / conic.z;
      let y = nearest_pixel_index(optimal_y, min_y, max_y);
      if (sample_alpha(splat, x, y) >= ALPHA_THRESHOLD) {
        return true;
      }
    }
  } else {
    for (var y = min_y; y < max_y; y += 1u) {
      let dy = f32(y) + 0.5 - center.y;
      let optimal_x = center.x - conic.y * dy / conic.x;
      let x = nearest_pixel_index(optimal_x, min_x, max_x);
      if (sample_alpha(splat, x, y) >= ALPHA_THRESHOLD) {
        return true;
      }
    }
  }
  return false;
}

fn contribution_count(splat: ProjectedSplat) -> u32 {
  let bbox = splat.bbox_min_max;
  if (bbox.x >= bbox.z || bbox.y >= bbox.w) {
    return 0u;
  }
  let min_tile_x = bbox.x / TILE_WIDTH;
  let min_tile_y = bbox.y / TILE_HEIGHT;
  let max_tile_x = (bbox.z - 1u) / TILE_WIDTH;
  let max_tile_y = (bbox.w - 1u) / TILE_HEIGHT;
  var count = 0u;
  for (var tile_y = min_tile_y; tile_y <= max_tile_y; tile_y += 1u) {
    for (var tile_x = min_tile_x; tile_x <= max_tile_x; tile_x += 1u) {
      if (tile_has_contribution(splat, tile_x, tile_y)) {
        count += 1u;
      }
    }
  }
  return count;
}

@compute @workgroup_size(128)
fn count_contributions(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let index = source_index(lane, wg_id, num_wg);
  if (index >= params.splat_count) {
    return;
  }
  let count = contribution_count(splats[index]);
  splat_counts[index] = count;
  let previous = atomicAdd(&status.total_entries, count);
  if (previous > 0xffffffffu - count) {
    atomicStore(&status.overflow, 1u);
  }
}

@compute @workgroup_size(128)
fn copy_splat_counts(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let index = source_index(lane, wg_id, num_wg);
  if (index < params.splat_count) {
    splat_offsets[index] = splat_counts[index];
  }
}

@compute @workgroup_size(1)
fn finalize_entry_count() {
  if (params.splat_count == 0u) {
    atomicStore(&status.total_entries, 0u);
    return;
  }
  let last = params.splat_count - 1u;
  let offset = splat_offsets[last];
  let count = splat_counts[last];
  if (offset > 0xffffffffu - count) {
    atomicStore(&status.overflow, 1u);
    return;
  }
  let scanned_total = offset + count;
  if (atomicLoad(&status.total_entries) != scanned_total) {
    atomicStore(&status.overflow, 1u);
    return;
  }
  atomicStore(&status.total_entries, scanned_total);
}

@compute @workgroup_size(128)
fn scatter_contributions(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let index = source_index(lane, wg_id, num_wg);
  if (index >= params.splat_count) {
    return;
  }
  let splat = splats[index];
  let bbox = splat.bbox_min_max;
  if (bbox.x >= bbox.z || bbox.y >= bbox.w) {
    return;
  }
  let min_tile_x = bbox.x / TILE_WIDTH;
  let min_tile_y = bbox.y / TILE_HEIGHT;
  let max_tile_x = (bbox.z - 1u) / TILE_WIDTH;
  let max_tile_y = (bbox.w - 1u) / TILE_HEIGHT;
  let output_base = splat_offsets[index];
  var local_index = 0u;
  for (var tile_y = min_tile_y; tile_y <= max_tile_y; tile_y += 1u) {
    for (var tile_x = min_tile_x; tile_x <= max_tile_x; tile_x += 1u) {
      if (!tile_has_contribution(splat, tile_x, tile_y)) {
        continue;
      }
      let output_index = output_base + local_index;
      let tile_id = tile_y * params.tiles_x + tile_x;
      if (output_index >= params.entry_capacity || tile_id >= params.tile_count) {
        atomicStore(&status.overflow, 1u);
      } else {
        entries[output_index] = TileContribution(
          tile_id,
          splat.source_depth_pad.y,
          splat.source_depth_pad.x,
          index,
        );
        atomicAdd(&tile_counts[tile_id], 1u);
        atomicAdd(&status.written_entries, 1u);
      }
      local_index += 1u;
    }
  }
  if (local_index != splat_counts[index]) {
    atomicStore(&status.overflow, 1u);
  }
}

@compute @workgroup_size(128)
fn copy_tile_counts(
  @builtin(local_invocation_index) lane: u32,
  @builtin(workgroup_id) wg_id: vec3<u32>,
  @builtin(num_workgroups) num_wg: vec3<u32>,
) {
  let index = source_index(lane, wg_id, num_wg);
  if (index < params.tile_count) {
    splat_offsets[index] = atomicLoad(&tile_counts[index]);
  }
}

@compute @workgroup_size(1)
fn finalize_tile_offsets() {
  splat_offsets[params.tile_count] = params.entry_count;
  atomicStore(&status.total_entries, params.entry_count);
  if (atomicLoad(&status.written_entries) != params.entry_count) {
    atomicStore(&status.overflow, 1u);
  }
}
