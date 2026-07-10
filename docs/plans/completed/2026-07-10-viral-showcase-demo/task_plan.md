# Task Plan: Viral Showcase Demo

## Goal

Create a visually distinctive, shareable showcase for `gsplat-rs`, backed by a legally reusable Gaussian Splat scene and verified on the repository's supported demo path.

## Current Phase

Complete

## Phases

### Phase 1: Audit the Current Demo

- [x] Map the current Web, desktop, Android, and iOS demo surfaces.
- [x] Inspect current visual design, controls, asset-loading limits, and bundled datasets.
- [x] Record technical and release constraints.
- **Status:** complete

### Phase 2: Creative Direction and Model Research

- [x] Define a concise, high-impact demo story and interaction loop.
- [x] Find candidate Gaussian Splat models with clear licensing and practical size/performance.
- [x] Select a primary model and fallback acquisition path.
- **Status:** complete

### Phase 3: Implementation

- [x] Implement the showcase in the smallest appropriate existing demo surface.
- [x] Integrate an explicit model-loading/default-scene path without bloating the repository.
- [x] Preserve the existing renderer and release boundaries.
- **Status:** complete

### Phase 4: Verification and Visual Review

- [x] Run the canonical checks for every touched path.
- [x] Launch and inspect the demo in a real browser or supported runtime.
- [x] Fix functional, responsive, and visual issues found.
- **Status:** complete

### Phase 5: Delivery

- [x] Document the chosen model, license, acquisition steps, and demo rationale.
- [x] Summarize the result, verification evidence, and remaining deployment choices.
- **Status:** complete

## Key Questions

1. Which existing demo surface can deliver the strongest first impression with the least new product surface?
2. What model is visually memorable, redistributable, Web-feasible, and representative of the renderer?
3. What interaction can be understood and shared within five seconds?
4. Can the showcase remain useful when the hero model is not bundled locally?

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Start by evaluating the existing Web example as the likely public showcase | It has the lowest audience friction, but the choice remains contingent on local capability and browser verification. |

## Errors Encountered

| Error | Attempt | Resolution |
|-------|---------|------------|
| Initially looked for the skill under `~/.codex/skills` | 1 | Used the configured `r1` root: `~/.agents/skills/planning-with-files`. |
| Port 4173 was already in use when starting a local server | 1 | Reused the existing repository server after confirming the demo loaded from it. |
| Rendering the 3.45M-splat Loop CE `Nandi` scene exceeded `wgpu`'s 128 MB buffer binding limit | 1 | Excluded the original file and continued searching for a smaller asset or licensable derivative path. |
| Wakufactory trimmed models failed strict parsing because Scaniverse emits `+inf` for fully opaque logits | 1 | Confirmed the only non-finite field is opacity; plan a narrow import normalization with tests. |
| Initial `cargo fmt --check` found one formatting difference | 1 | Ran `cargo fmt`; targeted parser tests then passed. |
| Browser wait helpers hit the in-app browser's short selector deadline | 1 | Used fresh DOM snapshots, authoritative pressed states, computed styles, and direct screenshots instead of retrying the same wait. |
| Browser initially reused stale CSS and WASM assets | 1 | Added explicit showcase cache keys to CSS, JavaScript, generated module, and WASM URLs. |
| wasm-bindgen logged a deprecated positional initialization warning | 1 | Updated the Web wrapper to pass `{ module_or_path }`, rebuilt the package, and confirmed zero fresh browser warnings/errors. |

## Notes

- Keep `SortedAlpha` as the quality-guaranteed path.
- Do not add a new top-level app unless the evidence shows the existing demo cannot host the concept.
- No model may be redistributed until its source and license are verified.
