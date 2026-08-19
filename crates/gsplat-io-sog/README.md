# gsplat-io-sog

PlayCanvas SOG and Streamed SOG import for `gsplat-rs`.

- Unbundled SOG (`meta.json` plus 8-bit images) and bundled `.sog` ZIP decode
  to one resident `SceneBuffers`, like PLY/SPZ. This is whole-scene import, not
  streaming. Archives accept STORED (the official writer) and DEFLATE.
- Streamed SOG (`lod-meta.json` plus chunk directories) is metadata-first.
  `assemble_streamed_sog` walks the spatial tree, selects leaves under
  independent source / decoded / gaussian / GPU-resident budgets, and
  returns only that subset. Native builds decode missing chunks in parallel.
  It never loads every chunk into one hidden full scene.

Coordinate conversion matches SPZ: SOG stores RUB, runtime `SceneBuffers` are
RUF.

## License

MIT OR Apache-2.0, at your option.
