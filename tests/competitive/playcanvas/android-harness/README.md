# Android WebView presentation host

This disposable app is a presentation-only host for the pinned PlayCanvas
competitive harness. It provides one hardware-accelerated WebView with:

- immersive edge-to-edge system-bar hiding;
- `layoutInDisplayCutoutMode=always` and a full-window `MATCH_PARENT` surface;
- WebView CDP debugging for the existing host-side collector; and
- loopback-only navigation to the repository's pinned HTTP harness.

It does not contain a renderer, change PlayCanvas, convert assets, select a
different sort path, resize the render target, or implement LOD. Formal quality
and timing policy remains in `../public/main.js` and
`../scripts/run-timed-benchmark.mjs`.

## Build

Use JDK 17 or newer and the same Android SDK 35 installation used by the main
Android binding:

```bash
export JAVA_HOME='<jdk-home>'
export ANDROID_HOME='<android-sdk-home>'
export ANDROID_SDK_ROOT="$ANDROID_HOME"

GRADLE_BIN="$(bash bindings/android/scripts/ensure-gradle.sh)"
"$GRADLE_BIN" \
  --project-dir tests/competitive/playcanvas/android-harness \
  --no-daemon \
  :app:assembleDebug
```

The APK is generated at
`tests/competitive/playcanvas/android-harness/app/build/outputs/apk/debug/app-debug.apk`.

## Start and expose CDP

The operator must leave the device unlocked and physically in landscape. The
collector does not wake, unlock, tap, swipe, or change display settings.

```bash
SERIAL='<adb-serial>'
ADB_CDP_PORT='9223'

adb -s "$SERIAL" reverse tcp:4174 tcp:4174
adb -s "$SERIAL" install -r -t \
  tests/competitive/playcanvas/android-harness/app/build/outputs/apk/debug/app-debug.apk
adb -s "$SERIAL" shell am start -W \
  -n com.gsplat.competitive.playcanvas/.MainActivity \
  --es qualification_url 'http://127.0.0.1:4174/?probe=1'

WEBVIEW_PID="$(adb -s "$SERIAL" shell pidof com.gsplat.competitive.playcanvas | tr -d '\r')"
test -n "$WEBVIEW_PID"
WEBVIEW_SOCKET="webview_devtools_remote_$WEBVIEW_PID"
adb -s "$SERIAL" forward "tcp:$ADB_CDP_PORT" "localabstract:$WEBVIEW_SOCKET"
```

`curl http://127.0.0.1:9223/json/version` must report both
`Android-Package: com.gsplat.competitive.playcanvas` and the installed System
WebView Chromium version. `/json/list` must expose exactly one visible page.

## Full-Truck qualification

This command keeps the complete source PLY and SH degree, fixed 2412x1080
backing/internal/presented pixels, moving two-view trace, 20 warmup frames, and
80 measured frames:

```bash
PHASE_E_QUALIFICATION=truck-quality-2412x1080-v1 \
PLAYCANVAS_CDP_ENDPOINT="http://127.0.0.1:$ADB_CDP_PORT" \
PLAYCANVAS_HARNESS_PORT=4174 \
PLAYCANVAS_ADB_SERIAL="$SERIAL" \
PLAYCANVAS_REFRESH_HZ=90 \
PLAYCANVAS_VIEWPORT_WIDTH=2412 \
PLAYCANVAS_VIEWPORT_HEIGHT=1080 \
PLAYCANVAS_CAMERA_MODE=sequence \
PLAYCANVAS_WARMUP_FRAMES=20 \
PLAYCANVAS_MEASURED_FRAMES=80 \
PLAYCANVAS_ANDROID_RUNTIME=webview \
PLAYCANVAS_ANDROID_HOST_PACKAGE=com.gsplat.competitive.playcanvas \
PLAYCANVAS_ANDROID_WEBVIEW_PACKAGE=com.google.android.webview \
PLAYCANVAS_ANDROID_CDP_SOCKET="$WEBVIEW_SOCKET" \
PLAYCANVAS_ARTIFACT_DIR='<fresh-absolute-output-directory>' \
node tests/competitive/playcanvas/scripts/run-timed-benchmark.mjs
```

The run fails closed unless both pre/post WindowManager receipts prove exact
2412x1080 parent/display/window frames, zero content and visible insets, a shown
and unobscured surface, and the host as top-resumed. The browser contract still
requires exact inner/canvas/backing/internal pixels. Only the observed Chromium
`visualViewport` floating report receives a bounded 0.25 CSS-pixel tolerance;
the unobscured gate samples one pixel inside all four corners and the center.
The original half-pixel samples remain in every receipt as diagnostics.

After the final collection, remove only the mappings created above:

```bash
adb -s "$SERIAL" forward --remove "tcp:$ADB_CDP_PORT"
adb -s "$SERIAL" reverse --remove tcp:4174
```
