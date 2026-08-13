struct QuantizedSource {
  pos_xy: u32,
  pos_z_alpha: u32,
  rotation: u32,
  scale_rgb: u32,
  color_dc: u32,
  _pad0: u32,
  _pad1: u32,
  _pad2: u32,
};

@group(0) @binding(0)
var<storage, read> sorted_index_words: array<u32>;
@group(0) @binding(1)
var<storage, read> source_elems: array<QuantizedSource>;
@group(0) @binding(2)
var<storage, read> sh1: array<u32>;
@group(0) @binding(3)
var<uniform> params: Params;
@group(0) @binding(4)
var<storage, read_write> projected: array<ProjectedRecord>;
@group(0) @binding(5)
var<storage, read> sh2: array<u32>;
@group(0) @binding(6)
var<storage, read> sh3: array<u32>;
@group(0) @binding(7)
var<storage, read> sh4: array<u32>;

const SQRT_ONE_HALF: f32 = 0.7071067811865476;
const COLOR_SCALE: f32 = 0.15;

fn sh_byte_from(word: u32, byte_index: u32) -> f32 {
  let packed = (word >> ((byte_index % 4u) * 8u)) & 255u;
  return (f32(packed) - 128.0) / 128.0;
}

fn sh1_byte(byte_index: u32) -> f32 {
  return sh_byte_from(sh1[byte_index / 4u], byte_index);
}

fn sh2_byte(byte_index: u32) -> f32 {
  return sh_byte_from(sh2[byte_index / 4u], byte_index);
}

fn sh3_byte(byte_index: u32) -> f32 {
  return sh_byte_from(sh3[byte_index / 4u], byte_index);
}

fn sh4_byte(byte_index: u32) -> f32 {
  return sh_byte_from(sh4[byte_index / 4u], byte_index);
}

fn sidecar_vec3(idx: u32, coeffs: u32, local: u32, degree: u32) -> vec3<f32> {
  let base = idx * coeffs * 3u;
  let r = base + local;
  let g = base + coeffs + local;
  let b = base + 2u * coeffs + local;
  if (degree == 1u) {
    return vec3<f32>(sh1_byte(r), sh1_byte(g), sh1_byte(b));
  }
  if (degree == 2u) {
    return vec3<f32>(sh2_byte(r), sh2_byte(g), sh2_byte(b));
  }
  if (degree == 3u) {
    return vec3<f32>(sh3_byte(r), sh3_byte(g), sh3_byte(b));
  }
  return vec3<f32>(sh4_byte(r), sh4_byte(g), sh4_byte(b));
}

fn sh_rest_vec3(base: u32, per_channel: u32, coeff: u32) -> vec3<f32> {
  let idx = select(0u, base / (per_channel * 3u), per_channel > 0u);
  if (coeff < 3u) {
    return sidecar_vec3(idx, 3u, coeff, 1u);
  }
  if (coeff < 8u) {
    return sidecar_vec3(idx, 5u, coeff - 3u, 2u);
  }
  if (coeff < 15u) {
    return sidecar_vec3(idx, 7u, coeff - 8u, 3u);
  }
  return sidecar_vec3(idx, 9u, coeff - 15u, 4u);
}

fn decode_smallest_three(packed_in: u32) -> vec4<f32> {
  var packed = packed_in;
  let largest = packed >> 30u;
  var rotation = vec4<f32>(0.0);
  var sum_squares = 0.0;
  for (var step = 0u; step < 4u; step++) {
    let index = 3u - step;
    if (index == largest) {
      continue;
    }
    let magnitude = packed & 0x1ffu;
    let negative = (packed >> 9u) & 1u;
    packed = packed >> 10u;
    let value = SQRT_ONE_HALF * f32(magnitude) / 511.0;
    let signed_value = select(value, -value, negative != 0u);
    if (index == 0u) { rotation.x = signed_value; }
    else if (index == 1u) { rotation.y = signed_value; }
    else if (index == 2u) { rotation.z = signed_value; }
    else { rotation.w = signed_value; }
    sum_squares = sum_squares + value * value;
  }
  let largest_value = sqrt(max(1.0 - sum_squares, 0.0));
  if (largest == 0u) { rotation.x = largest_value; }
  else if (largest == 1u) { rotation.y = largest_value; }
  else if (largest == 2u) { rotation.z = largest_value; }
  else { rotation.w = largest_value; }
  return rotation;
}

fn quat_to_mat3(q: vec4<f32>) -> mat3x3<f32> {
  let n2 = dot(q, q);
  let qn = select(vec4<f32>(0.0, 0.0, 0.0, 1.0), q * inverseSqrt(n2), n2 > 0.0);
  let x = qn.x;
  let y = qn.y;
  let z = qn.z;
  let w = qn.w;
  let xx = x * x;
  let yy = y * y;
  let zz = z * z;
  let xy = x * y;
  let xz = x * z;
  let yz = y * z;
  let wx = w * x;
  let wy = w * y;
  let wz = w * z;
  return mat3x3<f32>(
    vec3<f32>(1.0 - 2.0 * (yy + zz), 2.0 * (xy + wz), 2.0 * (xz - wy)),
    vec3<f32>(2.0 * (xy - wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz + wx)),
    vec3<f32>(2.0 * (xz + wy), 2.0 * (yz - wx), 1.0 - 2.0 * (xx + yy)),
  );
}

fn dequant_source(elem: QuantizedSource) -> SurfaceSourceElem {
  let pos_xy = unpack2x16float(elem.pos_xy);
  let pos_za = unpack2x16float(elem.pos_z_alpha);
  let q = decode_smallest_three(elem.rotation);
  let scale_u = elem.scale_rgb;
  let log_s = vec3<f32>(
    f32(scale_u & 255u) / 16.0 - 10.0,
    f32((scale_u >> 8u) & 255u) / 16.0 - 10.0,
    f32((scale_u >> 16u) & 255u) / 16.0 - 10.0,
  );
  let s = exp(log_s);
  let s2 = max(s * s, vec3<f32>(1e-12));
  let r = quat_to_mat3(q);
  let rs = mat3x3<f32>(r[0] * s2.x, r[1] * s2.y, r[2] * s2.z);
  let world = rs * transpose(r);
  let dc_u = elem.color_dc;
  let dc = vec3<f32>(
    (f32(dc_u & 255u) / 255.0 - 0.5) / COLOR_SCALE,
    (f32((dc_u >> 8u) & 255u) / 255.0 - 0.5) / COLOR_SCALE,
    (f32((dc_u >> 16u) & 255u) / 255.0 - 0.5) / COLOR_SCALE,
  );
  var source: SurfaceSourceElem;
  source.position = vec4<f32>(pos_xy.x, pos_xy.y, pos_za.x, 0.0);
  source.covariance0 = vec4<f32>(world[0][0], world[1][0], world[2][0], world[1][1]);
  source.covariance1 = vec4<f32>(world[2][1], world[2][2], pos_za.y, 0.0);
  source.color_dc = vec4<f32>(dc, 0.0);
  return source;
}

const WORKGROUP_SIZE: u32 = 64u;
const ITEMS_PER_THREAD: u32 = 4u;

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let base = gid.x * ITEMS_PER_THREAD;
  for (var k = 0u; k < ITEMS_PER_THREAD; k++) {
    let i = base + k;
    if (i >= params.len) {
      return;
    }
    let order_word = i * params.order_stride_words + params.order_id_offset_words;
    let idx = sorted_index_words[order_word];
    projected[i] = project_record(idx, dequant_source(source_elems[idx]));
  }
}
