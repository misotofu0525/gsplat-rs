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

@group(0) @binding(0)
var<storage, read> splats: array<ProjectedSplat>;
@group(0) @binding(1)
var<storage, read> entries: array<TileContribution>;
@group(0) @binding(2)
var<storage, read> sorted_entry_ids: array<u32>;
@group(0) @binding(3)
var<storage, read> tile_offsets: array<u32>;
@group(0) @binding(4)
var<uniform> params: TiledParams;
@group(0) @binding(5)
var output_image: texture_storage_2d<rgba16float, write>;

const ALPHA_THRESHOLD: f32 = 1.0 / 255.0;
const MAX_ALPHA: f32 = 0.99;
const TRANSMITTANCE_EARLY_OUT: f32 = 1e-4;

fn sample_alpha(splat: ProjectedSplat, pixel: vec2<u32>) -> f32 {
  let bbox = splat.bbox_min_max;
  if (pixel.x < bbox.x || pixel.x >= bbox.z || pixel.y < bbox.y || pixel.y >= bbox.w) {
    return 0.0;
  }
  let delta = vec2<f32>(pixel) + vec2<f32>(0.5) -
    splat.screen_center_depth_opacity.xy;
  let conic = splat.inverse_conic_and_qmax.xyz;
  let q = conic.x * delta.x * delta.x
    + 2.0 * conic.y * delta.x * delta.y
    + conic.z * delta.y * delta.y;
  return min(MAX_ALPHA, splat.screen_center_depth_opacity.w * exp(-0.5 * q));
}

@compute @workgroup_size(16, 16, 1)
fn rasterize_tile(
  @builtin(local_invocation_id) local_id: vec3<u32>,
  @builtin(workgroup_id) tile: vec3<u32>,
) {
  let pixel = vec2<u32>(
    tile.x * 16u + local_id.x,
    tile.y * 16u + local_id.y,
  );
  if (pixel.x >= params.width || pixel.y >= params.height) {
    return;
  }
  let tile_id = tile.y * params.tiles_x + tile.x;
  let begin = tile_offsets[tile_id];
  let end = tile_offsets[tile_id + 1u];
  var transmittance = 1.0;
  var rgb = vec3<f32>(0.0);
  for (var position = begin; position < end; position += 1u) {
    let entry = entries[sorted_entry_ids[position]];
    let splat = splats[entry.projected_index];
    let alpha = sample_alpha(splat, pixel);
    if (alpha < ALPHA_THRESHOLD) {
      continue;
    }
    let weight = transmittance * alpha;
    rgb += weight * splat.color_rgb_pad.xyz;
    transmittance *= 1.0 - alpha;
    if (transmittance < TRANSMITTANCE_EARLY_OUT) {
      break;
    }
  }
  textureStore(output_image, vec2<i32>(pixel), vec4<f32>(rgb, 1.0 - transmittance));
}
