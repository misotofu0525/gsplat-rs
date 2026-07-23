// Hardware SortedAlpha draw over a stable compacted prefix of projected ranks.

@group(0) @binding(0)
var<storage, read> projected_center_source: array<vec4<f32>>;
@group(0) @binding(1)
var<storage, read> projected_axes: array<vec4<f32>>;
@group(0) @binding(2)
var<storage, read> resolved_color: array<vec2<u32>>;
@group(0) @binding(3)
var<storage, read> contributor_ranks: array<u32>;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) local: vec2<f32>,
};

fn quad_offset(vertex_index: u32) -> vec2<f32> {
  let offsets = array<vec2<f32>, 4>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>(-1.0,  1.0),
    vec2<f32>( 1.0,  1.0),
  );
  return offsets[vertex_index];
}

fn alpha_extent_scale(alpha: f32) -> f32 {
  return sqrt(clamp(log(max(alpha, 1e-12) * 256.0) / 4.5, 0.0, 1.0));
}

fn unpack_color_rgb18e8(bits: vec2<u32>) -> vec3<f32> {
  let exponent_code = (bits.y >> 22u) & 0xffu;
  if (exponent_code == 0u) {
    return vec3<f32>(0.0);
  }
  let r = bits.x & 0x3ffffu;
  let g = (bits.x >> 18u) | ((bits.y & 0xfu) << 14u);
  let b = (bits.y >> 4u) & 0x3ffffu;
  let scale = exp2(f32(i32(exponent_code) - 127));
  return vec3<f32>(f32(r), f32(g), f32(b)) * (scale / 262143.0);
}

@vertex
fn vs_main(
  @builtin(instance_index) instance_index: u32,
  @builtin(vertex_index) vertex_index: u32,
) -> VsOut {
  let candidate_rank = contributor_ranks[instance_index];
  let center_source = projected_center_source[candidate_rank];
  var out: VsOut;
  let axes = projected_axes[candidate_rank];
  let source_id = bitcast<u32>(center_source.w);
  let local = quad_offset(vertex_index) * alpha_extent_scale(center_source.z);
  let offset = axes.xy * local.x + axes.zw * local.y;
  out.position = vec4<f32>(center_source.xy + offset, 0.0, 1.0);
  out.color = vec4<f32>(unpack_color_rgb18e8(resolved_color[source_id]), center_source.z);
  out.local = local;
  return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
  let r2 = dot(input.local, input.local);
  let g = exp(-4.5 * r2);
  let alpha = min(0.99, input.color.a * g);
  if (alpha < (1.0 / 255.0)) {
    discard;
  }
  return vec4<f32>(input.color.rgb * alpha, alpha);
}
