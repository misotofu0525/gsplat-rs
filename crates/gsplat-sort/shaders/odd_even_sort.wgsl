struct Pair {
  key: u32,
  value: u32,
};

struct Params {
  pass_index: u32,
  len: u32,
  _pad0: u32,
  _pad1: u32,
};

@group(0) @binding(0)
var<storage, read_write> pairs: array<Pair>;

@group(0) @binding(1)
var<uniform> params: Params;

fn should_swap(a: Pair, b: Pair) -> bool {
  if (a.key < b.key) {
    return true;
  }

  if (a.key == b.key && a.value > b.value) {
    return true;
  }

  return false;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let lane = gid.x;
  let start = lane * 2u + (params.pass_index & 1u);
  if (start + 1u >= params.len) {
    return;
  }

  let a = pairs[start];
  let b = pairs[start + 1u];

  if (should_swap(a, b)) {
    pairs[start] = b;
    pairs[start + 1u] = a;
  }
}
