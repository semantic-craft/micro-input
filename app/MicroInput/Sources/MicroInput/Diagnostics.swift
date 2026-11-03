import Foundation
import SwiftUI
import UniformTypeIdentifiers

struct DiagnosticFailure: Codable, Equatable {
    let stage: String
    let message: String
    let timestampMs: UInt64

    enum CodingKeys: String, CodingKey {
        case stage
        case message
        case timestampMs = "timestamp_ms"
    }
}

struct DiagnosticsReport: Codable {
    let schemaVersion: Int
    let generatedAtMs: UInt64
    let application: DiagnosticsApplication
    let environment: DiagnosticsEnvironment
    let connection: DiagnosticsConnection
    let document: DiagnosticsDocument
    let capabilities: DeviceOperationCapabilities
    let inputMonitorReady: Bool
    let statusMessage: String
    let discoveryMessage: String
    let lastFailure: DiagnosticFailure?
    let lastHostAction: DiagnosticsHostAction?

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case generatedAtMs = "generated_at_ms"
        case application
        case environment
        case connection
        case document
        case capabilities
        case inputMonitorReady = "input_monitor_ready"
        case statusMessage = "status_message"
        case discoveryMessage = "discovery_message"
        case lastFailure = "last_failure"
        case lastHostAction = "last_host_action"
    }
}

struct DiagnosticsApplication: Codable, Equatable {
    let version: String
    let build: String
}

struct DiagnosticsEnvironment: Codable, Equatable {
    let osVersion: String
}

struct DiagnosticsConnection: Codable, Equatable {
    let phase: String
    let candidateCount: Int
    let selectedCandidate: Bool
    let stale: Bool

    enum CodingKeys: String, CodingKey {
        case phase
        case candidateCount = "candidate_count"
        case selectedCandidate = "selected_candidate"
        case stale
    }
}

struct DiagnosticsDocument: Codable, Equatable {
    let source: String
    let revision: UInt64
    let readOnly: Bool

    enum CodingKeys: String, CodingKey {
        case source
        case revision
        case readOnly = "read_only"
    }
}

struct DiagnosticsHostAction: Codable, Equatable {
    let actionType: String
    let outcome: String
    let succeeded: Bool

    enum CodingKeys: String, CodingKey {
        case actionType = "action_type"
        case outcome
        case succeeded
    }
}

enum DiagnosticRedactor {
    private static let path = try! NSRegularExpression(pattern: #"(?i)((?:^|\s))/(?:applications|private|system|users|volumes|var|tmp)(?:/(?:[\w.\-()']+(?: [\w.\-()']+)*)(?=/))+/(?:[\w.\-()']+)(?![/\w])"#)
    private static let serial = try! NSRegularExpression(pattern: #"(?i)(serial(?:_number)?\s*[:=]\s*)(?:"[^\"]*"|[^\s,;\"']+)"#)
    private static let jsonPayload = try! NSRegularExpression(pattern: #"\{.*\}|\[.*\]"#)

    static func sanitize(_ value: String) -> String {
        var result = jsonPayload.stringByReplacingMatches(
            in: value,
            range: NSRange(value.startIndex..., in: value),
            withTemplate: "[redacted-payload]"
        )
        result = path.stringByReplacingMatches(
            in: result,
            range: NSRange(result.startIndex..., in: result),
            withTemplate: "$1[path]"
        )
        result = serial.stringByReplacingMatches(
            in: result,
            range: NSRange(result.startIndex..., in: result),
            withTemplate: "$1[redacted]"
        )
        return result
    }
}

struct DiagnosticsExportView: View {
    @ObservedObject var store: Store
    let close: () -> Void
    @State private var reportText = "正在准备脱敏诊断"
    @State private var saveError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                VStack(alignment: .leading) {
                    Text("脱敏诊断").font(.title2.bold())
                    Text("默认排除配置内容、原始回包、序列号、命令文本和文件路径。").font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button("关闭") { close() }.keyboardShortcut(.cancelAction)
            }
            ScrollView {
                Text(reportText)
                    .font(.system(size: 11, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityLabel("脱敏诊断报告")
            }
            .frame(minHeight: 280)
            HStack {
                Button("存储诊断…") { save() }
                if let saveError {
                    Text(saveError).font(.caption).foregroundStyle(.red).accessibilityLabel(saveError)
                }
                Spacer()
                Text("导出前请人工确认接收位置。").font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(24)
        .frame(width: 760)
        .task { await prepareReport() }
    }

    private func prepareReport() async {
        do {
            reportText = try await store.diagnosticJSON()
            if let evidencePath = ProcessInfo.processInfo.environment["MI_DIAGNOSTIC_EVIDENCE_PATH"] {
                try await writeEvidence(reportPath: URL(fileURLWithPath: evidencePath))
            }
        } catch {
            reportText = "诊断报告生成失败；可重试或关闭窗口。"
            saveError = "诊断报告生成失败"
        }
    }

    private func writeEvidence(reportPath: URL) async throws {
        try await store.writeDiagnosticReport(to: reportPath)
        let evidence: [String: Any] = [
            "status": "PASS",
            "failure_stage": store.lastFailure?.stage ?? "",
            "physical_key": "NOT_RUN",
            "lighting_visual": "NOT_RUN",
        ]
        let data = try JSONSerialization.data(withJSONObject: evidence, options: [.sortedKeys])
        let path = reportPath.deletingLastPathComponent().appendingPathComponent("diagnostics.json")
        try data.write(to: path, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: path.path)
    }

    private func save() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "micro-input-diagnostics.json"
        panel.allowedContentTypes = [.json]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try Data(reportText.utf8).write(to: url, options: .atomic)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
            saveError = nil
        } catch {
            saveError = "诊断文件写入失败，请选择可写入的位置"
        }
    }
}
