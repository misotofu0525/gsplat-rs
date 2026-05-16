import Foundation

func fail(_ error: Error) -> Never {
    fputs("swift smoke failed: \(error)\n", stderr)
    if let error = error as? GsplatKitError, error.code > 0 {
        exit(error.code)
    }
    exit(1)
}

let datasetPath = CommandLine.arguments.count > 1
    ? CommandLine.arguments[1]
    : "tests/datasets/minimal_ascii.ply"

do {
    let renderer = try GsplatContextRenderer()
    defer { renderer.close() }

    try renderer.setDefaultCamera()
    try renderer.loadScene(path: datasetPath)
    try renderer.renderFrame()

    let stats = try renderer.stats()
    print("swift smoke ok")
    let frameMs = String(format: "%.4f", stats.frameMs)
    print("drawn=\(stats.drawnCount) visible=\(stats.visibleCount) frame_ms=\(frameMs)")
} catch {
    fail(error)
}
