# Web SDK Package Slice

## Goal

Move the Web integration one step beyond validation-only direct `wasm-bindgen`
imports by adding a local browser ESM wrapper package.

## Scope

- Add a local `@gsplat-rs/web` package under `packages/web/`.
- Keep the Rust/WASM boundary in `crates/gsplat-web`.
- Add a build script that generates local wrapper dist plus wasm-bindgen output.
- Route the Web example's wasm path through the wrapper source.
- Document that this remains a local package slice, not a published npm release.

## Verification

- `node --check examples/web/src/main.js`
- `node --check packages/web/src/index.js`
- `bash packages/web/scripts/build.sh`
- `node --check packages/web/dist/index.js`
- `cargo check --workspace`
- `cargo check -p gsplat-web --target wasm32-unknown-unknown`
- static HTTP smoke for `examples/web/`, `src/main.js`,
  `packages/web/src/index.js`, and generated `pkg/gsplat_web.js`
- `git diff --check`
