# iPhone Native Metal Training Feasibility

## Status

- Active research plan. No implementation has started from this bundle.

## Goal

- Evaluate whether local, no-server Gaussian splat model creation on iPhone should be built as a native Metal training stack.
- Use Brush and commercial apps such as Scaniverse as feasibility references, not as the implementation substrate.
- Keep the current `gsplat-rs` boundary clear: this repository remains the renderer/SDK surface unless a separate training integration is deliberately added.

## Findings

- Brush is a strong candidate for local training research because it is Rust-based, Apache-2.0 licensed, and uses Burn plus wgpu/WebGPU rather than a CUDA-only stack.
- Brush main currently documents desktop, Android, and browser support. It does not include an iOS app/library path on `main`.
- Brush accepts COLMAP data or Nerfstudio-format datasets; it does not solve full on-device capture, camera tracking, or reconstruction dataset creation by itself.
- The browser path is not enough for iPhone product use. Brush's web demo expects Chromium and `showDirectoryPicker`; current Safari/iOS compatibility still blocks `showDirectoryPicker`, even though WebGPU itself is now available on recent iOS Safari.
- Brush has an open, unmerged iOS setup PR from 2024. It added a `brush-ios` staticlib and IPA packaging script, but the branch is stale and very large relative to current `main`; it is evidence that an iOS port is plausible, not something to merge wholesale.
- Commercial use is plausible from the repository license side because Brush is Apache-2.0, but productization still needs third-party dependency/license review and App Store policy review.
- The preferred product direction is now native iOS/Metal, not Brush/WebGPU/wgpu-on-iOS. Brush can still inform the training loop, data layout, densification/pruning schedule, export format, and benchmark targets.
- A Metal-native path should keep capture, training, and rendering as separate layers: ARKit capture produces posed frames and intrinsics; the Metal trainer optimizes splats; `gsplat-rs`/`GsplatKit` renders exported or live splats.
- The first hard technical question is not whether Metal can run the math, but whether a small enough on-device training formulation can hit acceptable memory, thermal, and wall-clock limits on target iPhones.

## Recommended Direction

1. [pending] Define a minimal native Metal trainer contract: input posed frames, intrinsics, initial sparse points, iteration budget, memory budget, and `.ply`/live-buffer output.
2. [pending] Build a tiny Metal compute spike for one forward projection/raster/loss pass over a fixed miniature splat set and one camera frame.
3. [pending] Add the backward/update path only after the forward pass is validated against a CPU reference or Brush/gsplat reference data.
4. [pending] Keep ARKit capture and dataset creation as a separate spike: frames, poses, intrinsics, sparse initialization, and optional Nerfstudio `transforms.json` export for debugging.
5. [pending] Feed the trainer output into `gsplat-rs`/`GsplatKit` to verify render compatibility and preserve the current renderer boundary.
6. [pending] Use Brush on Mac as a reference benchmark/export oracle, not as iOS runtime code.

## Sources

- Brush repository: https://github.com/ArthurBrussee/brush
- Brush iOS setup issue: https://github.com/ArthurBrussee/brush/issues/27
- Brush iOS setup PR: https://github.com/ArthurBrussee/brush/pull/52
- WebGPU compatibility: https://caniuse.com/webgpu
- `showDirectoryPicker` compatibility: https://caniuse.com/mdn-api_window_showdirectorypicker
- MDN `showDirectoryPicker`: https://developer.mozilla.org/en-US/docs/Web/API/Window/showDirectoryPicker
