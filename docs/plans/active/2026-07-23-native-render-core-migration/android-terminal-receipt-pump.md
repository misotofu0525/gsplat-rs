# Android strict terminal receipt pump

## Candidate boundary

- Parent: `4ffe7f4fbb0f8778a8becdb84dbf19a763691063`.
- Scope: native Surface receipt callback progress plus the Android strict
  terminal drain. The collector/schema/terminal ledgers are unchanged.
- Physical A065 execution remains a root-owned verification step.

## Lifecycle finding

Exact frames attach whole-plan completion and current-stats map callbacks to
the final command buffer before `queue.submit`. Successful Surface presentation
publishes the immutable submission and its already-issued ticket. A subsequent
ordinary render polls callbacks at frame entry, but it may also issue another
formal ticket. The `4ffe7f4` terminal drain removed those renders and retained
only instantaneous terminal polls, so Android had no bounded wait that required
the native queue to progress through the last submitted frame.

## Repair contract

`SurfaceRenderSession::pump_receipts(timeout)` waits only for queue work that
was already submitted when the call began. It does not acquire a Surface,
encode or submit commands, change camera state, request observer work, or
reserve/issue a ticket. Timeout remains pending under the caller's finite drain
bound; handle/device errors fail closed. After the wait, legacy sessions publish
completed compatibility telemetry and Exact sessions advance the mandatory
whole-plan callback. Existing current-stats terminal polls remain the sole
consumer of current-stats receipts.

The additive C/JNI pump is used only after all benchmark frames are recorded.
The Android loop performs no terminal-flush render and publishes no artifact if
the pump fails or terminal completeness is not reached within the fixed bound.

## Candidate-local evidence

- A Metal Exact test proves the pump completes an already-issued current-stats
  receipt and that the next explicit request/render receives exactly ticket
  `T+1`; the pump issued no hidden ticket.
- The Kotlin drain unit model proves callback-pump failure stops before receipt
  consumption/publication and incomplete terminals remain finitely bounded.
- Workspace Rust tests, Clippy, C FFI smoke, and strict Android
  collector/extractor fixtures pass locally.
- Gradle/Kotlin/JNI packaging is unavailable on this host because no Java
  runtime or Android SDK/NDK is installed; this is not counted as passing
  evidence.

## Root A065 verification

Build/install the exact candidate, then rerun the canonical A065
`2412x1080` Kitsune sequence with 20 warmup and 80 measured frames for CPU,
GPU, and Adaptive as required by the root matrix. Require one terminal Ready
receipt per issued measured ticket, complete summary/chunks, strict extractor
acceptance, and no terminal-flush render/submission ticket after measured frame
79. Any pump error, drain exhaustion, missing/duplicate ticket, identity drift,
or absent `GSPLAT_BENCHMARK_SUMMARY` rejects the candidate.
