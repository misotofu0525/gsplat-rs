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

@group(0) @binding(0)
var<storage, read> projected_center: array<vec4<f32>>;
@group(0) @binding(1)
var<storage, read> projected_conic: array<vec4<f32>>;
@group(0) @binding(2)
var<storage, read> projected_bbox: array<vec4<u32>>;
@group(0) @binding(3)
var<storage, read> resolved_color: array<vec2<u32>>;
@group(0) @binding(4)
var<storage, read> entries: array<CompactEntry>;
@group(0) @binding(5)
var<storage, read> sorted_entry_ids: array<u32>;
@group(0) @binding(6)
var<storage, read> tile_offsets: array<u32>;
@group(0) @binding(7)
var<uniform> params: TiledParams;
@group(0) @binding(8)
var output_image: texture_storage_2d<rgba16float, write>;

const ALPHA_THRESHOLD: f32 = 1.0 / 255.0;
const MAX_ALPHA: f32 = 0.99;
const TRANSMITTANCE_EARLY_OUT: f32 = 1e-4;

fn unpack_color_rgb18e8(bits: vec2<u32>) -> vec3<f32> {
  let exponent_code = (bits.y >> 22u) & 0xffu;
  if (exponent_code == 0u) { return vec3<f32>(0.0); }
  let r = bits.x & 0x3ffffu;
  let g = (bits.x >> 18u) | ((bits.y & 0xfu) << 14u);
  let b = (bits.y >> 4u) & 0x3ffffu;
  let scale = exp2(f32(i32(exponent_code) - 127));
  return vec3<f32>(f32(r), f32(g), f32(b)) * (scale / 262143.0);
}

fn sample_alpha(source_id: u32, pixel: vec2<u32>) -> f32 {
  let bbox = projected_bbox[source_id];
  if (pixel.x < bbox.x || pixel.x >= bbox.z || pixel.y < bbox.y || pixel.y >= bbox.w) {
    return 0.0;
  }
  let center = projected_center[source_id];
  let conic = projected_conic[source_id].xyz;
  let delta = vec2<f32>(pixel) + vec2<f32>(0.5) - center.xy;
  let q = conic.x * delta.x * delta.x + 2.0 * conic.y * delta.x * delta.y + conic.z * delta.y * delta.y;
  return min(MAX_ALPHA, center.w * exp(-0.5 * q));
}

@compute @workgroup_size(16, 16, 1)
fn rasterize_tile(
  @builtin(local_invocation_id) local_id: vec3<u32>,
  @builtin(workgroup_id) tile: vec3<u32>,
) {
  let pixel = vec2<u32>(tile.x * 16u + local_id.x, tile.y * 16u + local_id.y);
  if (pixel.x >= params.width || pixel.y >= params.height) { return; }
  let tile_id = tile.y * params.tiles_x + tile.x;
  let begin = tile_offsets[tile_id];
  let end = tile_offsets[tile_id + 1u];
  var transmittance = 1.0;
  var rgb = vec3<f32>(0.0);
  for (var position = begin; position < end; position += 1u) {
    let entry = entries[sorted_entry_ids[position]];
    let alpha = sample_alpha(entry.source_id, pixel);
    if (alpha < ALPHA_THRESHOLD) { continue; }
    let weight = transmittance * alpha;
    rgb += weight * unpack_color_rgb18e8(resolved_color[entry.source_id]);
    transmittance *= 1.0 - alpha;
    if (transmittance < TRANSMITTANCE_EARLY_OUT) { break; }
  }
  textureStore(output_image, vec2<i32>(pixel), vec4<f32>(rgb, 1.0 - transmittance));
}
