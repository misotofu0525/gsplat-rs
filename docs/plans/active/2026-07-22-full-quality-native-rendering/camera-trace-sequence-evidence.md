# Camera Trace Sequence Evidence

Date: 2026-07-22

This evidence verifies that the same checked-in v1 camera trace can drive every
available endpoint one changed camera revision at a time. It is a benchmark
control, not a quality shortcut: every run below used `sort_interval=1`, CPU
ordering was explicitly requested, and the complete three-splat fixture was
drawn on every frame.

## Shared receipt

- Trace fixture: `tests/perf/trace/fixtures/camera-trace-v1.json`
- Trace id: `contract-lateral-three-frame-v1`
- SHA-256: `e73f23c44f0cc1fb3fc2e533bcbce70989afe5f0b739e38601198f271484bca6`
- Default selected indices: `0,1,2`
- Timestamps in order: `0,16666667,33333334` nanoseconds
- Default schedule: zero warmup frames, three measured frames, one loop
- Dataset: `tests/datasets/minimal_ascii.ply`, 3 source splats

## Desktop Metal

Reproduction:

```sh
cargo run -p desktop-example -- \
  tests/datasets/minimal_ascii.ply \
  --camera-trace tests/perf/trace/fixtures/camera-trace-v1.json \
  --camera-sequence
```

Build product: `target/debug/desktop-example`.

Result: three `CAMERA_TRACE_FRAME` receipts appeared in the required index and
timestamp order. The final result reported `frames=3`, `visible_count=3`,
`drawn_count=3`, `requested_backend=cpu`, and
`offscreen_geometry_pipeline=sorted_index_direct`.

## Android Vulkan hardware

Device: Nothing A065 (`Pong`), Android 15 / API 35. Reproduction uses the
bundled minimal fixture. If the sample has a persisted imported scene, preserve
it with the reversible rename shown here and restore it after the run.

```sh
export JAVA_HOME=/opt/homebrew/Cellar/openjdk@21/21.0.11/libexec/openjdk.jdk/Contents/Home
export ANDROID_SDK_ROOT=/opt/homebrew/share/android-commandlinetools
ADB_BIN=/opt/homebrew/share/android-commandlinetools/platform-tools/adb

bash bindings/android/scripts/build-sample-apk.sh tests/datasets/minimal_ascii.ply
"$ADB_BIN" install -r examples/android/app/build/outputs/apk/debug/sample-app-debug.apk
"$ADB_BIN" push tests/perf/trace/fixtures/camera-trace-v1.json \
  /data/local/tmp/camera-trace-v1.json
"$ADB_BIN" shell "run-as com.gsplat.example cp \
  /data/local/tmp/camera-trace-v1.json \
  /data/user/0/com.gsplat.example/files/camera_trace.json"

"$ADB_BIN" shell am force-stop com.gsplat.example
"$ADB_BIN" shell "run-as com.gsplat.example mv \
  /data/user/0/com.gsplat.example/files/imported_scene.ply \
  /data/user/0/com.gsplat.example/files/imported_scene.ply.trace-sequence-backup"
"$ADB_BIN" logcat -c
"$ADB_BIN" shell am start -W -n com.gsplat.example/.MainActivity \
  --ez gsplat_benchmark true \
  --es gsplat_camera_trace_path \
    /data/user/0/com.gsplat.example/files/camera_trace.json \
  --ez gsplat_camera_trace_sequence true \
  --ei gsplat_surface_sort_interval 1 \
  --es gsplat_surface_order_backend cpu
"$ADB_BIN" logcat -d -t 1600 | \
  rg 'CAMERA_TRACE|BENCHMARK_RESULT|GSPLAT_BENCHMARK|panic|Validation Error'

"$ADB_BIN" shell am force-stop com.gsplat.example
"$ADB_BIN" shell "run-as com.gsplat.example mv \
  /data/user/0/com.gsplat.example/files/imported_scene.ply.trace-sequence-backup \
  /data/user/0/com.gsplat.example/files/imported_scene.ply"
```

Build product:
`examples/android/app/build/outputs/apk/debug/sample-app-debug.apk`.

Result: run id `7162bf68-920e-4b12-9475-1026fa727852` emitted three frame
records with trace indices `0,1,2`. Every record reported
`order_backend=cpu`, `sort_refreshed=true`, `gpu_sort_fallback=false`, and
`presented_order_revision_lag=0`. The result reported `samples=3`,
`visible=3`, and `drawn=3`. The pre-existing 630,225,580-byte Truck import was
restored to `files/imported_scene.ply` and its size was rechecked after the run.

## iOS Metal simulator

Runtime: booted iPhone 17 Pro simulator, arm64, iOS 26.2. Reproduction:

```sh
IOS_SIMULATOR_ID=3FC9AF01-5669-4BCF-A777-40649EBC9AFA
bash bindings/apple/scripts/build-ios-sim-app.sh tests/datasets/minimal_ascii.ply
xcrun simctl install "$IOS_SIMULATOR_ID" \
  target/ios-sim-app/GsplatIOSExample.app
xcrun simctl launch --terminate-running-process --console-pty \
  "$IOS_SIMULATOR_ID" com.gsplat.example.ios \
  --gsplat_benchmark true \
  --gsplat_camera_trace camera_trace.json \
  --gsplat_camera_trace_sequence true \
  --gsplat_surface_sort_interval 1 \
  --gsplat_surface_order_backend cpu
```

Build product: `target/ios-sim-app/GsplatIOSExample.app`.

Result: run id `ios-c22c5845-a20d-4b5a-9e9e-44d46a6da267` emitted the same
three frame receipts and timestamp order. The result reported `samples=3`,
`requested_backend=cpu`, `visible=3`, `drawn=3`, average call time 2.572 ms,
and average frame time 2.565 ms. The earlier low-binding-limit resident layout
creation failure did not recur.

## Web WebGPU browser

Runtime: Headless Chrome 150 on macOS, driven with `agent-browser`.

```sh
cargo check -p gsplat-web --target wasm32-unknown-unknown
export PATH=/Users/misotofu/.cargo/bin:$PATH
bash packages/web/scripts/build-wasm.sh
python3 -m http.server 4173 --bind 127.0.0.1 --directory .

agent-browser --session gsplat-trace-sequence open \
  'http://127.0.0.1:4173/examples/web/?dataset=minimal&gsplat_camera_trace_url=/tests/perf/trace/fixtures/camera-trace-v1.json&gsplat_camera_trace_sequence=true&gsplat_surface_sort_interval=1&gsplat_surface_order_backend=cpu&gsplat_benchmark=true&gsplat_benchmark_sync=true'
agent-browser --session gsplat-trace-sequence wait 5000
agent-browser --session gsplat-trace-sequence console
agent-browser --session gsplat-trace-sequence errors
agent-browser --session gsplat-trace-sequence close
```

Build products: `examples/web/pkg/gsplat_web.js` and
`examples/web/pkg/gsplat_web_bg.wasm`.

Result: run id `b49c2971-b032-4155-a4d8-77ef3794cea8` used the WASM surface
and WebGPU backend. All three frame records reported `order_backend=cpu`,
`gpu_sort_fallback=false`, and `drawn=3`. The result reported `samples=3`,
`visible=3`, `drawn=3`; the manifest recorded `frame_indices=[0,1,2]` and
`sort_interval=1`. The browser error stream was empty.

## Targeted verification

The following checks passed after all four endpoint runs:

```sh
cargo fmt --all -- --check
cargo test -p gsplat-core camera_trace
cargo test -p desktop-example trace_sequence
cargo test -p desktop-example fixed_camera_trace
node --test examples/web/test/camera-trace-v1.test.mjs
npm --prefix packages/web test
bash tests/perf/trace/test-trace-v1.sh
git diff --check
```

The fixed-frame desktop tests are retained because sequence playback is for
performance sampling; deterministic screenshot capture still uses one selected
trace frame.

## Android runtime-camera receipt hardening (2026-07-23)

The earlier Android artifact proved the requested trace index and the frame's
sort revision, but it did not expose the camera state actually adopted by the
Surface session. It therefore could not, by itself, rule out a frontend camera
mapping mismatch in a native-versus-PlayCanvas comparison.

The native path now emits `GsplatSurfaceCameraReceiptV1` immediately after each
successful present. It is derived from `SurfaceRenderSession::camera()` and the
current Surface aspect and includes the actual f32 pose/intrinsics, current and
presented revisions, and canonical row-major view, projection, and
`projection * view` matrices. The receipt does not read the trace cache.

The Android artifact records this receipt for every measured frame. Its
manifest pins the trace's raw file SHA-256, declared content hash, coordinate
system, matrix convention, f32 tolerance, playback mode, and schedule. The
formal extractor invokes an Android-specific validator before atomically
publishing the artifact; the validator reads the expected trace separately and
checks every frame's index/timestamp, Surface size, revision, pose, intrinsics,
and all 48 matrix elements. Fixed view 0/view 1 and sequence schedules share the
same validation path. Mutation tests cover missing receipts, wrong index,
stale revision, pose/matrix changes, and a wrong trace-file hash.

Fresh verification after the change:

```text
cargo test -p gsplat-ffi-c --lib                                      27 passed
bash tests/ffi/run-ffi-smoke.sh                                      passed
bash bindings/android/scripts/run-jni-smoke.sh                       passed
python3 -m unittest bindings/android/scripts/test_android_sort_benchmark_collector.py
                                                                      23 passed
bash bindings/android/scripts/test-android-benchmark-artifact-extraction.sh
                                                                      passed
Gradle :sample-app:testDebugUnitTest                                  passed
bash bindings/android/scripts/build-aar.sh                            passed
bash bindings/android/scripts/build-sample-apk.sh minimal_ascii.ply  passed
cargo clippy -p gsplat-ffi-c --all-targets -- -D warnings            passed
```

This is contract/build evidence only. A new Truck device artifact must pass
the receipt validator before replacing any performance conclusion based on the
older index-only artifacts.
