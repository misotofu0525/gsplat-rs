# Web SDK Package Slice

## Goal

Move the Web integration one step beyond demo-only direct `wasm-bindgen`
imports by adding a local browser ESM wrapper package.

## Scope

- Add a local `@gsplat-rs/web` package under `apps/web-demo/gsplat-web-sdk/`.
- Keep the Rust/WASM boundary in `crates/gsplat-web`.
- Add a build script that generates local wrapper dist plus wasm-bindgen output.
- Route the Web demo's wasm path through the wrapper source.
- Document that this remains a local package slice, not a published npm release.

## Verification

- `node --check apps/web-demo/src/main.js`
- `node --check apps/web-demo/gsplat-web-sdk/src/index.js`
- `bash apps/web-demo/build-web-sdk.sh`
- `node --check apps/web-demo/gsplat-web-sdk/dist/index.js`
- `cargo check --workspace`
- `cargo check -p gsplat-web --target wasm32-unknown-unknown`
- static HTTP smoke for `apps/web-demo/`, `src/main.js`,
  `gsplat-web-sdk/src/index.js`, and generated `pkg/gsplat_web.js`
- `git diff --check`
