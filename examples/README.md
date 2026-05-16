# Examples

Runnable validation examples for the shared `gsplat-rs` crates.

- `desktop/`: desktop viewer and offscreen PNG smoke harness
- `android/`: Android Surface sample app
- `ios/`: UIKit realtime Surface sample app
- `web/`: browser PLY loader and WebGL2/WASM validation surface

Reusable platform bindings and package build scripts live under `bindings/` and
`packages/`; examples should consume those layers rather than own packaging
logic.
