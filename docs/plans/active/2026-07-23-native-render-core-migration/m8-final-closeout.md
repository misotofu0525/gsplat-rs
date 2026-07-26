# M8 final closeout report candidate

## Decision boundary

- The populated **M8-D12 report** was independently reviewed and integrated by
  root as `315f78e059ee0cd94b7653b4465e1937039d0131`; its follow-up status-order
  repair was integrated as `c6ea13e41bde1a9d0f8af7252d343538603bc059`.
- It consolidates already implemented architecture, accepted deprecations,
  Git identities, retained verification and explicit limitations. It adds no
  renderer behavior, API, ABI, shader, script, test, platform qualification or
  release operation.
- This report does **not** accept M8 or Package M. M8-D15 remains Pending. Root
  integration of this status-admission repair defines the clean pre-archive
  SHA to freeze. The current-SHA matrix must be rerun on that exact new tree
  and independently reviewed before the pure M8-D13 archive and state
  transition. Results recorded at `c6ea13e` do not validate the new tree.
- Historical endpoint artifacts retain their recorded source SHAs. They are
  not current-candidate qualification and are never substituted for a missing
  browser, device or operating-system run.

Authoritative inputs are the [cutover contract](cutover.md),
[program progress ledger](progress.md), [M8 inventory](m8-closeout-inventory.md),
[M7 acceptance audit](m7-final-acceptance.md),
[M5 Android closeout](m5-android-strict-evidence.md),
[M6 Apple closeout](m6-apple-functional-evidence.md), and Git object identities.

## Final architecture and deprecation ledger

| Area | Final owner and supported boundary | Removed or deprecated ownership | Retained limitation |
| --- | --- | --- | --- |
| Exact semantic runtime | `Renderer::PreparedRuntimeSlot` owns the exact Resident scene, closed `PlanSet`, generations, mandatory sampler, whole-plan controller and terminal frame result. | Duplicate product controllers, samplers, generation owners, plan-cache owners and terminal owners are removed. | Only `SortedAlpha` is release-gated. |
| Surface transaction | `SurfaceRenderSession` composes the public frame transaction; `evidence/session_publication.rs::SessionPublication` is the sole statistics/evidence publication ledger; `SurfacePresenterHost` and Surface lifecycle/configuration/capture leaves own mechanics. | The standalone Packed product graph and duplicate Session/Presenter semantic publication are removed. Queue submission alone cannot publish a frame. | Standalone Presenter retains Direct and explicit diagnostic Paged only. |
| Product geometry | Product consumers select exact-count Resident/Packed. Direct remains the wide-f32 oracle. Capacity failure rejects before publication. | Automatic Direct-to-Packed/Paged fallback, sampling, hidden draw budgets and silent SH reduction are not product choices. | Paged remains a fixed-slot, CPU-only local diagnostic backed by complete `SceneBuffers`; it is not streaming or arbitrary-scale evidence. |
| Raster | Product Packed uses canonical `ProjectedQuadsExact`; one projection per visible splat feeds a four-vertex instanced `TriangleStrip`. | TiledExact and its public variant are deleted. The obsolete standalone Packed raster graph is deleted. | GlobalQuads remains only on Direct/Paged compatibility paths and as an oracle boundary. |
| Ordering and plans | CPU PostSort, GPU PostSort and GPU Preproject are complete same-Exact plans. Adaptive selects measured whole plans and preserves stable full32 depth/source-ID ordering. | A model-point-count threshold and partial-plan selection are not policy. Forced GPU does not silently fall back. | Eligibility and defaults remain scope- and device-measured; performance is observational. |
| Current statistics and evidence | Renderer/Session own requests, submissions, terminal queues, generations and ticket-addressed counts. C, Web, Android and Apple layers translate immutable receipts. | FFI-side compatibility queues, ticket ledgers, submission caches and producer-state mirrors are deleted. The legacy getter fails closed when exact current counts are unavailable. | Pending GPU counts remain unavailable rather than zero, capacity or stale values. |
| Native ABI | The small v0.1 C layouts and symbols remain compatible; public wrappers serialize single-owner handles. Retained compatibility functions are thin translations. | Cross-product live geometry mutation involving Packed is unsupported; geometry selection is constructor-time except for the retained Direct/Paged compatibility transaction. | Mobile convenience APIs remain experimental validation surfaces, not polished published SDKs. |
| Web consumer | Rust/WASM and the local ESM wrapper use the shared Packed Surface session and renderer-owned scheduling/evidence. Exact startup fails closed. | A browser-owned second scheduler, sorted-index copy and implicit sampled WebGL2 product fallback are not retained. | Sampled WebGL2 is explicit opt-in diagnostic only; the Web API is not a stable or npm-published v0.1 surface. |
| Android and Apple consumers | JNI/Kotlin and Swift/GsplatKit adapt platform handles and receipts around the same native session. AAR and XCFramework are local/direct prerelease artifacts. | Platform wrappers do not own render policy, generations or terminal state. | Android is currently arm64-only and not Maven-published; GsplatKit is not a remote binary SwiftPM package. |
| PLY import | `metadata.rs` owns header/property/SH layout, `decode.rs` owns per-vertex numeric and coordinate interpretation, `stream.rs` owns byte/file traversal and incremental lifecycle, and `lib.rs` is the public facade with budgets/allocation/publication/import-policy choice. | The former mixed single-file owner is closed; failed incremental decode is terminal and cannot replay published callbacks. | The file-backed Packed summary/stream two-open snapshot boundary remains a separate Deferred hardening issue. |
| SPZ import | `gsplat-io-spz` remains one cohesive bounded `SPZ v4 -> validated SceneBuffers` transaction. | No duplicate product consumer or renderer path was introduced. | External-format interoperability, a product-selected non-default resource budget and C/Web/mobile consumption remain Deferred. |
| Distribution | Tagged 0.1.x GitHub prereleases may attach checksum-listed AAR, XCFramework ZIP and npm-compatible tarball files. | The repository does not claim Maven, remote binary SwiftPM, npm or crates.io publication. | Tagging, upload, GitHub settings and fresh remote verification remain release-operation gates. |

This final boundary matches [PROJECT_CONTEXT](../../../../handbook/PROJECT_CONTEXT.md),
[ARCHITECTURE](../../../../handbook/ARCHITECTURE.md),
[ROADMAP](../../../../handbook/ROADMAP.md), and
[GOLDEN_PRINCIPLES](../../../../handbook/GOLDEN_PRINCIPLES.md).

## Exact M0--M8 SHA ledger

All objects below exist and are ancestors of the last root-integrated report base
`c6ea13e41bde1a9d0f8af7252d343538603bc059`. A behavior/evidence
tip identifies the tree on which that milestone's implementation or retained
runtime evidence was accepted. A closeout/state tip records the later program
transition; it must not be presented as if endpoint evidence were rerun there.

| Stage | Accepted base or dependency | Behavior / evidence tip | Closeout / state tip | Recorded meaning |
| --- | --- | --- | --- | --- |
| Exact dependency | `d721ea6cd0c334e28d3ad5c28792383524e27935` | same | `328e05c4cb55f4824cc9149c08629dd9d2ad5eed` | Accepted Exact implementation, then E13 package closeout. |
| M0 | `328e05c4cb55f4824cc9149c08629dd9d2ad5eed` | `e26a1df39780e744112924eb378e098c29be7cd4` | `49821f9df4784e60bf176f4c6762694eb406f710` | Reviewed cutover contract, then M0 Accepted / M1 Active. |
| M1 | `49821f9df4784e60bf176f4c6762694eb406f710` | `dc3e0de65073f819b714229b726ca55f47c6e6d6` | `696687d385c1f2370104a2969f07e6ec15ce1132` | Native Packed offscreen, desktop/bench Exact evidence, then M1 Accepted / M2 Active. |
| M2 | `696687d385c1f2370104a2969f07e6ec15ce1132` | `742bd5a928123557a9016e50655724aa82ef9bd5` | `8ca6a661b0238164ee182adde2e427bf7d03f9a4` | Shared real-window Exact Surface and four-arm M4 Metal evidence, then M2 closeout. |
| M3 | `8ca6a661b0238164ee182adde2e427bf7d03f9a4` | `2618ae9f1e64fe46353d2d8f3713151736df935a` | `a3037b3a9d72dea3f79a1333decdd6de191bd12c` | C ABI compatibility state delegates to Renderer/Session, then M3 closeout. |
| M4 | `a3037b3a9d72dea3f79a1333decdd6de191bd12c` | `68651f659d9b97e3ef2fe2149feabbd1073b8b31` | `2d9f0e4d5769690a89abfad56da51a0d21fd89e1` | Browser WebGPU/WASM consumer evidence, then M4 acceptance record. |
| M5 | `2d9f0e4d5769690a89abfad56da51a0d21fd89e1` | `46dce2ec1bad4c09f4688dfef9d59202803b0e9c` | `df87ff177097cb8b42c592b5e2826fa9bfe8d4e4` | Android/A065 current-stats repair and strict evidence, then M5 closeout. |
| M6 | `df87ff177097cb8b42c592b5e2826fa9bfe8d4e4` | `8022841957a196824eb9669c477f11ce91d2aab1` | `4de4cbb38f98d5f69bc0aebe8a998f947c929879` | Apple/GsplatKit/XCFramework and Simulator evidence, then M6 acceptance record. |
| M7 | accepted M6 rollback tree `a798b8accf454e96792df45d38e8564e7e27556e` | `2739f4899facc03e6a0fb35c23b42d9762ede9cc` | `0d291c5ea9b911fdfce8d064ff6617ee6f9955ed` -> `2e16fff072d6832d602d35c53e1c0582220ec9a2` -> `1de3f79fa2fa22955f99c887bea421c918e31ee0` | 39-commit deletion/ownership/evidence tree, acceptance record, test-count correction and final policy closeout. `1de3f79` is the M8 base. |
| M8 | `1de3f79fa2fa22955f99c887bea421c918e31ee0` | no accepted final tip yet | activation `67e9bc5be3303af64291a87a6d34d75d1c984617`; D12 report `315f78e059ee0cd94b7653b4465e1937039d0131`; status repair `c6ea13e41bde1a9d0f8af7252d343538603bc059` | M8 remains Active. D12 is integrated. D15 remains Pending: the SHA produced by root integration of this status-admission repair must be frozen, receive a fresh exact-SHA matrix and pass independent review. Only then may D13 archive the bundle and mark M8 and Package M Accepted. |

### M8 integrated chain through D12

The chain from the accepted M7 tree through the integrated D12 status repair is
linear:

| SHA | Purpose |
| --- | --- |
| `67e9bc5be3303af64291a87a6d34d75d1c984617` | activate bounded M8 inventory |
| `a7939696507404b70c7d6a6a44c707a87adf4eef` | correct M7 browser qualification identity |
| `999d42499a1b76a43184027959e03475b4cbf235` | align canonical handbook facts |
| `f831792ed8cfb5c1f3e4e24513f56cd9bcf5441c` | align Web Exact verification guidance |
| `41e18f86aa9209bd1843b64f8db0f2489839fb9a` | align public release facts |
| `049957b130da889d0757d4c203a1371f8ad12755` | reconcile M7 ledger program status |
| `28a1f52196a476357fc7110b8e2c94da774779a1` | close SPZ ownership review |
| `b99adc51b853a2c99b7aa9015acbce2a20337202` | extract PLY metadata owner |
| `93f9dc662a6f1bac008def8ba4b541fb9a66ab4e` | repair release/distribution facts |
| `fdd37a9dec9728e4ac3672d2ca2bb55745b1d2d2` | make incremental PLY failure terminal |
| `93118eb31b6fc034eebb6eab17c54b271d95f7e4` | establish non-circular archive ordering |
| `0c9ba2316924769e373e540bd8186980e68aa647` | extract PLY per-vertex decode owner |
| `cee2a9707fe67c47bdc198c4338c6367fd5ff8ca` | extract PLY stream transport owner |
| `b33be292b76acb4b1d3cbf34655ff1d581263725` | close PLY architecture review/policy entry |
| `76188933a87a37f50175fa80720a429d50608ee7` | repair pre-archive active/completed paths |
| `315f78e059ee0cd94b7653b4465e1937039d0131` | populate and integrate the D12 final closeout report |
| `c6ea13e41bde1a9d0f8af7252d343538603bc059` | repair D12/D15 closeout status ordering |

Root integration of this status-admission repair defines the next pre-archive
D15 candidate. Root records that resulting SHA, which must be verified and
reviewed on its own rather than inheriting the `c6ea13e` results.

## Verification evidence ledger

### Accepted historical milestone evidence

| Milestone and source SHA | Accepted result | Evidence boundary |
| --- | --- | --- |
| M1 at `dc3e0de` | Workspace/quality gates, Apple M4 Metal, complete 279,199-splat SH3 Kitsune 1920x1080 Packed artifact and Direct oracle comparison passed. | The recorded 9.806 ms mean and 12.883 ms p95 are one observation, not a gate or competitor claim. |
| M2 at `742bd5a` | Four 1920x1080 Apple M4/Metal real-window arms (CPU PostSort, GPU PostSort, GPU Preproject, Adaptive) retained identical complete-scene PNG identity and validated terminal artifacts. | Correctness/evidence acceptance only; no CPU/GPU performance claim. |
| M3 at `2618ae9` | Formatter, 35 FFI tests, 14 renderer compatibility tests, real C smoke, workspace check, strict FFI Clippy, WASM compile and architecture policy passed. | No fresh real-window or physical mobile qualification. |
| M4 at `68651f6` | Chrome/WebGPU/WASM full Kitsune SH3 Packed Exact 1920x1080 moving-trace artifact and one-cell full-quality suite passed; all 80 frames retained complete current receipt facts. | One Chrome/WebGPU consumer result; no multi-browser, PlayCanvas or general performance claim. |
| M5 at `5cc7c97` / `46dce2e` | Nothing A065 forced CPU, forced GPU and Adaptive functional runs retained complete 279,199-splat SH3, 2412x1080 artifacts; Adaptive suite and 80 camera/current-stats receipts passed. | Separate directional runs, not a paired winner, resource, power, thermal or competitor experiment. |
| M6 at `8022841` | GsplatKit/XCFramework packaging tests and iPhone 17 Pro Simulator CPU/Adaptive full-scene 2622x1206 artifacts passed. | Forced GPU was explicitly unsupported in Simulator; physical iPhone remained unavailable and Deferred. |
| M7 at `2739f48` | Clean workspace/renderer/static/FFI/WASM checks, forced M4 Metal conformance, hidden Metal Surface, formal A065 Packed/CPU artifact and complete 39-commit rollback proof passed. | Chrome at this SHA, physical iPhone and Windows/Linux runtime remained Deferred. |

Detailed datasets, trace hashes, artifact paths and validator results remain in
the milestone closeouts. Machine-local `target/` artifacts are not committed
or guaranteed to survive their worktrees.

### M8 work completed through D12

- D01--D10 and documented D16 facts were independently reviewed and integrated
  with the narrow endpoint/release boundaries in the [M8 inventory](m8-closeout-inventory.md).
- D14 closed the SPZ owner without source changes and split PLY into accepted
  metadata/decode/stream/facade responsibilities. Architecture self-tests and
  real-tree policy passed with zero production grandfather entries; PLY tests
  passed 46/46.
- M8f's release/consumer documentation repair passed Android collector tests
  56/56 and verification-bootstrap tests 11/11. These are host tests, not a
  new Android device or browser qualification.
- D11 path repair was independently reviewed and integrated in the D12 base
  `7618893`; the inventory records it as Accepted. The only `active/` path
  retained by D11 is an explicitly historical, non-rerunnable pre-archive
  transcript.
- D12 itself ran only document links, Git-object/ancestry checks and diff
  hygiene. It did not run or pre-claim the D15 matrix.

## Accepted limitations and non-claims

- **Current-candidate endpoints:** no Chrome/WebGPU, physical iPhone, Windows
  or Linux runtime is qualified at the post-status-repair D15 candidate until
  an exact-SHA run actually produces such evidence. Historical results,
  including the matrix attempt at `c6ea13e`, keep their own SHAs and do not
  qualify the new candidate.
- **Android:** accepted A065 artifacts prove only their recorded full-quality
  configurations. They do not prove CPU/GPU superiority, sustained FPS,
  energy, thermal, memory or Vulkan-wide behavior.
- **Apple:** M6 Simulator function and M7 host Metal evidence do not establish
  physical-iPhone runtime or performance.
- **Web:** M4 proves one Chrome/WebGPU consumer configuration. Sampled WebGL2,
  WASM compilation and Node tests are not equivalent browser qualification.
- **Operating systems:** macOS/Metal, Simulator, WASM compilation or software
  Vulkan cannot be substituted for declared Windows or Linux product runtime.
- **Quality and capacity:** Exact Packed retains all points and SH0--SH3; this
  closeout does not activate LOD, sampling, remote streaming, arbitrary-scale
  loading, Balanced mode or Streamed mode.
- **Competition and performance:** no fixed FPS, speedup, PlayCanvas parity,
  memory leadership or broad native-versus-Web advantage is accepted here.
- **Import:** PLY's two-open file snapshot boundary and SPZ interoperability/
  product-consumer decisions remain Deferred.
- **Distribution:** historical `v0.1.3` release facts describe direct GitHub
  prerelease assets only. This M8 work performs no tag, upload, remote-setting
  mutation, checksum download or registry publication.
- **Public contract:** the stable v0.1 C ABI remains small. Experimental
  geometry selectors, Resident layouts, benchmark schemas, Web wrappers and
  mobile convenience APIs do not become stable merely because migration is
  internally complete.

## D15 freeze, review and D13 archive order

1. D12 is already integrated as `315f78e059ee0cd94b7653b4465e1937039d0131`,
   with its status-order repair integrated as
   `c6ea13e41bde1a9d0f8af7252d343538603bc059`. Root integration of this
   status-admission repair defines the sole pre-archive D15 candidate; root
   records that resulting clean SHA.
2. On that exact new tree, root reruns the applicable [global migration
   matrix](cutover.md#global-migration-matrix), M8 link/command validation,
   dependency policy, C/consumer checks affected by the integrated range, and
   committed-range diff checks. Results are joined to the candidate SHA; the
   prior `c6ea13e` run cannot substitute for them.
3. A separate visible read-only task reviews that exact SHA with a finite
   Accept/Reject/Deferred result and P0/P1/P2 findings. Missing historical
   endpoints stay Deferred unless a new run was actually required and made.
4. Only after D15 acceptance may root move this entire bundle atomically from
   `docs/plans/active/` to `docs/plans/completed/`, set `M8 = Accepted`, and
   declare Package M complete in one pure D13 archive/state commit.
5. The archive commit receives the documented relative-link, architecture
   self/real-tree policy, stale-active-path, diff-summary and clean-tree checks.
   The expensive D15 matrix is not rerun solely because paths moved.

Until all five steps finish, M8 and Package M remain **Active**.
