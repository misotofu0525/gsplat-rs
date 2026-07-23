# A0 Integration Baseline and Evidence Inventory

> Status: Accepted
> Frozen: 2026-07-23 18:43 CST
> Evidence boundary amended: 2026-07-23 18:57 CST
> Integration branch: `codex/native-render-core-refactor`
> Source implementation closeout: `5db2520e0d7a0ef1c68a78bdb9abc6fc588c5186`
> Plan anchor and A0 parent: `c478252246733f6dc209686091caf183e1ef7f06`

## 1. Scope and claim

A0 freezes the Git and evidence baseline for Package A. It changes no
production code, historical evidence, benchmark schema, handbook contract or
CI configuration. No device or performance run was repeated.

The accepted integration route is a dedicated branch from the complete local
full-quality tip. Package A does **not** merge that branch into `main`, and it
does **not** rebase or cherry-pick the six full-quality commits. A1 starts from
the A0 result at the tip of `codex/native-render-core-refactor`.

## 2. Git topology frozen by A0

The following identities were observed from this worktree and checked against
the live remote with `git ls-remote --heads origin main
codex/full-quality-native-rendering`:

| Ref or object | Commit | Observation |
| --- | --- | --- |
| local `main` | `28f77d041d70fe3a11713590591ac57a122e1599` | local release/integration trunk ref |
| cached `origin/main` | `28f77d041d70fe3a11713590591ac57a122e1599` | identical to local `main` |
| live `origin` `main` | `28f77d041d70fe3a11713590591ac57a122e1599` | identical at A0 observation time |
| local `codex/full-quality-native-rendering` | `c478252246733f6dc209686091caf183e1ef7f06` | full-quality result plus the native-core plan bundle |
| live `origin` full-quality ref | absent | no remote branch protects this local line |
| A0 starting `HEAD` | `c478252246733f6dc209686091caf183e1ef7f06` | clean detached worktree before the integration branch was created |

`main` is the merge base and a strict ancestor of the full-quality tip.
`main...codex/full-quality-native-rendering` is `0 / 7`; there are no commits
on the `main` side and seven on the full-quality side. The first six form the
completed full-quality line, and the seventh adds only the four active-plan
documents:

```text
28f77d041d70fe3a11713590591ac57a122e1599 main, origin/main
  1a8c32784ce36cfeda9430e31faf57b16087621c exact full-quality resident pipeline
  006e37e2479bd32b86194077d580d80aa0380c98 exact GPU producer diagnostics
  7cabb6e0d7e81b5988406f49f25ab82b50c5ce75 producer evidence camera-revision fix
  28f79eecf3aeab8eacfb7555beed38760d3f0753 Adaptive probe-starvation fix
  76a9267b26d1c55bef5208152d02f8ce56d4d88f tiled-raster timestamp test fix
  5db2520e0d7a0ef1c68a78bdb9abc6fc588c5186 full-quality closeout documents
  c478252246733f6dc209686091caf183e1ef7f06 native-core refactor plan bundle
```

The primary full-quality worktree at
`/Users/misotofu/Documents/workspace/gsplat-rs` was clean and owned
`codex/full-quality-native-rendering`. The three Codex worktrees at `01b9`,
`14d2` and `2320` were also clean detached checkouts of `c478252` before A0;
this `14d2` worktree now owns `codex/native-render-core-refactor`.

## 3. Integration decision

### Accepted: branch from the full-quality tip

`codex/native-render-core-refactor` was created directly from `c478252`. This
preserves all six historical full-quality commit identities, includes the
active refactor contract, leaves `main` untouched and gives A1 and later
Package A tasks one named line.

The production tree at the branch point is the full-quality closeout tree at
`5db2520`; `c478252` changes only the active plan bundle. After this A0
documentation commit, the integration branch tip is the only valid A1 base.

### Rejected: rebase or cherry-pick the full-quality commits

There is no divergence to resolve. Rebase or cherry-pick would only replace
commit identities already named by reports and retained artifact metadata,
making evidence provenance harder to audit without changing the production
tree.

### Not performed: merge directly into `main`

A fast-forward is topologically possible, but A0 has no authority to change
`main`, and direct integration would mix the experimental refactor program
with the release trunk before Package A has its ratchet and closeout. The
dedicated branch provides the safer reversible boundary. A future merge is a
separate reviewed action after the package gates pass.

## 4. Evidence classes and inherited inventory

### 4.1 A0 evidence classes

A0 assigns every inherited result one of three roles. Clean/dirty identity is a
separate dimension: a clean artifact may still be only directional, while a
dirty artifact cannot support final-binary qualification.

| Evidence class | Definition | What Package A may inherit |
| --- | --- | --- |
| verified semantic/correctness fact | A deterministic type/layout, count, order, image, failure, state-machine or validator fact tied to named code/evidence. It says what the implementation does, not how fast a final product is. | A1 and later refactors may use the fact as a regression oracle, while preserving its endpoint and commit boundary. |
| directional performance | Timing or winner/loser observation scoped to one endpoint, binary/commit, dirty flag, camera and protocol. It guides the next hypothesis but is not a default, release or cross-platform qualification. | Package A may cite it only as motivation; it may not aggregate or relabel it as final performance evidence. |
| capacity-only | A short complete-scene admission/load/draw result with exact counts and quality fallback disabled. | It proves only that the recorded scene fit and rendered on that endpoint; it proves no interactive FPS, sustained stability or thermal behavior. |

Final qualification is intentionally outside these A0 inheritance classes. It
requires a clean final binary, one predeclared protocol and non-mixed commit
identity, and belongs to Package Q.

### 4.2 Accepted inherited facts and observations

These results are inputs, not experiments to repeat during responsibility-only
extraction:

| Class | Accepted evidence | Inherited scope and boundary | Primary record |
| --- | --- | --- | --- |
| verified semantic/correctness fact | Exact Resident/Packed ownership and Direct image oracle | Complete source membership and source SH0--SH3, fail-closed admission and the fixed Direct gate are established. | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md), [findings](../../completed/2026-07-22-full-quality-native-rendering/findings.md) |
| verified semantic/correctness fact | CPU/GPU depth, stable order and exact count vocabulary | Shared explicit f32 depth sequence, full32 ordering, deterministic source-ID ties and `S/V/C/D` semantics are established. | [depth parity](../../completed/2026-07-22-full-quality-native-rendering/cpu-gpu-depth-parity.md), [exact contributor evidence](../../completed/2026-07-22-full-quality-native-rendering/exact-contributor-evidence.md) |
| verified semantic/correctness fact | CPU and portable GPU primitives | Stable CPU radix, retained NEON/AVX2 helpers, bounded Rayon, portable full32 GPU visibility/radix and indirect draw exist. E4/E5 must still qualify complete preprocess kernels. | [parallel radix](../../completed/2026-07-22-full-quality-native-rendering/cpu-parallel-radix.md), [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md) |
| verified semantic/correctness fact | Metal-only radix8 admission | Four-pass full32 Resident radix8 is accepted only on the recorded qualified Metal scope; Android keeps exact radix16. | [radix8 evidence](../../completed/2026-07-22-full-quality-native-rendering/resident-radix8.md) |
| verified semantic/correctness fact | Canonical exact raster and Preproject exactness | Four-vertex `TriangleStrip`, conservative support, fail-closed ProjectedQuads guards, GlobalQuads oracle and exact Preproject/Compact count/image behavior are retained. | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md), [producer checkpoint](../../completed/2026-07-22-full-quality-native-rendering/phase2-production-producer-ab-checkpoint.md) |
| verified semantic/correctness fact | Measured-policy mechanics | `FrameCompletion`, ticket identity, hysteresis, cooldown, re-probe and the moving-camera starvation fix are established behavior. | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md) |
| directional performance | CPU/GPU and PostSort/Preproject timings | All winner/loser and ratio observations remain scoped to their exact commit, dirty flag, endpoint and protocol. They are not one combined final-binary cohort. | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md) |
| capacity-only | Complete Garden/Bicycle endpoint runs | Short full-count SH3 runs prove admission and drawing only; 640x360 codec views remain diagnostics rather than product qualification. | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md) |

### 4.3 Ignored artifact identity ledger

The `target/` tree is ignored by Git. None of the paths below exists in this
A0 worktree, and `git ls-files` contains none of these assets. A0 inspected an
available machine-local copy only to transcribe identity fields. The table uses
workspace-relative historical locators deliberately: a machine absolute path
is not a portable evidence contract, and another checkout must not assume the
ignored files are present.

| Workspace-relative locator | Recorded identity | Class and allowed use |
| --- | --- | --- |
| `target/full-quality-final/post-parallel-android-truck-2412x1080-cpu-gpu-adaptive-r3-20260723/` | commit `28f77d041d70fe3a11713590591ac57a122e1599`; `dirty=true`; nine complete A065 CPU/GPU/Adaptive runs | **directional performance only**. It supports the historical A065 ordering direction, not clean final-binary qualification. |
| `target/full-quality-final-v3/playcanvas-android-a065-truck-native-dpr-20260723-a/` | commit `28f77d041d70fe3a11713590591ac57a122e1599`; `dirty=true`; PlayCanvas WebGPU A065 terminal observation | **directional performance only** under a different precision/work contract. It is not a clean comparator qualification. |
| `target/full-quality-final-v4/android-a065-truck-producer-ab-7cabb6e/` | commit `7cabb6e0d7e81b5988406f49f25ab82b50c5ce75`; `dirty=false`; PostSort/Preproject manifests have `pairing=null`/no pairing metadata | **clean descriptive directional performance**. Exact count/image receipts may support semantic correctness, but the timing ratio is not a formal paired qualification. |
| `target/full-quality-final-v4/android-a065-truck-adaptive-moving-fixed-120x2-28f79ee/` | commit `28f79eecf3aeab8eacfb7555beed38760d3f0753`; `dirty=false`; two complete Adaptive moving runs | The bounded learning/terminal-ticket outcome is a **verified semantic/correctness fact**; its timings remain directional. |
| `target/full-quality-final-v4/android-a065-garden-capacity-76a9267/` | commit `76a9267b26d1c55bef5208152d02f8ce56d4d88f`; `dirty=false`; complete Garden SH3 | **capacity-only**. It proves exact admission/rendering on that A065, not cadence or sustained behavior. |
| `target/full-quality-final-v4/android-a065-bicycle-capacity-76a9267/` | commit `76a9267b26d1c55bef5208152d02f8ce56d4d88f`; `dirty=false`; complete Bicycle SH3 | **capacity-only**. It proves exact admission/rendering on that A065, not cadence or sustained behavior. |

## 5. Rejected evidence inherited by Package A

The following hypotheses remain closed unless a later task names a materially
different implementation and why the earlier evidence no longer answers it:

| Rejected hypothesis | A0 inheritance |
| --- | --- |
| Sparse or fixed-slot Paged as full quality | It published an incomplete source subset, is not streaming and cannot be the Scalable foundation. |
| Random source-point sampling as LOD | It does not provide coverage-preserving hierarchy and violates Exact/Balanced membership. |
| Radix8 on Adreno | It produced corrupt source IDs and an almost-black Truck image; Android retains exact radix16. |
| Portable radix8 local-rank variant | It was correct on Metal but slower in every controlled comparison and was removed. |
| Additional early-fragment support guards | Both variants regressed the full plan and were fully removed. |
| Rejected logarithmic-alpha CPU candidate | Its logs are excluded and its code was reverted; it is not current product evidence. |
| 640x360 or 640x480 as product qualification | Those sizes are diagnostic/historical only. |
| iOS simulator timing as device performance | Simulator results prove integration/correctness only. |
| Fixed point-count CPU/GPU selection | A065 and Web Bicycle demonstrate endpoint/workload-dependent winners. |
| Strict same-quality or universal native-vs-Web claim from PlayCanvas | The pinned comparator uses a different precision, SH-update and count-observability contract. |
| Failed or partial artifact as evidence | Missing terminal, hash/schema mismatch or failed collection yields no evidence, not a slow/fast sample. |

Two items are **deferred**, not rejected: promoting Preproject as a universal
product default waits for one whole-plan controller and fallback policy; a
physical-iPhone performance claim waits for physical-device/signing evidence.

## 6. Canonical evidence authorities

The baseline commit freezes content identity. A0 also verified that the
following current files exist and remain the canonical routing set:

| Authority | SHA-256 at `c478252` |
| --- | --- |
| `handbook/VERIFICATION.md` | `fa18389437ddee92344c554c11181b84a82c72e894f1517167e88c16d14e9157` |
| `tests/perf/benchmark-artifact-v1.md` | `12cca1e8583f4501a7700175bf603336687a73a7cbae0dbb8fdc71e4f286112c` |
| `tests/perf/full-quality-experiment-v1.md` | `c11aacdc602d3a481540dd99a7be6fc0d2ec94956b21a11b2b23da983210cb44` |
| `tests/perf/full-quality-matrix-plan-v1.json` | `827b18c2e9ad988f3a304f114a5b9a0fbf25d44d7b20687ec44257fb79a0cd67` |
| `tests/perf/test_full_quality_experiment.py` | `9002dba59bf5711ae29836414f5c8c6a60fde1ec5474a96f07dc5fe28e071b83` |
| `tests/perf/validate-full-quality-experiment.py` | `86f75f25902143ec3f9f1646724ffb1610c1b3ac7672d5d717b8842caa205b21` |
| `tests/perf/validate-benchmark-artifacts.py` | `f781bd3798adb07a664355993f8409bb764d26d2f5ec1296e44239cde0b2b148` |
| `tests/perf/test-benchmark-artifacts.sh` | `a12e35af2d489008af75cd84a058c904b3a6baee639d540583b6062eb2371a95` |
| `tests/perf/validate-dataset-manifests.py` | `f0e3123e9ce594845288906fb9dc048f4393b1dccdd3b91b8af8c1e167793c2d` |
| `tests/perf/trace/test-trace-v1.sh` | `28955f06dd1ca7dd1c16d2530eaf18e4829287a9959b47b283afa2f3b4ac6cc1` |
| `tests/competitive/playcanvas/README.md` | `d11f8c4988fd299586bf830fbff46399f0f7c2aba50547424b4850a6ae79a033` |

A later schema task may change these authorities only before collecting a new
field or making a new claim. Package A extraction does not edit them.

The current matrix is a **schema-valid plan, not a completed qualification
matrix**. The canonical validator reports `expected=339`, `rendered=0`,
`capacity_rejected=0`, and `missing=339` when run with `--allow-incomplete`.
It therefore proves that the planned cells and schema are valid; it does not
prove a unified clean final binary across those cells and cannot be cited as
final product qualification.

## 7. Risks and controls

- Both full-quality and the new integration branch are local-only. A local
  object loss would remove the named refs; A0 records exact object IDs, but no
  push or remote backup is authorized in this task.
- `main` may advance after A0. Package A must not silently rebase onto a newer
  `main`; any integration refresh requires a new topology check and explicit
  evidence-impact decision.
- Historical artifacts name commits `7cabb6e`, `28f79ee` and `76a9267`.
  Rewriting the inherited line would weaken that provenance.
- The dirty `28f77d0` A065 native and PlayCanvas artifacts remain directional.
  Their complete receipts do not make them clean final-binary cohorts.
- Ignored `target/` locators are not repository assets. Their absence in a new
  worktree is expected; only committed schemas, reports and validators are
  portable until artifacts are deliberately archived elsewhere.
- Several worktrees began at the same plan commit. A1 must confirm its own
  branch and clean status before edits rather than infer ownership from path.
- Full-quality performance numbers remain endpoint-, binary-, camera- and
  interval-scoped. Extraction tasks inherit correctness contracts, not a fixed
  FPS promise.

## 8. A1 handoff

A1 receives these exact inputs:

1. Start from the clean tip of `codex/native-render-core-refactor` after the A0
   documentation commit; do not start from `main`, `5db2520` alone or another
   detached `c478252` worktree.
2. Treat the six-commit full-quality line as immutable evidence ancestry. A1
   may inherit verified semantic/correctness facts as regression oracles, but
   it must not splice milliseconds, FPS or ratios from dirty `28f77d0`, clean
   `7cabb6e`, `28f79ee`, `76a9267` or any other distinct binary into one
   performance claim.
3. Treat directional timings only as hypothesis context and Garden/Bicycle as
   capacity-only. Package Q owns clean, same-protocol, same-final-binary
   qualification and any cross-endpoint or competitor claim.
4. Add only the source-size/dependency ratchet and its explicit baseline
   allowlist; do not move production responsibilities in A1.
5. Measure physical LOC fresh at the A0 base for every grandfathered giant
   file, assign its exit task and enforce shrink-only behavior.
6. Encode the forbidden dependency directions from `architecture.md`, while
   avoiding a runtime pass trait/DAG or new crate.
7. Use the canonical validators above. The 0/339 matrix is a planning/schema
   oracle, not inherited execution evidence. No device-performance rerun is needed
   for the checker itself; lightweight repository checks and checker fixtures
   are the relevant A1 gates.
8. Preserve accepted/rejected/deferred boundaries above and stop if the ratchet
   would require a benchmark-schema, handbook, CI or production-policy change.

## 9. A0 verification record

- inspected local refs, live remote heads, merge bases, ancestry, left/right
  counts, linear parents and worktree occupancy;
- confirmed all four observed worktrees were clean before branching;
- verified the seven named commits resolve as Git commit objects;
- verified canonical authorities and linked inherited evidence files exist;
- validated the active-plan local Markdown links after adding this record;
- parsed the full-quality matrix JSON and ran its validator with
  `--allow-incomplete` (`expected=339`, `rendered=0`, `capacity_rejected=0`,
  `missing=339`; schema-valid planning matrix only);
- confirmed `target/` is ignored, the listed full-quality artifact roots are
  absent from this worktree and no such paths are tracked;
- inspected only commit/dirty/pairing identity fields from an available local
  artifact copy and recorded only workspace-relative locators;
- ran `git diff --check` and confirmed the A0 diff is documentation-only;
- did not run Cargo, browser, simulator, device or performance workloads.
