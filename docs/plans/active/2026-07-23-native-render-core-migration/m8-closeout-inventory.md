# M8 documentation and release closeout inventory

## Scope and decision

- Slice: **M8a — bounded fact inventory and activation**.
- Fixed base: `1de3f79fa2fa22955f99c887bea421c918e31ee0`, the accepted
  M7 integrated tree.
- Decision: **Accepted** for activating M8 and freezing this reviewable
  closeout workplan. This document does not accept M8 or complete Package M.
- Mutable paths in M8a are only this file and [progress.md](progress.md).
- No handbook, README, `RELEASING.md`, source, ABI, shader, script, test,
  device, browser, merge, push or release operation belongs to M8a.
- A fixed line count is not a gate, target or split trigger. Later edits are
  selected by factual ownership, release alignment and verifiable scope.

`Accepted` below means the authoritative local fact and a repository-owned
verification path are available for a later M8 slice. It does not mean the
listed edit or final gate has already run. `Deferred` means the evidence or
decision is unavailable or belongs to a separately visible task; it must stay
explicit and cannot be replaced by compilation, Simulator, historical or
cross-platform inference.

## Accepted M7 integration chain

| Ledger field | Exact identity | Meaning |
| --- | --- | --- |
| `M7_BASE_SHA` | `a798b8accf454e96792df45d38e8564e7e27556e` | Accepted M6 tree and aggregate rollback target. |
| M7 source range | `a798b8a..2739f4899facc03e6a0fb35c23b42d9762ede9cc` | Linear 39-commit behavior, compatibility, ownership and evidence range. |
| M7 source/evidence tip | `2739f4899facc03e6a0fb35c23b42d9762ede9cc` | Fixed tree reviewed by the final ownership and available-platform acceptance audit. |
| M7 acceptance/registry closeout | `0d291c5ea9b911fdfce8d064ff6617ee6f9955ed` | Records [M7 final acceptance](m7-final-acceptance.md) and `M7 = Accepted`. |
| M7 ledger correction | `2e16fff072d6832d602d35c53e1c0582220ec9a2` | Corrects the accepted renderer-test totals without changing source or evidence scope. |
| M7 policy closeout | `1de3f79fa2fa22955f99c887bea421c918e31ee0` | Removes exactly the three satisfied M7 architecture grandfather records. |
| `M7_ACCEPT_SHA == M8_BASE_SHA` | `1de3f79fa2fa22955f99c887bea421c918e31ee0` | Final integrated M7 tree and sole base for M8 work. |

The source/evidence tip is not mislabeled as the integrated acceptance tip.
The complete chain is linear, and the final policy commit is part of M7
closeout because `M7 = Accepted` made those three records expire.

## Remaining documentation and release alignment

| ID | Remaining item and destination | Owner | Authoritative source | Later verification | Class | External platform evidence |
| --- | --- | --- | --- | --- | --- | --- |
| M8-D01 | Replace the stale “M7 Active / M8 unstarted” current-focus and open-gap statements in [`handbook/PROJECT_CONTEXT.md`](../../../../handbook/PROJECT_CONTEXT.md). Preserve the exact-count product route, small release surface and explicit endpoint limits. | M8 factual-doc writer | [M7 final acceptance](m7-final-acceptance.md), current renderer/Web/mobile component READMEs, and the fixed M8 base | Relative-link check; targeted stale-status search; `git diff --check`; architecture policy | **Accepted** | No new run. Chrome/WebGPU, physical iPhone and Windows/Linux remain Deferred. |
| M8-D02 | Update [`handbook/ARCHITECTURE.md`](../../../../handbook/ARCHITECTURE.md) from migration-era status to accepted M7 ownership. Correct the Web example flow so sampled WebGL2 is explicit opt-in, never an automatic product fallback. | M8 architecture-doc writer | `examples/web/src/renderer-policy.mjs`, `examples/web/src/main.js`, [`examples/web/README.md`](../../../../examples/web/README.md), and M7 ownership audit | `node --test examples/web/test/renderer-policy.test.mjs`; targeted symbol/path search; relative-link check; architecture policy | **Accepted** | No new runtime. Browser execution remains Deferred. |
| M8-D03 | Reconcile the Web smoke sections in [`handbook/VERIFICATION.md`](../../../../handbook/VERIFICATION.md): default Exact WASM/WebGPU failure is fail-closed, sampled WebGL2 requires its opt-in flag, and current Packed output uses `renderer=wasm_packed_atlas` rather than the retired Direct product label. Do not weaken the formal 1920x1080 contract. | M8 verification-doc writer | Web README/policy tests, `examples/web/src/main.js`, `packages/web` scripts, and the M4/M7 evidence boundaries | Validate every documented command with shell/tool admission or `--help`/syntax where non-mutating; `node --check`; Web unit tests; link check | **Accepted** | Real Chrome/WebGPU at `M7_ACCEPT_SHA == M8_BASE_SHA` (`1de3f79fa2fa22955f99c887bea421c918e31ee0`) stays Deferred and is not required merely to correct commands. |
| M8-D04 | Update [`handbook/ROADMAP.md`](../../../../handbook/ROADMAP.md) to state M7 Accepted and M8 Active, while leaving Balanced, Streamed, competitor qualification, SDK publication and broader APIs inactive. | M8 roadmap writer | Cutover M8 frozen boundary, M7 final acceptance, current release boundary | Stale-status search; link check; compare release-boundary terms with root README and `RELEASING.md` | **Accepted** | None. |
| M8-D05 | Review [`handbook/GOLDEN_PRINCIPLES.md`](../../../../handbook/GOLDEN_PRINCIPLES.md). Its exact-count, Direct-oracle, explicit-Paged, shared-scheduler and executable-verification rules are current; change it only if a later factual audit finds a contradiction. | M8 principles reviewer | Current M7 architecture and public compatibility ledger | Targeted terminology audit; link check; `git diff --check` | **Accepted; expected no change** | None. |
| M8-D06 | Reconcile the external-facing [`README.md`](../../../../README.md). At minimum, label the WebGL2 preview as explicit sampled diagnostic rather than an automatic fallback and describe Packed as the product Surface route without widening the stable v0.1 contract. | M8 public-doc writer | Current Web example policy, renderer README, C/Web/mobile package defaults, ROADMAP release boundary | README link check; documented quick-start command admission; targeted default/fallback search | **Accepted** | README must retain explicit device/browser limitations; no new endpoint run. |
| M8-D07 | Rewrite the stale [`CHANGELOG.md`](../../../../CHANGELOG.md) Unreleased entries that still call Direct the only production pipeline and omit the accepted Exact migration. Record the actual M1--M7 cutover/deletion and keep experimental API/distribution limits explicit. | M8 release-note writer | M1--M7 closeouts in this bundle, Git range `M8_BASE_SHA` ancestry, current ROADMAP | Diff review against `v0.1.3..M8_BASE_SHA`; link/format check; release-version check only when a release version is selected | **Accepted** | No publication or endpoint claim. |
| M8-D08 | Audit [`RELEASING.md`](../../../../RELEASING.md) against current scripts and artifact names. No M7 fact presently requires a process change; retain clean-main, version, archive, full-matrix, checksum and manual GitHub-setting gates unless a script audit proves drift. | M8 release-process reviewer | `tests/release/check-version.sh`, release workflow, verification handbook, current v0.1 release boundary | Shell syntax/read-only command audit; workflow/artifact-name search; link check | **Accepted; expected no change** | GitHub settings, tag, upload and published-asset verification are **Deferred** until an actual release. |
| M8-D09 | Audit component and consumer docs individually: `crates/gsplat-render-wgpu/README.md`, `crates/gsplat-ffi-c/README.md`, `crates/gsplat-web/README.md`, `examples/desktop/README.md`, `examples/web/README.md`, `packages/web/README.md`, `bindings/android/README.md`, `examples/android/README.md`, `bindings/apple/README.md`, `bindings/apple/GsplatKit/README.md`, and `examples/ios/README.md`. Current route/default/current-stats wording is the comparison oracle; edit only demonstrated contradictions. | M8 consumer-doc reviewers, one platform family per visible task if edits are needed | Matching source entrypoints and tests plus M7 public call-site/API/ABI ledger | Per-family documented-command and link audit; targeted API-symbol search; package tests/build checks from verification handbook when an edit changes a claim | **Accepted; audit pending** | Existing M7/M6 platform evidence only. Fresh Chrome/iPhone/Windows/Linux proof remains Deferred. |
| M8-D10 | Correct current program-status prose in [`m7-public-callsite-api-abi-ledger.md`](m7-public-callsite-api-abi-ledger.md) before archival. Preserve slice-local “M7 remained Active” statements in implementation notes as historical context rather than rewriting history. | M8 migration-ledger writer | M7 final acceptance and integrated chain above | Search the bundle for current-tense `M7 Active`, `M8 unstarted` and `acceptance Deferred`; classify every hit as historical or stale | **Accepted** | None. |
| M8-D11 | Normalize the five hard-coded active-bundle paths before the directory move: `m7-r3-renderer-offscreen-host.md`, `m7-r4-renderer-facade.md`, `m7-standalone-packed-entry-closeout.md`, `m7-legacy-packed-presenter-graph-closeout.md`, and `m7-public-callsite-api-abi-ledger.md`. Commands intended to be rerunnable after archival must point to `completed`; immutable historical command transcripts must be labelled historical. | M8 archive/link writer | Repository-wide search for `docs/plans/active/2026-07-23-native-render-core-migration` | Exact-path search; full Markdown relative-link check after the move; command path existence check | **Accepted** | None. |
| M8-D12 | Produce one populated final closeout report containing the final architecture/deprecation ledger, exact M0--M8 SHA ledger, verification results, accepted limitations and non-claims. Do not add an empty placeholder report. | M8 final-ledger writer; root accepts | M0 cutover contract, `progress.md`, all milestone closeouts, Git object identities | Independent fixed-SHA review; SHA ancestry/tree checks; links; final diff hygiene | **Accepted; final slice** | Deferred endpoints stay explicit rather than blocking truthful closeout. |
| M8-D13 | Move this entire bundle atomically from `docs/plans/active/` to `docs/plans/completed/` only in the M8 acceptance commit. The architecture policy already names both locations; after the move, its ledger selection must resolve only the completed path. Edit the policy only if a separate audit demonstrates another required change. | Root M8 integration owner | Cutover M8 archive requirement, `RELEASING.md`, `tests/architecture/source_architecture_policy.json` | Full link checker; architecture self-tests and real-tree policy; `git diff --summary`; clean-tree check | **Accepted; only after all other exits** | None. |
| M8-D14 | Resolve the two external ownership review records `IO-PLY-1` and `IO-SPZ-1`. The independent IO-SPZ review at fixed baseline `41e18f8` accepted the existing parser as one cohesive `SPZ v4 -> validated SceneBuffers` transaction with P0/P1/P2 all zero, so its grandfather and review allowlist entry are removed without a source split. `IO-PLY-1` remains active and must complete its separately reviewed ownership exit without renewing the M8 exception. M8 cannot become Accepted while that remaining `review_task: M8` is unresolved. | Separate visible `IO-PLY-1` and `IO-SPZ-1` review owners; root integrates the decisions | `tests/architecture/source_architecture_policy.json` `grandfather` entries, the IO-SPZ review task `019f9e81-6a96-7e73-9c73-96a16b46ce10`, and checker `grandfather.review_due` behavior | Architecture self-tests; real-tree policy with `M8 = Accepted` in a candidate ledger; focused IO tests if source changes are separately authorized | **Partially resolved: IO-SPZ Accepted; IO-PLY required before M8 acceptance** | No source or external-platform edit. SPZ external-format interoperability and a product-selected non-default resource budget remain **Deferred**. |
| M8-D15 | Run and record the final clean-tree local matrix required by the cutover contract after all documentation/policy/archive edits. Use repository commands, not an ad-hoc substitute. | Root verification owner | [cutover verification catalog](cutover.md#verification-command-catalog) and verification handbook | Formatting, workspace check/tests, strict Clippy, Rustdoc, architecture self/real-tree checks, dependency policy, C/consumer checks affected by edits, committed-range diff checks | **Accepted; execution pending** | Device/browser runs only where a new factual claim requires them. |
| M8-D16 | Keep release distribution facts narrow: AAR/XCFramework/npm-compatible files are direct prerelease artifacts, not Maven, binary SwiftPM, npm or crates.io publication. Do not tag, upload, push or mutate GitHub settings as part of migration closeout. | Release-boundary reviewer | `RELEASING.md`, ROADMAP, root README, release workflow | Cross-document terminology search; release artifact-name audit | **Accepted** | Actual release and remote settings are **Deferred**. |

## Deferred endpoint ledger

| Endpoint | M8 disposition | Evidence that remains usable | Forbidden substitution | Later owner and verification |
| --- | --- | --- | --- | --- |
| Chrome/WebGPU at `M7_ACCEPT_SHA == M8_BASE_SHA` (`1de3f79fa2fa22955f99c887bea421c918e31ee0`) | **Deferred** | M7 WASM compilation and historical M4 Chrome functional/quality evidence, labelled with their own SHAs | Compilation, WebGL2, a `2739f4899facc03e6a0fb35c23b42d9762ede9cc` Chrome run, or any other browser result at an earlier source/evidence tip cannot substitute for a `1de3f79fa2fa22955f99c887bea421c918e31ee0` browser result | Future Web qualification task at exact `1de3f79fa2fa22955f99c887bea421c918e31ee0`; exact locked `wasm-bindgen-cli 0.2.121`, real Chrome/WebGPU run and canonical artifact validators |
| Physical iPhone at accepted M7 SHA | **Deferred** | M6 wrapper/Simulator functional evidence and M7 macOS/Metal evidence | Simulator or host Metal cannot become physical-device proof | Future Apple qualification task; signed physical-device run with actual drawable and canonical validators |
| Windows runtime | **Deferred** | Source/API review only | macOS, WASM compile, cross-compilation or CI capacity cannot become Windows runtime proof | Future qualification task on real Windows/DX12 or other declared backend |
| Linux runtime | **Deferred** | Source/API review only | macOS, WASM compile, cross-compilation or Lavapipe capacity cannot become claimed product runtime proof without the declared run | Future qualification task on the declared Linux backend/environment |

These four cells are not silently converted to Accepted by M8 documentation
work. M8 may close with them Deferred only if every public document states the
same boundary and makes no broader platform, pixel, performance or release
claim.

## Slice order and acceptance gates

1. **M8a (this slice):** activate M8, freeze the exact M7 integration chain,
   enumerate every remaining alignment/owner/verification boundary, and stop.
2. **M8b factual docs:** update the five canonical handbook pages, root README
   and CHANGELOG; audit `RELEASING.md` and every component/consumer README.
3. **M8c ledger and ownership decisions:** correct the migration ledger's
   current status, resolve `IO-PLY-1`/`IO-SPZ-1` through separate visible
   reviews, and prepare the populated final closeout/architecture-deprecation
   report.
4. **M8d verification:** run link/command checks and the applicable clean-tree
   global matrix; independently review the fixed candidate with finite
   P0/P1/P2 and Accepted/Deferred results.
5. **Root closeout:** only after all gates pass, move the bundle to completed,
   set `M8 = Accepted`, confirm that the policy selects only the completed
   ledger, create the separate closeout commit, and report its SHA as Package
   M's accepted tip.

M8 is rejected if a later slice changes renderer behavior, ABI, shaders,
product defaults or evidence protocols; rewrites Deferred platform evidence as
Accepted; leaves current public facts contradictory; leaves links/commands
broken; or archives the bundle before the final fixed-SHA review. M8 is
deferred for a concrete missing fact or required independent decision, not for
an unproved performance preference.

## M8a verification record

The M8a handoff must record fresh results for:

```bash
git merge-base --is-ancestor \
  a798b8accf454e96792df45d38e8564e7e27556e \
  2739f4899facc03e6a0fb35c23b42d9762ede9cc
git rev-list --count \
  a798b8accf454e96792df45d38e8564e7e27556e..\
2739f4899facc03e6a0fb35c23b42d9762ede9cc
git log --reverse --format='%H %s' \
  2739f4899facc03e6a0fb35c23b42d9762ede9cc..\
1de3f79fa2fa22955f99c887bea421c918e31ee0
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s tests/architecture -p 'test_source_architecture.py'
PYTHONDONTWRITEBYTECODE=1 python3 \
  tests/architecture/check_source_architecture.py
git diff --check 1de3f79fa2fa22955f99c887bea421c918e31ee0..HEAD
git show --check HEAD
```

A read-only Markdown checker must additionally resolve every relative link in
the two M8a-owned files. The committed diff must contain exactly
`progress.md` and `m8-closeout-inventory.md`, and the final worktree must be
clean. These checks prove only the M8a registry/inventory slice; they do not
prove M8 closeout or any Deferred endpoint.
