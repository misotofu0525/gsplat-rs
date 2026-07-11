import CryptoKit
import Foundation
import UIKit

private let benchmarkSchema = "gsplat-benchmark/v1"

struct BenchmarkSample {
    let elapsedNs: UInt64
    let callMs: Double
    let frameWallMs: Double
    let rendererFrameMs: Double
    let preprocessMs: Double
    let sortMs: Double
    let geometrySubmitMs: Double
    let visible: UInt32
    let drawn: UInt32
}

final class SurfaceBenchmark {
    let config: BenchmarkConfig
    private(set) var complete = false
    private var observedFrames = 0
    private var samples: [BenchmarkSample]
    private var runStartedAt = Date()
    private var measurementStartedAt: Date?
    private var measurementEndedAt: Date?
    private var measurementStartNs: UInt64?
    private var previousFrameStartNs: UInt64?
    private let initialThermalState = ProcessInfo.processInfo.thermalState
    private var finalThermalState: ProcessInfo.ThermalState?

    init(config: BenchmarkConfig) {
        self.config = config
        samples = []
        samples.reserveCapacity(config.frames)
    }

    func record(stats: GsplatStats, renderCallNs: UInt64, frameStartNs: UInt64) {
        guard config.enabled, !complete else { return }
        observedFrames += 1
        if observedFrames <= config.warmupFrames {
            previousFrameStartNs = frameStartNs
            return
        }

        if measurementStartNs == nil {
            measurementStartNs = frameStartNs
            measurementStartedAt = Date()
        }
        let wallNs = previousFrameStartNs.map { frameStartNs - $0 } ?? renderCallNs
        samples.append(BenchmarkSample(
            elapsedNs: frameStartNs - (measurementStartNs ?? frameStartNs),
            callMs: Double(renderCallNs) / 1_000_000.0,
            frameWallMs: Double(wallNs) / 1_000_000.0,
            rendererFrameMs: Double(stats.frame_ms),
            preprocessMs: Double(stats.preprocess_ms),
            sortMs: Double(stats.sort_ms),
            geometrySubmitMs: Double(stats.raster_ms),
            visible: stats.visible_count,
            drawn: stats.drawn_count
        ))
        previousFrameStartNs = frameStartNs
        complete = samples.count >= config.frames
        if complete {
            measurementEndedAt = Date()
            finalThermalState = ProcessInfo.processInfo.thermalState
        }
    }

    func emitArtifacts(datasetPath: String, datasetLabel: String, width: Int, height: Int) {
        let runEndedAt = Date()
        let runID = "ios-\(UUID().uuidString.lowercased())"
        let seriesID = "ios-native-surface"
        guard let dataset = inspectDataset(path: datasetPath) else {
            print("BENCHMARK_ARTIFACT_ERROR dataset metadata unavailable path=\(datasetPath)")
            return
        }
        let frameBudgetMs = 1000.0 / 60.0
        let unavailable = [
            "build.repository_commit", "build.dirty", "environment.browser",
            "environment.driver", "frames[*].gpu_wait_ms",
            "frames[*].gpu_complete_ms", "frames[*].sort_refreshed",
        ]
        let manifest: [String: Any] = [
            "schema": benchmarkSchema, "record_type": "manifest", "run_id": runID,
            "identity": [
                "series_id": seriesID,
                "started_at_utc": utc(runStartedAt), "ended_at_utc": utc(runEndedAt),
                "measurement_started_at_utc": utc(measurementStartedAt ?? runStartedAt),
                "measurement_ended_at_utc": utc(measurementEndedAt ?? runEndedAt),
            ],
            "build": [
                "repository_commit": NSNull(), "dirty": NSNull(), "profile": "release",
                "package_version": Bundle.main.object(
                    forInfoDictionaryKey: "CFBundleShortVersionString"
                ) as? String ?? "\(gsplat_version_major()).\(gsplat_version_minor())",
            ],
            "dataset": [
                "id": datasetLabel, "sha256": dataset.sha256, "bytes": dataset.bytes,
                "splat_count": dataset.splatCount, "sh_degree": dataset.shDegree,
            ],
            "trace": ["id": "orbit-yaw-step-\(config.yawStepRadians)", "sha256": traceHash()],
            "renderer": [
                "implementation": "gsplat-rs", "path": geometryPipelineName(config.geometryPath), "backend": "metal",
                "sort_policy": config.sortInterval == 1 ? "cpu_every_frame" : "cpu_interval_\(config.sortInterval)",
            ],
            "display": [
                "width": width, "height": height, "dpr": Double(UIScreen.main.scale),
                "refresh_hz": 60.0, "frame_budget_ms": frameBudgetMs,
                "refresh_hz_source": "configured", "frame_budget_source": "configured",
            ],
            "environment": [
                "platform": "ios", "os": UIDevice.current.systemVersion,
                "device": machineIdentifier(), "browser": NSNull(), "adapter": "Apple GPU",
                "driver": NSNull(), "thermal_state_start": thermalName(initialThermalState),
                "thermal_state_end": thermalName(finalThermalState ?? initialThermalState),
            ],
            "unavailable_fields": unavailable,
        ]

        emit(kind: "manifest", object: manifest)
        for (index, sample) in samples.enumerated() {
            emit(kind: "frame", object: [
                "schema": benchmarkSchema, "record_type": "frame", "run_id": runID,
                "frame_index": index, "elapsed_ns": sample.elapsedNs, "call_ms": sample.callMs,
                "frame_wall_ms": sample.frameWallMs, "preprocess_ms": sample.preprocessMs,
                "sort_ms": sample.sortMs, "geometry_submit_ms": sample.geometrySubmitMs,
                "gpu_wait_ms": NSNull(), "gpu_complete_ms": NSNull(),
                "visible": sample.visible, "drawn": sample.drawn, "sort_refreshed": NSNull(),
            ])
        }
        emit(kind: "summary", object: summary(runID: runID, frameBudgetMs: frameBudgetMs))
    }

    func resultLine(datasetLabel: String) -> String {
        let count = max(samples.count, 1)
        func mean(_ value: (BenchmarkSample) -> Double) -> Double {
            samples.reduce(0.0) { $0 + value($1) } / Double(count)
        }
        let visible = samples.reduce(UInt64(0)) { $0 + UInt64($1.visible) } / UInt64(count)
        let drawn = samples.reduce(UInt64(0)) { $0 + UInt64($1.drawn) } / UInt64(count)
        return [
            "BENCHMARK_RESULT", "dataset=\(datasetLabel)", "samples=\(samples.count)",
            "warmup=\(config.warmupFrames)", "sort_interval=\(config.sortInterval)",
            "async_sort=\(config.asyncSort)", "geometry_pipeline=\(geometryPipelineName(config.geometryPath))",
            "frame_latency=\(config.frameLatency)", "avg_call_ms=\(format(mean { $0.callMs }))",
            "avg_frame_ms=\(format(mean { $0.rendererFrameMs }))",
            "avg_preprocess_ms=\(format(mean { $0.preprocessMs }))",
            "avg_sort_ms=\(format(mean { $0.sortMs }))",
            "avg_raster_ms=\(format(mean { $0.geometrySubmitMs }))",
            "avg_visible=\(visible)", "avg_drawn=\(drawn)",
        ].joined(separator: " ")
    }

    private func summary(runID: String, frameBudgetMs: Double) -> [String: Any] {
        let wall = samples.map(\.frameWallMs)
        return [
            "schema": benchmarkSchema, "record_type": "summary", "run_id": runID,
            "sample_count": samples.count, "warmup_count": config.warmupFrames,
            "frame_budget_ms": frameBudgetMs,
            "missed_frame_count": wall.filter { $0 > frameBudgetMs }.count,
            "distributions": [
                "call_ms": distribution(samples.map(\.callMs)),
                "frame_wall_ms": distribution(wall),
                "preprocess_ms": distribution(samples.map(\.preprocessMs)),
                "sort_ms": distribution(samples.map(\.sortMs)),
                "geometry_submit_ms": distribution(samples.map(\.geometrySubmitMs)),
                "gpu_wait_ms": NSNull(), "gpu_complete_ms": NSNull(),
            ],
        ]
    }

    private func distribution(_ values: [Double]) -> [String: Any] {
        let sorted = values.sorted()
        func percentile(_ p: Double) -> Double {
            sorted[max(Int(ceil(p * Double(sorted.count))) - 1, 0)]
        }
        var total = 0.0
        for value in values { total += value }
        return [
            "count": values.count, "mean": total / Double(values.count), "p50": percentile(0.50),
            "p90": percentile(0.90), "p95": percentile(0.95), "p99": percentile(0.99),
            "max": sorted.last ?? 0.0,
        ]
    }

    private func emit(kind: String, object: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) else { return }
        print("BENCHMARK_ARTIFACT \(kind) \(data.base64EncodedString())")
    }

    private func format(_ value: Double) -> String { String(format: "%.3f", value) }
}

private func utc(_ date: Date) -> String {
    ISO8601DateFormatter().string(from: date)
}

private func thermalName(_ state: ProcessInfo.ThermalState) -> String {
    switch state {
    case .nominal: return "nominal"
    case .fair: return "fair"
    case .serious: return "serious"
    case .critical: return "critical"
    @unknown default: return "unknown"
    }
}

private func machineIdentifier() -> String {
    var info = utsname()
    uname(&info)
    return withUnsafePointer(to: &info.machine) {
        $0.withMemoryRebound(to: CChar.self, capacity: 1) { String(cString: $0) }
    }
}

private struct DatasetFacts {
    var sha256: String
    var bytes: UInt64
    var splatCount: Int
    var shDegree: Int
}

private func inspectDataset(path: String) -> DatasetFacts? {
    guard let handle = try? FileHandle(forReadingFrom: URL(fileURLWithPath: path)),
          let headerData = try? handle.read(upToCount: 65_536),
          !headerData.isEmpty else {
        return nil
    }
    try? handle.close()
    let marker = Data("end_header\n".utf8)
    let headerEnd = headerData.range(of: marker)?.upperBound ?? headerData.endIndex
    let header = String(data: headerData[..<headerEnd], encoding: .ascii) ?? ""
    let vertexLine = header.split(separator: "\n").first { $0.hasPrefix("element vertex ") }
    let count = vertexLine.flatMap { Int($0.split(separator: " ").last ?? "") } ?? 0
    guard count > 0 else { return nil }
    let restCount = header.split(separator: "\n").filter { $0.hasPrefix("property float f_rest_") }.count
    let degree = restCount >= 45 ? 3 : (restCount >= 24 ? 2 : (restCount >= 9 ? 1 : 0))
    let bytes = (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? NSNumber)?.uint64Value ?? 0
    guard bytes > 0, let hash = sha256File(path) else { return nil }
    return DatasetFacts(sha256: hash, bytes: bytes, splatCount: count, shDegree: degree)
}

private func sha256File(_ path: String) -> String? {
    guard let handle = try? FileHandle(forReadingFrom: URL(fileURLWithPath: path)) else {
        return nil
    }
    defer { try? handle.close() }
    var hasher = SHA256()
    while let chunk = try? handle.read(upToCount: 1_048_576), !chunk.isEmpty {
        hasher.update(data: chunk)
    }
    return hasher.finalize().map { String(format: "%02x", $0) }.joined()
}

private func traceHash() -> String {
    SHA256.hash(data: Data("gsplat-ios-orbit-trace-v1".utf8)).map { String(format: "%02x", $0) }.joined()
}
