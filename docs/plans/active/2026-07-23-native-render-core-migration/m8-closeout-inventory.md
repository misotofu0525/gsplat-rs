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
| M8-D08 | Audit [`RELEASING.md`](../../../../RELEASING.md) against current scripts and artifact names. M8e found only a stale copy-paste `0.1.2` literal; M8f makes the intended version a caller-supplied `VERSION` while retaining clean-main, archive, full-matrix, checksum and manual GitHub-setting gates. The workflow is unchanged. The independent review accepted candidate `6bbd3c1ca82a21c04613b17e722d001c23561f8a`, and root integrated it as `93f9dc662a6f1bac008def8ba4b541fb9a66ab4e`. | M8 release-process reviewer | `tests/release/check-version.sh`, release workflow, verification handbook, current v0.1 release boundary | Parameterized `0.1.3` version check, shell syntax/read-only command audit, workflow/artifact-name search and link check all passed in the M8f candidate and review | **Accepted** | GitHub settings, a future tag/upload and future published-asset verification are **Deferred** until the next release operation. |
| M8-D09 | Audit component and consumer docs individually: `crates/gsplat-render-wgpu/README.md`, `crates/gsplat-ffi-c/README.md`, `crates/gsplat-web/README.md`, `examples/desktop/README.md`, `examples/web/README.md`, `packages/web/README.md`, `bindings/android/README.md`, `examples/android/README.md`, `bindings/apple/README.md`, `bindings/apple/GsplatKit/README.md`, and `examples/ios/README.md`. M8e accepted the unaffected components and found three bounded drifts: Apple GitHub ZIP versus remote SwiftPM wording, the locked `wasm-bindgen-cli` diagnostic, and stale M2b ownership text for the still-rejected Android GPU-producer option. Independently reviewed M8f candidate `6bbd3c1ca82a21c04613b17e722d001c23561f8a`, integrated as `93f9dc662a6f1bac008def8ba4b541fb9a66ab4e`, repairs only those facts. | M8 consumer-doc reviewers | Matching source entrypoints and tests plus M7 public call-site/API/ABI ledger | Per-family documented-command and link audit, exact Web tool-version search, Android collector 56/56 and verification-bootstrap 11/11 passed | **Accepted** | Existing M7/M6 platform evidence only. Fresh Chrome/iPhone/Windows/Linux proof remains Deferred. |
| M8-D10 | Correct current program-status prose in [`m7-public-callsite-api-abi-ledger.md`](m7-public-callsite-api-abi-ledger.md) before archival. Preserve slice-local “M7 remained Active” statements in implementation notes as historical context rather than rewriting history. | M8 migration-ledger writer | M7 final acceptance and integrated chain above | Search the bundle for current-tense `M7 Active`, `M8 unstarted` and `acceptance Deferred`; classify every hit as historical or stale | **Accepted** | None. |
| M8-D11 | Normalize the five hard-coded active-bundle paths before the directory move: `m7-r3-renderer-offscreen-host.md`, `m7-r4-renderer-facade.md`, `m7-standalone-packed-entry-closeout.md`, `m7-legacy-packed-presenter-graph-closeout.md`, and `m7-public-callsite-api-abi-ledger.md`. Commands intended to be rerunnable after archival must point to `completed`; immutable historical command transcripts must be labelled historical. M8g confirmed all five files still contain active-path references at reconciliation base `fdd37a9dec9728e4ac3672d2ca2bb55745b1d2d2`. | M8 archive/link writer | Repository-wide search for `docs/plans/active/2026-07-23-native-render-core-migration` | Exact-path search before the fixed candidate; full Markdown relative-link and command path existence checks after the move | **Pending; five files retain active-path references** | None. |
| M8-D12 | Produce one populated [final closeout report](m8-final-closeout.md) containing the final architecture/deprecation ledger, exact M0--M8 SHA ledger, verification results, accepted limitations and non-claims. The report candidate is populated from integrated base `7618893`; root must independently review and integrate it before D15. | M8 final-ledger writer; root accepts | M0 cutover contract, `progress.md`, milestone closeouts, Git object identities | Independent fixed-SHA review; SHA ancestry/tree checks; links; final diff hygiene | **Candidate ready; root acceptance pending** | Deferred endpoints stay explicit rather than blocking truthful closeout. |
| M8-D13 | After the fixed pre-archive candidate passes D15 and independent review, move this entire bundle atomically from `docs/plans/active/` to `docs/plans/completed/` in one pure root archive/state commit and set `M8 = Accepted`. The architecture policy already names both locations; after the move, its ledger selection must resolve only the completed path. Edit the policy only if a separate audit demonstrates another required change. | Root M8 integration owner | Cutover M8 archive requirement, `RELEASING.md`, `tests/architecture/source_architecture_policy.json` | After the move run the full relative-link checker, architecture self-tests and real-tree policy, `git diff --summary`, stale active-path search and clean-tree check; do not rerun the expensive D15 matrix merely because paths moved | **Pending; only after fixed-candidate D15 and review** | None. |
| M8-D14 | Resolve the two external ownership review records `IO-PLY-1` and `IO-SPZ-1`. IO-SPZ was accepted unchanged at fixed baseline `41e18f8` as one cohesive `SPZ v4 -> validated SceneBuffers` transaction. The initial IO-PLY review task `019f9e81-6a96-7e73-9c73-9682fb0c0b0a` rejected the former mixed owner; the accepted exit then landed as A1 metadata (`b99adc5`), A2 terminal-safe incremental failure (`fdd37a9`), A3 per-vertex decode (`0c9ba23`) and A4 stream/facade (`cee2a97`). Independent A4 review task `019f9ed8-25e0-7393-b961-a9af77aef2a2` accepted the final boundary with P0/P1/P2 all zero and explicitly authorized closing the grandfather without another split. The production policy now has no grandfather or external-owner allowlist entry. | Separate visible IO ownership implementers and reviewers; root integrates the decisions | `metadata.rs` owns header/attributes/SH layout; `decode.rs` owns per-vertex numeric interpretation and coordinate normalization; `stream.rs` owns file/bytes/reader traversal and the incremental lifecycle; `lib.rs` owns public facade/errors, resource budgets, allocation/publication and rotation import-policy selection | Architecture self-tests and real-tree policy with zero grandfather entries; `cargo test -p gsplat-io-ply --locked` (46/46); format and diff checks | **Accepted** | The pre-existing file-backed Packed summary/stream two-open snapshot boundary remains **Deferred** as a separate behavior-hardening concern, not an ownership blocker. SPZ external-format interoperability and a product-selected non-default resource budget also remain **Deferred**. |
| M8-D15 | After all source, documentation, policy, active-path and populated-report edits are committed, freeze one pre-archive candidate SHA. Run and record the cutover clean-tree local matrix on that exact SHA, then obtain an independent finite review before D13. Use repository commands, not an ad-hoc substitute. | Root verification owner | [cutover verification catalog](cutover.md#verification-command-catalog) and verification handbook | Formatting, workspace check/tests, strict Clippy, Rustdoc, architecture self/real-tree checks, dependency policy, C/consumer checks affected by edits and committed-range diff checks; join every result to the fixed candidate SHA | **Pending; current-SHA matrix not yet run** | Device/browser runs only where a new factual claim requires them. Historical endpoint artifacts retain their own SHAs; they are not current-candidate qualification. |
| M8-D16 | Keep release distribution facts narrow: AAR/XCFramework/npm-compatible files are direct prerelease artifacts, not Maven, remote binary SwiftPM, npm or crates.io publication. The historical read-only M8e/M8f audits on 2026-07-26 recorded `v0.1.3 -> a47542fbcae092e07eb427f64e0a81ac2123b4c7` and the release assets `gsplat-android-release.aar`, `GsplatFFI.xcframework.zip`, `gsplat-rs-web-*.tgz` and `SHA256SUMS`; the wildcard is the repository release-workflow name, not a claim about a newly queried remote filename. M8f reconciled the docs and was independently accepted as `6bbd3c1ca82a21c04613b17e722d001c23561f8a`, then integrated as `93f9dc662a6f1bac008def8ba4b541fb9a66ab4e`. This M8h reconciliation did not re-query the network or perform a release operation. | Release-boundary reviewer | `RELEASING.md`, ROADMAP, root README, release workflow, M8e read-only distribution audit and M8f independent review | Cross-document terminology search and local release artifact-name audit | **Accepted for documented distribution facts** | Fresh remote verification, future tag/upload operations, remote settings and all package-registry publication remain **Deferred**. |

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
   reviews, repair D11 paths, and prepare the populated final
   closeout/architecture-deprecation report.
4. **M8d fixed-candidate verification:** after every source, documentation,
   policy, D11 and final-report edit is committed, freeze one pre-archive SHA;
   run the applicable clean-tree global matrix and independently review that
   exact candidate with finite P0/P1/P2 and Accepted/Deferred results.
5. **Root archive/state closeout:** only after the fixed candidate is accepted,
   move the bundle to completed and set `M8 = Accepted` in one pure
   archive/state commit. Confirm that policy selects only the completed ledger,
   then run link, architecture-policy, stale-path, diff-summary and clean-tree
   checks. These lightweight checks qualify the move itself; they do not create
   a circular requirement to rerun the expensive matrix after a path-only
   commit. Report this archive/state SHA as Package M's accepted tip.

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
