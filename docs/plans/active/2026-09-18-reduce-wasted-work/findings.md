# Findings: Reduce Wasted Work

Environment: Linux x86_64, 4 cores, Mesa Lavapipe `llvmpipe (LLVM 20.1.2,
256 bits)` over Vulkan, Xvfb for the interactive viewer. Kitsune CC0 scene
(279,199 splats, SH degree 3). Lavapipe timings are software-rasterizer
evidence for the direction of each change, not device performance.

## Baseline observations

- The default `Camera::default()` view of Kitsune keeps 70,402 splats after
  near/far culling but only 1,123 (256²) / 2,838 (1280×720) of them are inside
  the NDC footprint. The projected-record pass therefore evaluated degree-3 SH
  for ~96% culled records on that view.
- The offscreen bench (`bench-runner`) is fragment-bound on Lavapipe for that
  view (a few very large on-screen splats), so `avg_gpu_complete_frame_ms`
  hides compute-pass changes: 27.6–28.5 ms before and 27.7–28.2 ms after
  the whole series. The isolated projection-only test hook was added to measure the
  compute pass by itself.
- One early baseline bench (67 ms) ran while a background compile was
  active; it was discarded. All retained numbers were taken on an idle box.
- `bench-runner --stability-seconds` RSS grows ~170 MB during the first 60
  frames on Lavapipe and then plateaus (same max at 5 s and 15 s); this is
  driver warm-up, not a leak, and is identical before and after.

## Per-item evidence

### 1. SH early-out (`splat_common.wgsl`)

Isolated projection pass, `cargo test -p gsplat-render-wgpu
stationary_projection_cache -- --nocapture`, 40 frames each:

| Scene / camera | on-screen / visible | before | after |
| --- | --- | --- | --- |
| Kitsune, `Camera::default()` | 1,123 / 70,402 | 1.92–1.97 ms | 1.24–1.26 ms |
| Kitsune, orbit (all on-screen) | 279,199 / 279,199 | 4.74–5.37 ms | 4.08–4.28 ms |
| synthetic 200k, every other splat off-screen | 100,000 / 200,000 | 4.12–4.15 ms | 4.08–4.18 ms |

The saving appears when culled records are coherent in the sorted order
(whole SIMD groups skip the `sh_rest` reads). Interleaved culling (the
synthetic case) is divergence-bound and unchanged within noise.
Image-exact: `sha256sum` of the desktop PNGs is identical before/after for
Kitsune default camera (`3dbaeb74…`), Kitsune `--auto-camera`
(`cc86dc70…`), and `minimal_ascii` (`0181234e…`).

### 2. Stationary projection cache (`resident.rs`)

Same test, `projection_pass_ms` (forced re-projection) vs `cached_ms`:

| Scene | forced | cached |
| --- | --- | --- |
| Kitsune orbit, 279k | 4.08–4.28 ms | 0.005 ms |
| Kitsune default camera | 1.24–1.26 ms | 0.005 ms |
| synthetic 200k | 4.08–4.18 ms | 0.005 ms |

The cached draw is asserted pixel-identical to a forced re-projection, and
a 1e-3 camera translation without an order upload must re-dispatch.

### 3. On-demand rendering (`surface_session.rs`, desktop viewer)

`desktop-example --features interactive-viewer -- tests/datasets/minimal_ascii.ply
--auto-camera --interactive` under Xvfb, 10 s wall, viewer process CPU time
from `/proc/<pid>/stat`:

| Scene / mode | before | after |
| --- | --- | --- |
| minimal, stationary (`--auto-camera`) | 30.64 s | 0.13 s |
| minimal, moving (`--auto-camera --orbit`) | 30.60 s | 30.40 s |
| Kitsune, stationary | 20.84 s | 2.84 s |
| Kitsune, moving (`--orbit`) | 18.56 s | 18.50 s |

The Kitsune numbers include ~2.5 s of debug-build PLY load. The Xvfb
root-window capture after the stationary run is pixel-identical (ImageMagick
`compare -metric AE` = 0) between the base build (continuous redraw) and the
final build (idle after the first frame). A measurement script that pipes the
viewer into `head` dies of SIGPIPE before killing its children; runs recorded
here were taken with no leftover viewer or Xvfb process.

### 4. Async sort workspace reuse (`surface_async.rs`)

`cargo test -p gsplat-render-wgpu surface_async -- --nocapture`:

- `warm_workspace_and_recycled_indices_allocate_nothing`: the second request
  returns the same `indices` pointer and capacity and leaves the depth-key
  capacity unchanged.
- `worker_shares_the_scene_and_reuses_the_recycled_index_buffer`: through the
  spawned worker, the recycled buffer comes back with the same pointer.
- `workspace_reuse_microbench` (Kitsune, 70,402 visible, 20 iterations):
  debug 12.2–12.3 ms fresh vs 11.4–11.5 ms warm; `--release` 1.39–1.40 ms
  fresh vs 0.94–0.96 ms warm. Before, every request allocated two index/key
  vectors plus a fresh `CpuSortBackend` (~3.4 MB for this scene) and the
  session dropped the displayed buffer on apply.

### 5. CPU copy convergence (`lib.rs`, `resident.rs`, `quantized.rs`)

`cargo run --release -p bench-runner -- kitune1.ply --stability-seconds 5`:

| | before | after |
| --- | --- | --- |
| `rss_min_kib` (after load) | 175,584 | 157,904 / 157,920 |
| `rss_max_kib` | 346,276 | 328,832 / 328,776 |

The 17.3 MiB drop matches the removed 64 B/splat of retained derived arrays
(36 B world covariance + 24 B covariance terms + 4 B alpha) for 279,199
splats. The 64 B/splat transient `Vec` of GPU records at upload is gone as
well (records are written into the mapped buffer). The async worker's 12
B/splat positions copy is replaced by an `Arc<SceneBuffers>` share. PNG
hashes unchanged.
