struct InstanceData {
  pos_xy: vec2<f32>,
  size: f32,
  _pad0: f32,
  color: vec4<f32>,
};

@group(0) @binding(0)
var<storage, read> instances: array<InstanceData>;

struct VsOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
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

@vertex
fn vs_main(
  @builtin(instance_index) instance_index: u32,
  @builtin(vertex_index) vertex_index: u32,
) -> VsOut {
  let instance = instances[instance_index];
  let offset = quad_offset(vertex_index) * instance.size;

  var out: VsOut;
  out.position = vec4<f32>(instance.pos_xy + offset, 0.0, 1.0);
  out.color = instance.color;
  return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
  return input.color;
}
