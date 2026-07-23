// One coherent SH0-SH3 color evaluation per resident splat.

struct ColorAux {
  dc_xy: u32,
  dc_z: u32,
};

struct ChunkMeta {
  dc_min: vec4<f32>,
  dc_extent: vec4<f32>,
  sh_scale_l1: vec4<f32>,
  sh_scale_l2: vec4<f32>,
  sh_scale_l3: vec4<f32>,
};

struct Params {
  camera_pos: vec4<f32>,
  len: u32,
  sh_degree: u32,
  _reserved0: u32,
  _pad: u32,
};

@group(0) @binding(0)
var<storage, read> position_alpha: array<vec4<f32>>;
@group(0) @binding(1)
var<storage, read> color_auxiliary: array<ColorAux>;
@group(0) @binding(2)
var<storage, read> sh_plane0: array<vec4<u32>>;
@group(0) @binding(3)
var<storage, read> sh_plane1: array<vec4<u32>>;
@group(0) @binding(4)
var<storage, read> sh_plane2: array<vec4<u32>>;
@group(0) @binding(5)
var<storage, read> sh_plane3: array<vec4<u32>>;
@group(0) @binding(6)
var<storage, read> chunk_metadata: array<ChunkMeta>;
@group(0) @binding(7)
var<storage, read_write> resolved_color: array<vec2<u32>>;
@group(0) @binding(8)
var<uniform> params: Params;

fn decode_u16(bits: u32, minimum: f32, extent: f32) -> f32 {
  return minimum + (f32(bits & 0xffffu) / 65535.0) * extent;
}

fn sh_word(slot: u32, word_index: u32) -> u32 {
  let lane = word_index & 3u;
  if (word_index < 4u) {
    return sh_plane0[slot][lane];
  }
  if (word_index < 8u) {
    return sh_plane1[slot][lane];
  }
  if (word_index < 12u) {
    return sh_plane2[slot][lane];
  }
  return sh_plane3[slot][lane];
}

fn decode_packed_bits(slot: u32, bit_offset: u32, bit_count: u32) -> u32 {
  let word_index = bit_offset >> 5u;
  let shift = bit_offset & 31u;
  var bits = sh_word(slot, word_index) >> shift;
  if (shift + bit_count > 32u) {
    bits = bits | (sh_word(slot, word_index + 1u) << (32u - shift));
  }
  return bits & ((1u << bit_count) - 1u);
}

fn decode_sh11(slot: u32, logical_value: u32) -> f32 {
  let bits = decode_packed_bits(slot, logical_value * 11u, 11u);
  return f32(bitcast<i32>((bits & 0x7ffu) << 21u) >> 21u);
}

fn sh_value_count() -> u32 {
  if (params.sh_degree == 1u) {
    return 9u;
  }
  if (params.sh_degree == 2u) {
    return 24u;
  }
  return 45u;
}

fn sh_band_index(coefficient: u32) -> u32 {
  if (coefficient < 3u) {
    return 0u;
  }
  if (coefficient < 8u) {
    return 1u;
  }
  return 2u;
}

fn sh_point_scales(slot: u32) -> vec3<f32> {
  let bit_offset = sh_value_count() * 11u;
  return vec3<f32>(
    f32(decode_packed_bits(slot, bit_offset, 5u)),
    f32(decode_packed_bits(slot, bit_offset + 5u, 5u)),
    f32(decode_packed_bits(slot, bit_offset + 10u, 5u)),
  ) / 31.0;
}

fn sh_scale(chunk: ChunkMeta, coefficient: u32) -> vec3<f32> {
  if (coefficient < 3u) {
    return chunk.sh_scale_l1.xyz;
  }
  if (coefficient < 8u) {
    return chunk.sh_scale_l2.xyz;
  }
  return chunk.sh_scale_l3.xyz;
}

fn sh_vec3(slot: u32, coefficient: u32, chunk: ChunkMeta, point_scales: vec3<f32>) -> vec3<f32> {
  let logical = coefficient * 3u;
  let quantized = vec3<f32>(
    decode_sh11(slot, logical),
    decode_sh11(slot, logical + 1u),
    decode_sh11(slot, logical + 2u),
  );
  return quantized / 1023.0 * sh_scale(chunk, coefficient) * point_scales[sh_band_index(coefficient)];
}

fn normalize_or_default(v: vec3<f32>) -> vec3<f32> {
  let len2 = dot(v, v);
  if (len2 <= 1e-20) {
    return vec3<f32>(0.0, 0.0, 1.0);
  }
  return v * inverseSqrt(len2);
}

fn eval_sh(slot: u32, dc: vec3<f32>, dir: vec3<f32>, chunk: ChunkMeta) -> vec3<f32> {
  let c0 = 0.28209479177387814;
  let c1 = 0.4886025119029199;
  let c2 = array<f32, 5>(
    1.0925484305920792,
    -1.0925484305920792,
    0.31539156525252005,
    -1.0925484305920792,
    0.5462742152960396,
  );
  let c3 = array<f32, 7>(
    -0.5900435899266435,
    2.890611442640554,
    -0.4570457994644658,
    0.3731763325901154,
    -0.4570457994644658,
    1.445305721320277,
    -0.5900435899266435,
  );
  var result = c0 * dc;
  if (params.sh_degree == 0u) {
    return result;
  }
  let point_scales = sh_point_scales(slot);
  let x = dir.x;
  let y = dir.y;
  let z = dir.z;
  result = result
    - c1 * y * sh_vec3(slot, 0u, chunk, point_scales)
    + c1 * z * sh_vec3(slot, 1u, chunk, point_scales)
    - c1 * x * sh_vec3(slot, 2u, chunk, point_scales);
  if (params.sh_degree == 1u) {
    return result;
  }
  let xx = x * x;
  let yy = y * y;
  let zz = z * z;
  let xy = x * y;
  let yz = y * z;
  let xz = x * z;
  result = result
    + c2[0] * xy * sh_vec3(slot, 3u, chunk, point_scales)
    + c2[1] * yz * sh_vec3(slot, 4u, chunk, point_scales)
    + c2[2] * (2.0 * zz - xx - yy) * sh_vec3(slot, 5u, chunk, point_scales)
    + c2[3] * xz * sh_vec3(slot, 6u, chunk, point_scales)
    + c2[4] * (xx - yy) * sh_vec3(slot, 7u, chunk, point_scales);
  if (params.sh_degree == 2u) {
    return result;
  }
  result = result
    + c3[0] * y * (3.0 * xx - yy) * sh_vec3(slot, 8u, chunk, point_scales)
    + c3[1] * xy * z * sh_vec3(slot, 9u, chunk, point_scales)
    + c3[2] * y * (4.0 * zz - xx - yy) * sh_vec3(slot, 10u, chunk, point_scales)
    + c3[3] * z * (2.0 * zz - 3.0 * xx - 3.0 * yy) * sh_vec3(slot, 11u, chunk, point_scales)
    + c3[4] * x * (4.0 * zz - xx - yy) * sh_vec3(slot, 12u, chunk, point_scales)
    + c3[5] * z * (xx - yy) * sh_vec3(slot, 13u, chunk, point_scales)
    + c3[6] * x * (xx - 3.0 * yy) * sh_vec3(slot, 14u, chunk, point_scales);
  return result;
}

// 64-bit non-negative RGB: one shared binary exponent and 18 exact mantissa
// bits per channel. This keeps highlights above 1.0 without the relative
// precision loss of RGB16F in the common [0, 2] range.
fn pack_rgb18e8(rgb: vec3<f32>) -> vec2<u32> {
  let nonnegative = max(rgb, vec3<f32>(0.0));
  let maximum = max(nonnegative.x, max(nonnegative.y, nonnegative.z));
  if (maximum == 0.0) {
    return vec2<u32>(0u);
  }
  let exponent = clamp(i32(ceil(log2(maximum))), -126, 127);
  let exponent_code = u32(exponent + 127);
  let scale = exp2(f32(exponent));
  let mantissa_max = 262143.0;
  let q = vec3<u32>(round(clamp(nonnegative / scale, vec3<f32>(0.0), vec3<f32>(1.0)) * mantissa_max));
  return vec2<u32>(
    (q.x & 0x3ffffu) | ((q.y & 0x3fffu) << 18u),
    ((q.y >> 14u) & 0xfu) | ((q.z & 0x3ffffu) << 4u) | (exponent_code << 22u),
  );
}

@compute @workgroup_size(128)
fn main(
  @builtin(workgroup_id) workgroup_id: vec3<u32>,
  @builtin(num_workgroups) num_workgroups: vec3<u32>,
  @builtin(local_invocation_index) local_index: u32,
) {
  let logical_group = workgroup_id.x + workgroup_id.y * num_workgroups.x;
  let slot = logical_group * 128u + local_index;
  if (slot >= params.len) {
    return;
  }
  let chunk = chunk_metadata[slot >> 8u];
  let dc_bits = color_auxiliary[slot];
  let dc = vec3<f32>(
    decode_u16(dc_bits.dc_xy, chunk.dc_min.x, chunk.dc_extent.x),
    decode_u16(dc_bits.dc_xy >> 16u, chunk.dc_min.y, chunk.dc_extent.y),
    decode_u16(dc_bits.dc_z, chunk.dc_min.z, chunk.dc_extent.z),
  );
  let direction = normalize_or_default(position_alpha[slot].xyz - params.camera_pos.xyz);
  let rgb = eval_sh(slot, dc, direction, chunk) + vec3<f32>(0.5);
  resolved_color[slot] = pack_rgb18e8(rgb);
}
