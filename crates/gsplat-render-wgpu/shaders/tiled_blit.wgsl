@group(0) @binding(0)
var source_image: texture_2d<f32>;

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
  let positions = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 3.0, -1.0),
    vec2<f32>(-1.0,  3.0),
  );
  var out: VertexOutput;
  out.position = vec4<f32>(positions[index], 0.0, 1.0);
  return out;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  return textureLoad(source_image, vec2<i32>(position.xy), 0);
}
