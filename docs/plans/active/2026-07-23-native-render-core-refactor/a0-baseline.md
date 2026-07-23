# A0 Integration Baseline and Evidence Inventory

> Status: Accepted
> Frozen: 2026-07-23 18:43 CST
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

## 4. Accepted evidence inherited by Package A

These results are inputs, not experiments to repeat during responsibility-only
extraction:

| Accepted evidence | Inherited scope and boundary | Primary record |
| --- | --- | --- |
| Exact Resident/Packed ownership | Complete source membership and source SH0--SH3; fail-closed admission; Direct remains the wide-f32 oracle | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md), [findings](../../completed/2026-07-22-full-quality-native-rendering/findings.md) |
| Direct image gate | Full-resolution Truck passes the fixed Direct gate; historical Garden/Bicycle codec views support the retained representation but 640x360 is diagnostic only | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md) |
| CPU/GPU depth and stable order | Shared explicit f32 depth sequence, full32 ordering and deterministic source-ID ties; complete Garden visibility parity | [depth parity](../../completed/2026-07-22-full-quality-native-rendering/cpu-gpu-depth-parity.md) |
| Exact count vocabulary | `S/V/C/D` and `candidate_visible_contributor_issued_v1`; Candidate requires `D=V`, exact Compact permits `D=C<=V` | [exact contributor evidence](../../completed/2026-07-22-full-quality-native-rendering/exact-contributor-evidence.md) |
| CPU ordering primitives | Stable four-pass CPU radix, retained NEON/AVX2 helpers and bounded Rayon path; E4/E5 still must qualify complete preprocess kernels rather than claim greenfield SIMD | [parallel radix](../../completed/2026-07-22-full-quality-native-rendering/cpu-parallel-radix.md) |
| Portable GPU order | Full32 visibility, hierarchical scan, stable radix and indirect draw are established; the base16 eight-pass path is the portability baseline | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md) |
| Metal radix8 | Four-pass full32 Resident radix8 is accepted only on the recorded qualified Metal scope | [radix8 evidence](../../completed/2026-07-22-full-quality-native-rendering/resident-radix8.md) |
| Canonical exact raster | Four-vertex `TriangleStrip`, opacity-aware conservative support, exact ProjectedQuads cache guards and GlobalQuads oracle are retained | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md) |
| Exact Preproject/Compact | Exact contributor-first producer is implemented, transactionally prepared and image exact; same-binary A/Bs are accepted for their tested endpoints | [producer checkpoint](../../completed/2026-07-22-full-quality-native-rendering/phase2-production-producer-ab-checkpoint.md) |
| Runtime CPU/GPU selection | `FrameCompletion`, ticket identity, hysteresis, cooldown and re-probe are accepted; the moving-camera starvation fix is part of the baseline | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md) |
| Endpoint evidence | M4 Metal, Chrome/WebGPU and A065 results are scoped to their recorded binaries/workloads; iOS simulator evidence is functional only; Garden/Bicycle short runs are capacity only | [final report](../../completed/2026-07-22-full-quality-native-rendering/final-report.md) |
| Artifact and camera identity | Formal resolutions, full count/SH receipts, camera hashes and terminal tickets remain authoritative | [full-quality schema](../../../../tests/perf/full-quality-experiment-v1.md), [matrix](../../../../tests/perf/full-quality-matrix-plan-v1.json) |

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

## 7. Risks and controls

- Both full-quality and the new integration branch are local-only. A local
  object loss would remove the named refs; A0 records exact object IDs, but no
  push or remote backup is authorized in this task.
- `main` may advance after A0. Package A must not silently rebase onto a newer
  `main`; any integration refresh requires a new topology check and explicit
  evidence-impact decision.
- Historical artifacts name commits `7cabb6e`, `28f79ee` and `76a9267`.
  Rewriting the inherited line would weaken that provenance.
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
2. Treat the six-commit full-quality line as immutable evidence ancestry.
3. Add only the source-size/dependency ratchet and its explicit baseline
   allowlist; do not move production responsibilities in A1.
4. Measure physical LOC fresh at the A0 base for every grandfathered giant
   file, assign its exit task and enforce shrink-only behavior.
5. Encode the forbidden dependency directions from `architecture.md`, while
   avoiding a runtime pass trait/DAG or new crate.
6. Use the canonical validators above. No device-performance rerun is needed
   for the checker itself; lightweight repository checks and checker fixtures
   are the relevant A1 gates.
7. Preserve accepted/rejected/deferred boundaries above and stop if the ratchet
   would require a benchmark-schema, handbook, CI or production-policy change.

## 9. A0 verification record

- inspected local refs, live remote heads, merge bases, ancestry, left/right
  counts, linear parents and worktree occupancy;
- confirmed all four observed worktrees were clean before branching;
- verified the seven named commits resolve as Git commit objects;
- verified canonical authorities and linked inherited evidence files exist;
- validated the active-plan local Markdown links after adding this record;
- parsed the full-quality matrix JSON and ran its validator with
  `--allow-incomplete` (`expected=339`, `missing=339`, valid planning matrix);
- ran `git diff --check` and confirmed the A0 diff is documentation-only;
- did not run Cargo, browser, simulator, device or performance workloads.
