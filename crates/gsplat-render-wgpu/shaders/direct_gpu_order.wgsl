struct SurfaceSourceElem {
  position: vec4<f32>,
  covariance0: vec4<f32>,
  covariance1: vec4<f32>,
  color_dc: vec4<f32>,
};

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
  _render_pad0: u32,
  _render_pad1: u32,
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

struct RadixMeta {
  bucket_base: array<u32, 16>,
  group_prefix: array<u32>,
};

@group(0) @binding(0)
var<storage, read> key_source: array<SurfaceSourceElem>;
@group(0) @binding(1)
var<uniform> render_params: RenderParams;
@group(0) @binding(2)
var<storage, read_write> generated_pairs: array<SortPair>;

@group(0) @binding(4)
var<storage, read> radix_src: array<SortPair>;
@group(0) @binding(5)
var<storage, read_write> radix_dst: array<SortPair>;
@group(0) @binding(6)
var<storage, read_write> radix_meta: RadixMeta;
@group(0) @binding(7)
var<uniform> pass_params: PassParams;

const WORKGROUP_SIZE: u32 = 64u;
const ITEMS_PER_THREAD: u32 = 4u;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const RADIX: u32 = 16u;

// One private histogram row per thread. The row order is also the stable
// scatter order: workgroup, then local thread, then the thread's four items.
var<workgroup> rows: array<u32, 1024>;
var<workgroup> bucket_totals: array<u32, 16>;

fn row_index(thread: u32, digit: u32) -> u32 {
  return thread * RADIX + digit;
}

fn digit_for(key: u32) -> u32 {
  return (key >> pass_params.shift) & 15u;
}

@compute @workgroup_size(64)
fn generate_pairs(
  @builtin(local_invocation_id) local_id3: vec3<u32>,
  @builtin(workgroup_id) group_id3: vec3<u32>,
) {
  let first = group_id3.x * TILE_SIZE + local_id3.x * ITEMS_PER_THREAD;
  for (var item = 0u; item < ITEMS_PER_THREAD; item += 1u) {
    let index = first + item;
    if (index < render_params.len) {
      let position = key_source[index].position.xyz;
      let relative = position - render_params.camera_pos.xyz;
      let depth = dot(render_params.view_rot_row2.xyz, relative);
      var key = 0u;
      if (depth >= render_params.near_plane && depth <= render_params.far_plane) {
        // Positive finite IEEE-754 values have the same ordering as their bits.
        key = bitcast<u32>(max(depth, 0.0));
      }
      generated_pairs[index] = SortPair(key, index);
    }
  }
}

@compute @workgroup_size(64)
fn histogram(
  @builtin(local_invocation_id) local_id3: vec3<u32>,
  @builtin(workgroup_id) group_id3: vec3<u32>,
) {
  let local_id = local_id3.x;
  let group_id = group_id3.x;
  for (var digit = 0u; digit < RADIX; digit += 1u) {
    rows[row_index(local_id, digit)] = 0u;
  }

  let first = group_id * TILE_SIZE + local_id * ITEMS_PER_THREAD;
  for (var item = 0u; item < ITEMS_PER_THREAD; item += 1u) {
    let index = first + item;
    if (index < pass_params.count) {
      let digit = digit_for(radix_src[index].key);
      let cell = row_index(local_id, digit);
      rows[cell] = rows[cell] + 1u;
    }
  }
  workgroupBarrier();

  if (local_id < RADIX) {
    var total = 0u;
    for (var thread = 0u; thread < WORKGROUP_SIZE; thread += 1u) {
      total += rows[row_index(thread, local_id)];
    }
    radix_meta.group_prefix[local_id * pass_params.group_count + group_id] = total;
  }
}

@compute @workgroup_size(64)
fn prefix(@builtin(local_invocation_id) local_id3: vec3<u32>) {
  let digit = local_id3.x;
  if (digit < RADIX) {
    var running = 0u;
    for (var group = 0u; group < pass_params.group_count; group += 1u) {
      let offset = digit * pass_params.group_count + group;
      let count = radix_meta.group_prefix[offset];
      radix_meta.group_prefix[offset] = running;
      running += count;
    }
    bucket_totals[digit] = running;
  }
  workgroupBarrier();

  if (digit < RADIX) {
    var base = 0u;
    // Higher digits precede lower digits for descending ordering.
    for (var higher = digit + 1u; higher < RADIX; higher += 1u) {
      base += bucket_totals[higher];
    }
    radix_meta.bucket_base[digit] = base;
  }
}

@compute @workgroup_size(64)
fn scatter(
  @builtin(local_invocation_id) local_id3: vec3<u32>,
  @builtin(workgroup_id) group_id3: vec3<u32>,
) {
  let local_id = local_id3.x;
  let group_id = group_id3.x;
  for (var digit = 0u; digit < RADIX; digit += 1u) {
    rows[row_index(local_id, digit)] = 0u;
  }

  let first = group_id * TILE_SIZE + local_id * ITEMS_PER_THREAD;
  var items: array<SortPair, 4>;
  var valid: array<bool, 4>;
  for (var item = 0u; item < ITEMS_PER_THREAD; item += 1u) {
    let index = first + item;
    valid[item] = index < pass_params.count;
    if (valid[item]) {
      let pair = radix_src[index];
      items[item] = pair;
      let digit = digit_for(pair.key);
      let cell = row_index(local_id, digit);
      rows[cell] = rows[cell] + 1u;
    }
  }
  workgroupBarrier();

  if (local_id < RADIX) {
    var running = 0u;
    for (var thread = 0u; thread < WORKGROUP_SIZE; thread += 1u) {
      let cell = row_index(thread, local_id);
      let count = rows[cell];
      rows[cell] = running;
      running += count;
    }
  }
  workgroupBarrier();

  for (var item = 0u; item < ITEMS_PER_THREAD; item += 1u) {
    if (valid[item]) {
      let pair = items[item];
      let digit = digit_for(pair.key);
      let cell = row_index(local_id, digit);
      let local_rank = rows[cell];
      rows[cell] = local_rank + 1u;
      let group_rank = radix_meta.group_prefix[
        digit * pass_params.group_count + group_id
      ];
      let output_index = radix_meta.bucket_base[digit] + group_rank + local_rank;
      radix_dst[output_index] = pair;
    }
  }
}
