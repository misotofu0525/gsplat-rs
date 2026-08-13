@group(0) @binding(0)
var<storage, read> sorted_index_words: array<u32>;
@group(0) @binding(1)
var<storage, read> source_elems: array<SurfaceSourceElem>;
@group(0) @binding(2)
var<storage, read> sh_rest: array<f32>;
@group(0) @binding(3)
var<uniform> params: Params;
@group(0) @binding(4)
var<storage, read_write> projected: array<ProjectedRecord>;

fn sh_rest_vec3(base: u32, per_channel: u32, coeff: u32) -> vec3<f32> {
  return vec3<f32>(
    sh_rest[base + coeff],
    sh_rest[base + per_channel + coeff],
    sh_rest[base + 2u * per_channel + coeff],
  );
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
    projected[i] = project_record(idx, source_elems[idx]);
  }
}
