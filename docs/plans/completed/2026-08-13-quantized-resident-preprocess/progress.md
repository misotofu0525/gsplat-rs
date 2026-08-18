# Progress

## 2026-08-13

- Phase 0 committed as `b9f2af9`.
- Compute preprocess is the default SortedAlpha path. SortedAlpha
  conformance still matches the previous vertex-stage evaluation.
- `ResidentStorageProfile::Quantized` packs a 32-byte hot record + per-degree
  u8 SH sidecars. GPU order keygen reads quantized f16 positions. Surface and
  offscreen request 8 storage buffers per stage when the adapter allows.

## Leftover closeout (same day)

- Per-degree SH sidecars (deg 1-4) land in the quantized preprocess; the
  largest degree-3 binding is 21 B/splat.
- Experimental GPU order composes with Quantized via a dedicated f16 keygen
  shader. Radix passes stay shared.
- Real-scene orbit-camera SSIM vs full-f32 (128², Metal): Kitsune 0.999429,
  Flowers 0.998745. Degree-3 synthetic SSIM ≥ 0.99.
- `bench-runner --storage-profile quantized` on Kitsune (279,199 deg-3):
  preflight fits, limiting resource Projected, avg GPU-complete 2.59 ms,
  0 missed frames at 16.67 ms.

## Verification (2026-08-13 leftover closeout)

| Command | Result |
|---------|--------|
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --offline` | pass |
| `GSPLAT_REQUIRE_GPU_CONFORMANCE=1` SortedAlpha | pass |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | pass |
| `bash tests/ffi/run-ffi-smoke.sh` | pass |
| `bench-runner --storage-profile quantized` minimal + Kitsune | pass |

## Android evidence path (same day)

- Surface can rebuild `ResidentSceneResources` after
  `set_storage_profile` without recreating the draw pipeline. Device
  already requests 8 storage buffers/stage when the adapter allows.
- Android sample-only knob: `gsplat_android_benchmark_set_storage_profile`
  (not in `gsplat.h`), JNI `BenchmarkBridge.setSurfaceStorageProfile`,
  extra `gsplat_surface_storage_profile`, collector `--storage-profile`.
- Call order: storage profile first, then order backend, so GPU order
  initializes against quantized buffers.

## Android A065 Kitsune (279,199, deg-3, 716×1600, thermal 0)

Local artifacts under `target/android-sort-benchmarks/`. Sequential
collector runs, not a randomized pair; do not treat CPU-time deltas as a
crossover decision.

| Run | avg_call_ms | missed / 80 | notes |
|-----|-------------|-------------|-------|
| CPU full-f32 | 20.699 | 80 | first presented frame 7.38 ms |
| CPU quantized | 9.486 | 0 | first presented frame 7.79 ms |
| GPU quantized | 21.993 | 80 | 80 GPU frames, 0 fallbacks; first presented frame 6.93 ms |

Quantized preprocess and f16 GPU-order keygen created on Vulkan with no
validation/`create_failed` errors. `avg_drawn=279199` on all three.
Host: collector unit tests, `cargo check`/`clippy` on the touched crates.

## Web evidence (same day)

Create-time profile on `gsplat-web` (optional 5th `createRenderer` token).
Example extra `gsplat_surface_storage_profile`; collector
`GSPLAT_STORAGE_PROFILE`. Quantized collection fails closed unless the
artifact is `resident_sorted_indices` + `webgpu`.

Headless Chrome 151, 1280×720, Kitsune 279,199 deg-3, warmup 20 + 80
sync frames, `sort_interval=2`:

| Run | avg_call_ms | first call_ms | missed / 80 |
|-----|-------------|---------------|-------------|
| full-f32 | 2.161 | 2.20 | 1 (max 79.7 ms) |
| quantized | 1.486 | 2.40 | 0 (max 6.0 ms) |

Both `avg_drawn=279199`, `storage_profile_requested` matches. Minimal
quantized smoke also passed (3 splats). Sync collector times are CPU
call/submit, not GPU-complete. Artifacts under
`target/benchmarks/phase-a/web-*-v1`.
