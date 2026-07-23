# Native Render Exact Core Progress

> Parent roadmap:
> [Native Render Core Refactor](../../completed/2026-07-23-native-render-core-refactor/task_plan.md)

## Machine task-state registry

<!-- gsplat-program-task-states: begin -->
E0 = Active
<!-- gsplat-program-task-states: end -->

## Package status

- Package: E — Exact prepared plans and native execution, still shadowed.
- Active task: E0 — freeze the Exact prepared-plan contract and migration
  oracle.
- Dependency: Package A/A9 Accepted.
- Product default: unchanged legacy renderer/session path.
- Parallel writer lanes: none. E0 and E1 are shared foundations and remain
  sequential; disjoint implementation lanes begin only after E1 establishes
  their interfaces.
- Source-size rule: no fixed LOC quota, split trigger or completion gate. Module
  boundaries follow responsibility, dependency direction, compatibility,
  testability and maintenance risk. A new mixed-responsibility multi-thousand
  owner requires finite review but does not force an artificial split.
- Performance rule: no fixed FPS or competitor percentage gate. Correctness,
  exactness, fail-closed publication and truthful evidence are hard; measured
  performance determines scoped admission or rejection.

## E0 active contract

- Hypothesis: the existing exact-count proofs can be mapped to one small,
  immutable prepared-plan vocabulary before implementation, preventing E1 and
  later parallel lanes from inventing competing state or controller models.
- Deliverables in this package directory:
  - `contract.md`: prepared runtime, plan, cache, generation and transactional
    publication responsibilities;
  - `oracle.md`: source membership, SH, order, image, state, ABI and endpoint
    evidence mapped to the target owners and later tasks;
  - this ledger updated to terminal E0 state and the next eligible task.
- Allowed scope: this package directory only. E0 does not modify production
  Rust/WGSL, public API/ABI, platform wrappers, benchmark schemas or product
  defaults.
- Required review: verify every target owner is named once, every accepted
  inherited proof retains its exact scope, every missing proof maps to a later
  task, and no aspirational E implementation is described as present.
- Verification: Markdown links, machine-ledger checker/self-tests,
  `git diff --check` and exact file scope.
- Writer model: one user-visible Codex task in an isolated worktree. Subagents
  may audit read-only but do not write the deliverables.
