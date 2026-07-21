# Task Plan: Packed Atlas Branch Closeout

## Goal

Close `refactor/packed-atlas-d-reset` without promoting its local paging
prototype into the stable v0.1 contract: keep the verified Packed and
competitive-measurement gains, fix concrete correctness/resource defects,
archive completed research, then fast-forward the exact verified commit to
`main`.

## Status

Implementation complete. Remote publication is deliberately tracked by the
Git commit and GitHub checks rather than a follow-up documentation commit.

## Boundaries

- Hard gates: build/test regressions, invalid resource accounting, incorrect
  first-frame color, unsafe input/state transitions, and unsupported claims.
- Observations only: fixed FPS ratios, winning every competitor comparison,
  full device/browser matrices, and long-run performance percentages.
- `SortedAlpha` Direct remains the release-gated default.
- Paged remains an explicit local-source diagnostic. This closeout does not add
  metadata-first streaming, remote I/O, LOD generation, or another public Auto
  selector.
- SPZ remains an isolated experimental loader; consumer integration is deferred.

## Phases

### 1. Audit and choose the boundary

- [x] Compare the branch with `origin/main` and inspect all active plans.
- [x] Review current competitors and primary sources.
- [x] Separate retained, frozen, and deferred work.

### 2. Correct and narrow

- [x] Remove the packed shader's unread SH GPU allocation and false preflight gate.
- [x] Make Packed Surface first-frame SH color complete.
- [x] Freeze one camera for each banded Packed color refresh.
- [x] Remove unused public Auto Surface constructors.
- [x] Keep paging implementation types crate-private rather than compatibility API.

### 3. Reconcile project records

- [x] Archive the three completed predecessor bundles.
- [x] Align roadmap, architecture, context, README, changelog, and verification.
- [x] Record honest historical evidence and current non-claims.

### 4. Verify and publish

- [x] Run the repository verification routes required by the touched scope.
- [x] Define exact-SHA publication: push one closeout commit, wait for all
  required checks, then fast-forward `main` to that same SHA.

The branch push, required-check result, and `main` fast-forward are operational
Git/GitHub state. They must not be backfilled into this bundle with a new,
unverified commit.

## Accepted / Rejected / Deferred

| Item | Decision | Reason |
|---|---|---|
| Packed 20 B hot record and resource preflight | Accepted | Direct memory/binding relief with existing image/count gates. |
| Artifact/trace/PlayCanvas harness | Accepted | Reproducible comparison is a prerequisite for competitive work. |
| Direct-first presenter/draw ownership cleanup | Accepted | Benefits the release-gated path without changing its default. |
| Fixed four-slot local Paged runtime | Frozen diagnostic | It retains full `SceneBuffers`, is synchronous, and is not streaming or a demonstrated speedup. |
| Public automatic Paged selection | Rejected for this closeout | No real SDK consumer exists and it widens product policy prematurely. |
| SPZ consumer integration | Deferred | The bounded v4 loader is useful, but FFI/Web/mobile wiring is a separate product decision. |
| GPU cull/compact/radix/indirect pipeline | Deferred next priority | More directly matches current Brush/PlayCanvas renderer architecture. |

## Research Sources

- [Brush](https://github.com/ArthurBrussee/brush): portable Rust/WebGPU
  rendering and training; useful GPU-sort and cross-platform baseline.
- [PlayCanvas engine](https://github.com/playcanvas/engine): current GPU
  cull/compact/radix/indirect render path.
- [PlayCanvas Streamed SOG](https://developer.playcanvas.com/user-manual/gaussian-splatting/formats/streamed-sog/): true spatial tree, per-leaf LOD, and chunked-source reference.
- [gsplat](https://github.com/nerfstudio-project/gsplat): quality and
  profile-evidence baseline.
- [Niantic SPZ](https://github.com/nianticlabs/spz): interoperable compressed
  source format baseline.
