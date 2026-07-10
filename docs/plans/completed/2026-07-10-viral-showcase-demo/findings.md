# Findings & Decisions

## Requirements

- Replace the current unremarkable demo experience with a memorable, highly shareable showcase.
- Improve the project's open-source appeal without widening the renderer's release contract.
- Find a stronger Gaussian Splat model suitable for the demo.
- Verify model licensing, size, loading path, and renderer compatibility before integration.

## Research Findings

- `gsplat-rs` is a cross-platform Rust + `wgpu` renderer; the Web example is explicitly an experimental browser validation surface rather than a polished product.
- `SortedAlpha` is the only release-gated render mode and must remain the showcase path.
- Existing examples are intended to validate shared crates, so a showcase should enhance an existing surface rather than create a new top-level product track.
- The repository currently has a local `@gsplat-rs/web` wrapper and a browser PLY loader, making Web the leading candidate for a frictionless public demo, subject to detailed audit.
- The Web example already has wasm-first rendering with a WebGL2 fallback, URL dataset selection, local PLY upload, orbit/zoom/pan/reset gestures, stats, and benchmark query parameters. The main gap is presentation and default content, not basic interaction plumbing.
- The current startup scene is `minimal_ascii.ply` (566 B) and the only documented larger scene is NVIDIA Flowers (133 MB, CC BY 4.0). The minimal scene is useful for smoke tests but unsuitable as a public first impression.
- The Web renderer auto-frames scene bounds and exposes camera operations through the wrapper, so a cinematic auto-orbit and authored starting composition can be implemented largely in the example without widening the public Rust API.
- Existing local, ignored candidate assets include LoopCE `basket.ply` (240 MB), LoopCE `nandi.ply` (817 MB), and a Steam Studio chunked binary set. Their source, format compatibility, and redistribution terms still require verification.
- A raw 133 MB PLY is already expensive for a zero-friction Web hero; 240–817 MB candidates would need an explicit desktop/local path or an optimized derivative before they could be considered Web defaults.
- Loop CE publishes a benchmark collection under CC BY 4.0 with explicit permission to share and adapt, including commercial use with attribution. Candidate subjects include bench, rock, beetle, hydrant, Nandi, lamp, basket, panel, and tree.
- The Loop CE collection is captured on smartphones under unconstrained conditions and distributed uncompressed. Its license is suitable, but the locally available 240 MB and 817 MB files are not Web-first assets at their current size.
- Steam Studio publishes a high-detail studio capture under CC0. Its source notes roughly two million splats for the high-quality version and recommends only 100k-200k splats for mobile Web delivery; the downloaded local parts include compressed and uncompressed variants that still need format inspection.
- The University of Vienna publishes a Gaussian Splat of a church crypt under CC BY 4.0. It is visually distinctive and culturally grounded, but automated access is protected and practical download size/format still need verification.
- A paid still-life pack advertises CC BY PLY files, but paid acquisition adds friction and is unnecessary while strong open candidates exist.
- Dataset API verification shows the Loop CE repository's PLY files range from roughly 251 MB (`basket`) to 1.82 GB (`rock`). Even the smallest file contains about 1.01 million splats, well above the intended 100k-200k Web target.
- The project renderer successfully loaded and rendered `basket`, but one 1280x720 development frame took about 320 ms and the automatic first composition was noisy and over-bright. It is not a suitable Web hero without curation and reduction.
- `Nandi` is the strongest Loop CE subject visually: an ornate stone bull sculpture with a clear silhouette and culturally specific detail. Its original PLY contains 3,454,040 splats and fails the current renderer because the instance buffer exceeds `wgpu`'s 128 MB binding limit.
- The official Loop CE preview confirms `basket` is a still-life picnic basket in a cluttered room, while `Nandi` has a centered sculptural composition. If Loop CE is used, an attributed reduced derivative of `Nandi` is the only candidate with enough visual identity.
- Wakufactory publishes unmodified Scaniverse PLY samples under CC0, including trimmed `kitune` (65.9 MB, 279,199 splats) and `sakura` (58.6 MB, 236,178 splats). Both land directly in the desired Web size and splat-count range.
- `kitune` is the preferred story candidate: a Japanese shrine fox statue has a strong silhouette, recognizable subject, and a natural connection to the proposed theme of capturing a place as light.
- Both Wakufactory files are standard binary-little-endian 3DGS PLYs. They currently fail `gsplat-io-ply` only because a small number of fully opaque splats use positive infinity for the opacity logit (186 in `kitune`, 433 in `sakura`); every other field is finite.
- Normalizing only infinite opacity logits to a large finite logit is a narrow compatibility fix. It preserves the intended post-sigmoid alpha while keeping the project's finite-buffer invariant.
- Fresh project renders confirm the decision: `kitune` produces a clean isolated fox statue on a stone pedestal, with a vivid red bib and strong 360-degree silhouette. The development render completed in about 102 ms for 279,199 visible splats.
- `sakura` also renders after normalization and is slightly faster (about 95 ms for 236,178 splats), but the partial tree trunk and flower branch read as a cropped scan rather than a complete hero object.
- The `kitune` subject naturally supports an asymmetric composition: large title and status on the left, fox statue offset to the right, with auto-orbit communicating that the image is live rather than a photograph.
- The finished shell keeps the live canvas full viewport, puts the project story in a two-line hero, moves datasets into a bottom scene rail, and collapses renderer controls, timing, benchmark, and diagnostics under Studio.
- Startup now streams model bytes into a determinate loading screen. It tries Kitsune, Flowers, then the minimal smoke scene, so a fresh clone remains functional even when optional datasets are absent.
- The model remains external to git and is installed reproducibly by `tests/datasets/fetch-wakufactory-kitune.sh`, including a pinned SHA-256 check and source/license output.
- Fresh browser inspection confirms the rebuilt Rust/WASM path renders Kitsune correctly. At 1440x900 the live panel reported about 43.7 FPS during inspection, with 279,199 splats drawn.
- Desktop dark mode, desktop light mode, and a 390x844 mobile layout were visually inspected. The mobile layout keeps the two-line hero and both CTAs visible, hides the secondary scene card, and leaves the three scene choices in a bottom rail.
- Browser interaction checks confirmed Studio expansion, theme switching, scene selection, active-scene state, and automatic return to the full Kitsune scene.
- Final browser reload produced no new console warnings or errors after the wrapper initialization update.
- Repository verification passed across the full Rust workspace, Clippy with warnings denied, rustdoc with warnings denied, Web wrapper checks/tests/pack dry-run, WASM build, JavaScript syntax, fetch-script checksum, and diff hygiene.

## Technical Decisions

| Decision | Rationale |
|----------|-----------|
| Treat model selection as part of product design, not a late asset swap | Subject, composition, file size, license, initial camera, and interaction determine whether the demo feels compelling. |
| Do not commit a third-party model until redistribution rights are explicit | Public download availability alone does not grant repository redistribution rights. |
| Build the public-facing treatment inside `examples/web/` | The surface already owns browser loading, interaction, fallback rendering, and benchmark controls; this keeps the repository shape small. |
| Use the trimmed Wakufactory `kitune` scene as the primary showcase model, with Flowers as the existing fallback | `kitune` is CC0, 65.9 MB, 279k splats, visually specific, and Web-feasible; the current Flowers path remains useful when the optional showcase asset is absent. |
| Add strict, field-specific normalization for infinite opacity logits | Scaniverse uses `+inf` to encode full opacity. Other non-finite values must continue to fail parsing. |
| Lock the hero model to `kitune`; keep `sakura` as a documented alternate, not a second default | `kitune` has the clearer silhouette, complete subject, stronger accent color, and better shareable thumbnail. |

## Issues Encountered

| Issue | Resolution |
|-------|------------|
| Planning skill path did not match the first assumed directory | Resolved through the provided skill-root mapping. |
| Starting a new HTTP server on port 4173 failed because the port was already occupied | Verified that the existing local server already serves the repository and reused it for browser inspection. |
| Loop CE `Nandi` render exceeded the 128 MB `wgpu` buffer binding limit | Rejected the original 3.45M-splat asset as a direct demo model; only a reduced derivative could be considered. |
| Wakufactory files failed with `failed to parse numeric value` | Diagnosed all non-finite values as fully opaque `opacity=+inf`; implement a narrow normalization rather than weakening validation globally. |
| CSS and WASM looked stale after the first browser reload | Added cache keys to the style, main module, generated WASM module, and WASM binary requests. |
| Selector waits timed out even after the target state changed | Switched to fresh DOM snapshots, pressed-state verification, and read-only computed-style checks. |
| wasm-bindgen warned about positional initialization parameters | Updated `initGsplatWeb()` to pass `{ module_or_path }`, updated its unit test, rebuilt, and confirmed a clean fresh console. |

## Resources

- `handbook/PROJECT_CONTEXT.md`
- `handbook/ROADMAP.md`
- `handbook/VERIFICATION.md`
- `handbook/ARCHITECTURE.md`
- `handbook/GOLDEN_PRINCIPLES.md`
- `examples/web/README.md`
- `packages/web/README.md`
- Loop CE Gaussian Splats: https://loopce.com/gaussian-splats
- Loop CE dataset card: https://huggingface.co/datasets/loopce/gaussiansplats
- Steam Studio CC0 sample article: https://note.com/steam_studio/n/ne9736d94f162
- University of Vienna crypt model record: https://phaidra.univie.ac.at/detail/o:2118791
- Wakufactory CC0 samples: https://www.wakufactory.jp/wxr/splats/sample.html

## Visual/Browser Findings

- The current desktop first view is dominated by a 360 px pale control sidebar with four stacked engineering panels. The actual render viewport receives less emphasis than the controls.
- The 566 B minimal scene renders only three splats, so the right side reads as an empty black grid even though the renderer is working.
- The largest visible strings are `gsplat-rs Web Example`, file/debug labels, and a multiline status dump. There is no public-facing value proposition, immediate interaction invitation, or memorable visual composition.
- The functional foundation is healthy: the browser DOM exposes all controls, the generated wasm package is available, and the page reaches the Rust/WASM + `wgpu` Surface path locally.
- Existing design dials are approximately variance 3, motion 2, density 7. The approved overhaul target is variance 8, motion 7, density 3.
- `kitune` visually reads as a floating shrine sculpture against black, with a bright red neck cloth and red carved detail. It remains recognizable at thumbnail scale.
- `sakura` is delicate but compositionally incomplete: the trunk and branch fade into black and leave the right side empty. It is better as an alternate dataset than the hero.
- Dark desktop mode reads as a cinematic black field with off-white type, the stone fox centered to the right, and a single coral accent matching the statue's cloth.
- Light mode uses one intentional split from warm silver to the black render field. Text, scene metadata, controls, and the fox remain legible without changing the canvas renderer.
- At 390x844, the model sits behind and below the copy without covering the CTAs; the scene rail remains fully usable at the bottom.
