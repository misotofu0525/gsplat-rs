import Foundation

func fail(_ message: String, code: Int32) -> Never {
    fputs("\(message) (code=\(code))\n", stderr)
    exit(Int32(code == 0 ? 1 : code))
}

let datasetPath = CommandLine.arguments.count > 1
    ? CommandLine.arguments[1]
    : "tests/datasets/minimal_ascii.ply"

if gsplat_version_major() != 0 || gsplat_version_minor() != 1 {
    fail("unexpected ABI version", code: 10)
}

var config = GsplatConfig(width: 1280, height: 720, mode: 0)
var ctx: OpaquePointer? = nil

var rc = gsplat_context_create(config, &ctx)
if rc != 0 || ctx == nil {
    fail("gsplat_context_create failed", code: rc)
}

defer { gsplat_context_destroy(ctx) }

var camera = GsplatCamera()
camera.position = (0, 0, 0)
camera.rotation_xyzw = (0, 0, 0, 1)
camera.vertical_fov_radians = 1.0471976
camera.near_plane = 0.01
camera.far_plane = 1000.0

rc = gsplat_context_set_camera(ctx, camera)
if rc != 0 {
    fail("gsplat_context_set_camera failed", code: rc)
}

rc = datasetPath.withCString { path in
    gsplat_context_load_scene_path(ctx, path)
}
if rc != 0 {
    fail("gsplat_context_load_scene_path failed", code: rc)
}

rc = gsplat_context_render_frame(ctx)
if rc != 0 {
    fail("gsplat_context_render_frame failed", code: rc)
}

var stats = GsplatStats()
rc = gsplat_context_get_stats(ctx, &stats)
if rc != 0 {
    fail("gsplat_context_get_stats failed", code: rc)
}

print("swift smoke ok")
let frameMs = String(format: "%.4f", stats.frame_ms)
print("drawn=\(stats.drawn_count) visible=\(stats.visible_count) frame_ms=\(frameMs)")
