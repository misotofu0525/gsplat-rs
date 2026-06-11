struct InstanceData {
  // xy = center in NDC, zw = major axis in NDC
  center_and_axis_u: vec4<f32>,
  // xy = minor axis in NDC
  axis_v_and_pad: vec4<f32>,
  color: vec4<f32>,
};

@group(0) @binding(0)
var<storage, read> instances: array<InstanceData>;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) local: vec2<f32>,
};

fn quad_offset(vertex_index: u32) -> vec2<f32> {
  let offsets = array<vec2<f32>, 6>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>( 1.0,  1.0),
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0,  1.0),
    vec2<f32>(-1.0,  1.0),
  );
  return offsets[vertex_index];
}

// fs_main discards fragments with alpha * exp(-4.5 * r2) <= 1/256, so the
// quad only needs to cover r2 <= ln(256 * alpha) / 4.5. Scaling the offset and
// the local coordinate by the same factor keeps the pixel -> gaussian mapping
// identical while skipping rasterization of fragments that would be discarded.
fn alpha_extent_scale(alpha: f32) -> f32 {
  let s2 = log(max(alpha, 1e-12) * 256.0) / 4.5;
  return sqrt(clamp(s2, 0.0, 1.0));
}

@vertex
fn vs_main(
  @builtin(instance_index) instance_index: u32,
  @builtin(vertex_index) vertex_index: u32,
) -> VsOut {
  let instance = instances[instance_index];
  let center = instance.center_and_axis_u.xy;
  let axis_u = instance.center_and_axis_u.zw;
  let axis_v = instance.axis_v_and_pad.xy;
  let local = quad_offset(vertex_index) * alpha_extent_scale(instance.color.a);
  let offset = axis_u * local.x + axis_v * local.y;

  var out: VsOut;
  out.position = vec4<f32>(center + offset, 0.0, 1.0);
  out.color = instance.color;
  out.local = local;
  return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
  let r2 = dot(input.local, input.local);
  if (r2 > 1.0) {
    discard;
  }

  // Axis vectors encode a 3-sigma ellipse, so local-space exponent scales by 9.
  let g = exp(-4.5 * r2);
  let alpha = input.color.a * g;
  if (alpha <= (1.0 / 256.0)) {
    discard;
  }
  return vec4<f32>(input.color.rgb * g, alpha);
}
