# gsplat-core

Core data types and API contracts shared by every `gsplat-rs` crate.

This crate is dependency-free and defines the vocabulary of the project:

- `SceneBuffers`: validated in-memory Gaussian splat scene data
- `Camera`, `CameraPose`, `CameraIntrinsics`: camera model
- `RendererConfig`, `RenderMode`: renderer configuration; `SortedAlpha` is the
  only release-gated render mode on the `0.1.x` line
- `FrameStats`: per-frame timing and visibility counters
- `ErrorCode`: stable error codes shared with the C ABI

## Position in the workspace

`gsplat-core` sits at the bottom of the crate graph. `gsplat-io-ply` produces
`SceneBuffers`, `gsplat-render-wgpu` consumes them, and `gsplat-ffi-c` maps the
types onto the C ABI.

See the [repository README](https://github.com/misotofu0525/gsplat-rs) for the
project-level picture.

## License

MIT OR Apache-2.0, at your option.
