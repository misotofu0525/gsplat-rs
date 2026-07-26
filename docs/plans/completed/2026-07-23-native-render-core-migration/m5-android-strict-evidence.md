# M5 Android strict evidence closeout

## Status and claim boundary

- M5 is Accepted for its defined Android/JNI/AAR consumer and physical-device
  qualification scope. Package M remains Active: M6, M7 and M8 are not
  completed by this record.
- The retained runs are three separate, one-repetition observations on the
  Nothing A065 (`033ed212`). Each run is functional/directional evidence only.
  They are not a paired performance experiment and do not establish a CPU/GPU
  winner, a resource or power result, or a competitor comparison.
- All artifact paths below are machine-local, ignored `target/` evidence. Git
  contains this text record only; it contains no Kitsune PLY, PNG, APK, AAR,
  native library or other large evidence payload.

## Fixed implementation chain

- Forced CPU and forced GPU completed at clean source commit
  `5cc7c978e9857e6626cbe9bf9d7d521dbd375a7b` under the same formal
  conditions: one repetition, 20 warmups, 80 measured frames, sort interval
  1, Packed geometry, the canonical two-view Kitsune trace, 2412x1080, and
  thermal status 0 before and after each run.
- The first Adaptive run at `5cc7c97` failed closed on measured frame 1 before
  artifact publication: the order receipt existed, but strict current-stats
  admission had not yet reached a matching requested-and-`Issued` pre-ticket
  sample. That failed run is diagnostic evidence only and was not included in
  the accepted runs.
- Repair candidate `d902c040ffd2eedfb44c6e9c72ad691efd9d88fa` was reviewed at
  fixed SHA by root and integrated byte-for-byte for its three owned Android
  files as `46dce2ec1bad4c09f4688dfef9d59202803b0e9c`.
- The repair retains one sample binding and one camera revision and retries
  presentations until the existing request reports a real `Issued`
  submission. `Pending` remains unavailable. It creates no synthetic ticket,
  synchronous readback or CPU fallback, and it does not change `SortedAlpha`,
  source membership, SH degree or resolution.

## Retained strict runs

| Requested policy | Source commit | Run ID | Strict result | Local evidence root |
| --- | --- | --- | --- | --- |
| forced CPU | `5cc7c97` | `cbe17254-fb58-42b7-b253-49d8c1a8f86f` | 80/80 frames; generic artifact, strict current-stats and camera receipts pass | `/Users/misotofu/.codex/worktrees/native-render-root/gsplat-rs/target/android-sort-benchmarks/migration-m5-a065-kitsune-5cc7c97-cpu` |
| forced GPU | `5cc7c97` | `9532de4c-b5a8-42b0-a2b4-4ede48a9f86f` | 80/80 frames; generic artifact, strict current-stats and camera receipts pass | `/Users/misotofu/.codex/worktrees/native-render-root/gsplat-rs/target/android-sort-benchmarks/migration-m5-a065-kitsune-5cc7c97-gpu` |
| Adaptive | `46dce2e` | `bc28e7e4-8890-4e0a-b8c2-4d4060889ae8` | 80/80 frames; generic artifact, strict current-stats and camera receipts pass | `/Users/misotofu/.codex/worktrees/2ce4/gsplat-rs/target/android-sort-benchmarks/m5-a065-adaptive-46dce2ec-20260726-031909` |

Every accepted manifest records full requested, Surface, internal-render and
presented dimensions of 2412x1080; all 279,199 source, decoded, encoded,
resident and addressable SH3 splats; and sampling, LOD, dynamic resolution and
upscaling disabled. Every run uses the same dataset SHA-256
`3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2`
and trace file SHA-256
`f9a8316369966928e63816141ef61f3b38af8758cf307f94673e3a234a7daf44`.

The fresh `46dce2e` Adaptive run built a new release-native debug APK and
release AAR. The installed `base.apk` matched the local APK at 11,610,127
bytes and SHA-256
`d2ae0c1c941b436198ecc69f661b21fe80d00d91d4ff87c4a5f5fc68ae3eac67`;
the 4,063,071-byte AAR had SHA-256
`cc66cdc8f91045b4704133d7b2b42922c743ea565bbe64983fb94e1a30c77502`;
the APK and AAR contained the same 10,527,232-byte `libgsplat_jni.so` with
SHA-256
`71edb82e3e3b9116c35d0a1f5cdd348c9282b94132996c0e01d4a115ce1d70c0`.
Its final PNG is 2412x1080 with SHA-256
`0c4ef40b1318b903e62a992a6c20611fd6b8a00c42a7c8a3eabf8eabc2faba27`.

Adaptive actually selected 79 CPU frames and one GPU frame, with no recorded
GPU-sort fallback. This is a functional/directional observation of controller
execution, not a timing or performance conclusion.

## Adaptive suite publication and revalidation

The fresh `46dce2e` device run itself completed and passed the generic artifact,
strict current-stats and 80-frame camera-receipt validators. The collector's
first invocation did not publish `suite.json` solely because its Kitsune PLY
argument was an absolute path in another checkout; it failed the canonical
repository-path rule after the run artifact was complete. This was packaging
admission failure, not a render, receipt, image or device failure.

Without rerunning or touching the device, an isolated evidence task placed a
byte-identical physical copy at the canonical dataset path in its own
worktree. The copy was a regular file with a different inode, not a symlink;
it matched 65,892,441 bytes and the dataset SHA-256 above. Both the canonical
dataset directory and the derived output remained ignored, and tracked Git
status stayed clean.

The existing completed run was copied byte-for-byte into a fresh ignored
evidence directory and the repository publisher added only `suite.json`.
The copied historical `experiment.json` intentionally retains its original
failed packaging status; the separately published suite is complete. The
derived suite is retained at:

`/Users/misotofu/.codex/worktrees/2329/gsplat-rs/target/android-sort-benchmarks/m5-a065-adaptive-46dce2ec-postprocessed-suite/suite.json`

Its SHA-256 is
`d9029ac53946b45d19d0c14343416d8c7427076e3b907070f96125f2095eea53`.
Root independently revalidated:

- `validate-full-quality-experiment.py --verify-inputs`: one expected and one
  rendered cell, zero capacity rejections and zero missing cells;
- `validate-benchmark-artifacts.py`: pass for the copied Adaptive artifact;
- `validate-android-camera-receipts.py`: all 80 frames match the canonical
  camera trace.

The strict collector validator also accepts the unchanged Adaptive run as 80
measured Packed frames with `current_stats_strict=true`.

## Explicitly unverified

- CPU-versus-GPU or Adaptive performance superiority;
- device memory/resource use, energy use, power or sustained thermal behavior;
- GPU superiority or a general Android/Vulkan result;
- PlayCanvas image, timing, resource or competitive comparison;
- any inference from these temporary local paths after their worktrees or
  ignored artifacts are removed.
