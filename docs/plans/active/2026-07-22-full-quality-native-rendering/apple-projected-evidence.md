# Apple projected-draw integration evidence

Date: 2026-07-23 (Asia/Shanghai)

## Scope and evidence boundary

The current Apple environment has no physical iPhone (`xcrun devicectl list
devices` reports no devices). The available runtime is an iPhone 17 Pro iOS
26.2 Simulator in landscape, with an actual drawable of 2622x1206. Simulator
runs qualify the Apple wrapper, C ABI integration, artifact schema, full-size
presentation, and Candidate correctness path. They are not physical-device
Metal performance evidence.

The Simulator adapter reports a 268,435,456-byte storage-buffer binding limit
and 31 storage buffers per shader stage. It does not expose the exact Compact
projected graph or the GPU order lane used by this implementation. A forced
Compact configuration therefore fails before publication with structured
`GSPLAT_UNSUPPORTED` (`rc=4`):

```text
exact projected contributor compaction is unavailable on this surface adapter
```

Adaptive consequently reports `candidate_only`. It executes Candidate without
fabricating a Compact comparison, projected probe ticket, or terminal receipt.
This is the correct capability-declared behavior, not Candidate-versus-Compact
performance evidence.

## Wrapper and artifact contract

The Apple wrapper and iOS example now use the versioned projected-policy API
with these fail-closed rules:

- every V1 call initializes and verifies exact `struct_size/version`, including
  empty polls;
- projected tickets must stay in the independent JavaScript-safe
  `[2^52, 2^53 - 1]` namespace;
- a frame cannot issue both an order ticket and a projected ticket;
- forced Candidate/Compact must report the same execution, disabled Adaptive
  state, and no ticket or unsampled reason;
- Adaptive issued, unsampled, and terminal states are mutually exclusive;
- every issued ticket reaches exactly one success or failure terminal with the
  same ticket, camera revision, execution, order lane, projection generation,
  and probe generation;
- a success immediately consumes its same-ticket V/C/D receipt before another
  poll or render can interleave;
- Candidate requires `D == V`; Compact requires exact compaction and `D == C`;
- formal evidence rejects ring busy, Surface unavailable, dropped receipts,
  failures, missing counts, duplicate terminals, and identity changes.
- `renderer.projected_evidence_version = 1` is mandatory for projected
  evidence; missing versions are not silently treated as legacy;
- every measured frame ticket is joined to the terminal's camera revision,
  execution, order lane, V/C/D tuple, and exact-compaction bit;
- per-frame CPU/GPU completion timing is emitted only for the frame that
  submitted that ticket. Cached redraws declare the timing unavailable instead
  of copying an earlier warmup receipt into every measured frame.

The iOS artifact adds an independent projected submission/success/failure
ledger and per-frame requested policy, actual execution, Adaptive state,
submission state, ticket, execution, unsampled reason, and flags. Extraction
runs both the shared benchmark validator and the Apple projected validator
before atomically publishing an artifact.

## Retained Simulator evidence

### Full Truck, forced Candidate

Artifact:

```text
target/full-quality-final/apple-projected-candidate-truck-sim-2622x1206-20260723-v2/artifact/
```

The artifact passes both validators and records:

```text
source = decoded = encoded = resident = addressable = 2,541,226
source/resident SH degree = 3 / 3
sampling / LOD = disabled / disabled
requested = Surface = internal = presented = 2622x1206
dynamic resolution / upscaling = disabled / disabled
visible V = 1,886,298
contributor C = 1,077,644
drawn D = 1,886,298 (Candidate D == V)
order lane = CPU
projected policy/execution/state = candidate / candidate / disabled
projected issued/success/failure/unsampled = 0 / 0 / 0 / 0
```

The initial fixed-camera CPU order completion was 535.909 ms. It belongs to the
warmup order ticket, so all four cached measured frames now correctly record
`cpu_frame_complete_ms = null`, the summary distribution is null, and both are
listed in `unavailable_fields`. The four cached measured redraw calls averaged
3.351 ms. That redraw number is not moving-camera throughput: the camera and
exact projection/order cache did not change, and the run used a Simulator debug
app. It is retained only as integration behavior.

### Minimal policy checks

Artifacts:

```text
target/full-quality-final/apple-projected-minimal-sim-2622x1206-20260723/artifact/
target/full-quality-final/apple-projected-candidate-minimal-sim-2622x1206-20260723/artifact/
```

The Adaptive artifact reports `candidate_only`, 20 Candidate frames, no
projected ticket, no unsampled state, and no failure. The forced Candidate
artifact similarly reports disabled Adaptive state and no fabricated ticket.
Both pass the shared and Apple projected validators.

The expected forced Compact capability failure is retained at:

```text
target/full-quality-final/apple-projected-compact-minimal-sim-2622x1206-20260723/console.log
```

It has no artifact because configuration never published a renderer.

## Verification completed

The following completed successfully on the current source tree:

```text
bash bindings/apple/scripts/test-ios-benchmark-artifact-extraction.sh
bash bindings/apple/scripts/run-swift-smoke.sh
bash bindings/apple/scripts/build-xcframework.sh
xcodebuild -scheme GsplatKit -destination 'generic/platform=iOS Simulator' build
bash bindings/apple/scripts/build-ios-sim-app.sh
bash bindings/apple/scripts/run-ios-sim-smoke.sh
python3 tests/perf/validate-benchmark-artifacts.py <each retained artifact>
python3 bindings/apple/scripts/validate-ios-projected-artifacts.py <each retained artifact>
```

`run-ios-sim-smoke.sh` rendered Flowers with `drawn=339846`,
`visible=339846`, and reported 102.429 ms for its smoke frame. This remains a
Simulator smoke result, not a product FPS claim.

## Remaining qualification

A physical iPhone is still required for Apple CPU/GPU order comparison,
Candidate/Compact projected comparison, moving-camera Truck throughput,
thermal behavior, and image capture. Until a capable device produces a
complete independent terminal ledger, the Simulator's `candidate_only` result
must remain an integration/capability result rather than an Adaptive policy
performance conclusion.
