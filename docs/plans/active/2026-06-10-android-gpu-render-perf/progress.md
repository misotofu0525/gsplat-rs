# Progress

## 2026-06-10

- Created plan bundle. Phase 0: built/installed sample APK on `033ed212`,
  pushed `flowers_1.ply`, recorded baselines (interval 2: call 51.955 /
  frame 36.234; interval 1: call 52.308 / frame 41.586).
- Phase 1: attribution diagnostics. SH=0 -> call 33.0 (~19ms VS SH cost);
  quad x0.5 -> call 38.6 (~18ms fragment fill). Local alpha histogram:
  0 cullable splats, 21% theoretical area reduction from alpha-aware shrink.
- Phase 2 Opt A: VS-only `alpha_extent_scale` quad shrink in
  `splat_surface.wgsl`, `splat_surface_direct.wgsl`, `splat.wgsl`.
  Device: call 51.955 -> 38.522 (-25.9%). Desktop PNG diff max 1/255.
- Phase 2 Opt B (compute color prepass): call 159.171, reverted cleanly
  (lib.rs restored from git, shader file deleted).
- Phase 2 Opt B': vectorized 3-channel SH evaluation in VS
  (`eval_sh_rgb`/`sh_rest_vec3`, identical accumulation order) in
  `splat_surface.wgsl` and `splat_surface_direct.wgsl`.
  Device: call 32.793, at the SH=0 diagnostic floor.
- Final retained default: call 33.192 / frame 30.298, drawn 562974/562974.
  Opt-in paths reversed for the first time: gpu_preproject 30.504,
  static_direct 29.820 (both still opt-in).
- Phase 3: device screencap A/B (run noise = 0 bytes; before/after max
  delta 12/255 on 0.0035% subpixels >2). Regression set green (fmt, clippy,
  tests, docs, wasm check, bench-runner). Normal-mode launch verified.
- Awaiting user confirmation before any commit.
- Follow-up (user request): promoted `static_direct` to the default render
  path. Flipped defaults in FFI core, Android/Apple binding options, and both
  example apps; synced READMEs, iOS benchmark script, and ARCHITECTURE Web
  note. Verified: cargo check/test/fmt/clippy, FFI/JNI/Swift smokes,
  XCFramework + GsplatKit sim build, device default-config benchmark
  (`static_direct=true`, cold call 29.973ms) and normal-mode render
  (raster=0.00ms, drawn full). Thermal throttling noted as an open question
  for sustained load. Still awaiting commit confirmation.
- iOS true-device benchmark (iPhone 17 Pro Max): default config confirms
  `static_direct=true`, call 17.79/17.24ms across two runs vs CPU-path
  control 17.62ms (present-bound, no regression); direct path cuts native
  frame prep 9.95 -> ~5.9ms. Cross-platform validation for the default flip
  is complete except sustained-thermal study.
