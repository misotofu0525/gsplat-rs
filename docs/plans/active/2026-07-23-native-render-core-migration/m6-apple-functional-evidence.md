# M6 Apple functional evidence closeout

## Status and claim boundary

- M6 is Accepted for the defined Apple/GsplatKit/XCFramework functional consumer
  scope at source commit `8022841957a196824eb9669c477f11ce91d2aab1`.
- This record accepts iPhone 17 Pro **Simulator** functional and capacity
  evidence only. It does not claim physical-iPhone frame time, energy,
  thermals, CPU/GPU winner, Metal performance, or competitive parity.
- `xcrun xctrace list devices` on 2026-07-26 listed the Mac and simulators but
  no attached physical iPhone. Physical-iPhone CPU/GPU/Adaptive qualification
  is therefore explicitly Deferred, as permitted by the M6 cutover contract.
- All artifacts named below are ignored, machine-local `target/` evidence. Git
  contains this closeout record only; it contains no PLY, app, framework,
  native archive, screenshot, or log payload.

## Consumer and ABI verification

Root independently passed the repository Apple package route at the accepted
SHA:

- `bash bindings/apple/scripts/test-gsplatkit-package.sh` rebuilt the device
  and simulator XCFramework slices, verified the current-stats V1 symbols in
  both archives, linked the Swift package for iOS and Simulator, and passed 18
  XCTest cases. Those cases include the frozen C/Swift size, alignment, offset,
  version and Exact-plan translation contracts as well as the projected-policy
  receipt contract.
- `bash bindings/apple/scripts/test-ios-render-loop-lifecycle.sh` passed.
- `python3 bindings/apple/scripts/test_ios_sim_benchmark_collector.py` passed
  all 14 collector/terminal-integrity tests.

The final M6 repair chain is already integrated in this SHA: `20527e4` keeps
the configured projected policy distinct from the actual Exact execution, and
`8022841` seals the UIKit render-loop after a terminal and makes the collector
fail closed if its terminal is lost, changed, or duplicated.

## Retained Simulator runs

Both accepted arms use the full unmodified Kitsune PLY:

- dataset SHA-256
  `3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2`,
  65,892,441 bytes, 279,199 splats, SH3;
- trace raw SHA-256
  `8b74bf8c4123e1d64907ab45cddb721c3a871ec0a31173a7ff781562b9c9e775`,
  canonical SHA-256
  `18bacc60328fb113ae3f9cc928428d300d98617de22f4c7b178595a187b64f2b`,
  2622x1206 and frame sequence `[0, 1]`;
- Packed SortedAlpha, 20 warmups, 80 measured frames, interval 1, no async
  sort, frame latency 2, full source membership and source SH degree;
- requested, Surface, internal-render and presented dimensions all 2622x1206;
  sampling, LOD, dynamic resolution and upscaling all disabled.

| Requested order policy | Run ID | Result | Local evidence root |
| --- | --- | --- | --- |
| CPU | `ios-48a94948-23b2-457d-9194-f5a36649e36d` | Accepted: 80 measured frames, actual `cpu_post_sort` | `/Users/misotofu/.codex/worktrees/6a2f/gsplat-rs/target/ios-sim-benchmarks/m6d-final-retry-8022841-cpu` |
| Adaptive | `ios-7077f56b-81e2-4550-b72e-79576aca6878` | Accepted: 80 measured frames, all actual `cpu_post_sort`, final `cpu_stable` | `/Users/misotofu/.codex/worktrees/6a2f/gsplat-rs/target/ios-sim-benchmarks/m6d-final-retry-8022841-adaptive` |

Every accepted arm records
`source = decoded = encoded = resident = addressable = 279199`, source and
resident SH3, exactly one `BENCHMARK_RESULT`, 80 current-stats submissions and
matching Ready terminals, 80 successful order terminals, and no terminal
failures. `S = V = D = 279199`; contributor counts remain measured rather than
invented. Both artifacts carry the clean source SHA, app executable, native
archive, Simulator XCFramework slice, simulator UUID, dataset, trace and raw
console-log identities in their collector receipts.

Root independently re-ran, for both artifact directories:

- `tests/perf/validate-benchmark-artifacts.py`;
- `bindings/apple/scripts/validate-ios-projected-artifacts.py`;
- `bindings/apple/scripts/validate-ios-current-stats-artifacts.py`.

The two-cell suite at
`/Users/misotofu/.codex/worktrees/6a2f/gsplat-rs/target/ios-sim-benchmarks/m6d-final-retry-8022841-suite.json`
also passed `validate-full-quality-experiment.py --verify-inputs` with
`expected=2 rendered=2 capacity_rejected=0 missing=0`.

## Forced-GPU and physical-device boundary

The required forced-GPU attempt was made only after the valid CPU artifact. It
did not publish an artifact or get substituted with another policy. The
simulator returned
`IOS_SURFACE_CONFIG_FAILED option=order_backend rc=4 error=unsupported` three
times and emitted no benchmark terminal. Its diagnostic is retained at

`/Users/misotofu/.codex/worktrees/6a2f/gsplat-rs/target/ios-sim-benchmark-failures/m6d-final-retry-8022841-gpu-8022841957a1-w15zv5c2/collector-failure.json`.

This is a Simulator capability Deferred, not a rejected quality result, a
capacity result, or a GPU performance observation. The absence of an attached,
signed physical iPhone is a separate M6 physical-device Deferred. Neither
deferred item blocks the Apple functional consumer migration.
