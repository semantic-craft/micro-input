import Foundation

struct AntigravityCapabilities: Codable, Equatable, Sendable {
    let bundleId: String
    let installedVersion: String
    let languageServerCl: String
    let agentapiCommands: [String]
    let sidecarSdkVersion: String
    let hookEvents: [String]
    let supportsSessionCatalog: Bool
    let supportsTargetFocus: Bool
    let supportsStopTurn: Bool
    let supportsApprovalGate: Bool

    static let installed2_29_0 = AntigravityCapabilities(
        bundleId: "com.google.antigravity",
        installedVersion: "2.29.0",
        languageServerCl: "1013060670",
        agentapiCommands: ["get-conversation-metadata", "new-conversation", "send-message"],
        sidecarSdkVersion: "1.0.0",
        hookEvents: ["SessionStart", "PreInvocation", "PostInvocation", "PreToolUse", "PostToolUse", "Stop"],
        supportsSessionCatalog: false,
        supportsTargetFocus: false,
        supportsStopTurn: false,
        supportsApprovalGate: false
    )

    var valid: Bool {
        bundleId == "com.google.antigravity"
            && !installedVersion.isEmpty && installedVersion.utf8.count <= 64
            && !languageServerCl.isEmpty && languageServerCl.utf8.count <= 64
            && agentapiCommands == ["get-conversation-metadata", "new-conversation", "send-message"]
            && !sidecarSdkVersion.isEmpty && sidecarSdkVersion.utf8.count <= 64
            && hookEvents == ["SessionStart", "PreInvocation", "PostInvocation", "PreToolUse", "PostToolUse", "Stop"]
            && !supportsSessionCatalog && !supportsTargetFocus
            && !supportsStopTurn && !supportsApprovalGate
    }
}

struct AntigravitySessionKey: Codable, Hashable, Comparable, Sendable {
    let conversationId: String
    let workspacePath: String

    static func < (lhs: AntigravitySessionKey, rhs: AntigravitySessionKey) -> Bool {
        lhs.conversationId == rhs.conversationId
            ? lhs.workspacePath < rhs.workspacePath
            : lhs.conversationId < rhs.conversationId
    }
}

enum AntigravityHookEventType: String, Codable, CaseIterable, Sendable {
    case sessionStart = "SessionStart"
    case preInvocation = "PreInvocation"
    case postInvocation = "PostInvocation"
    case preToolUse = "PreToolUse"
    case postToolUse = "PostToolUse"
    case stop = "Stop"
}

enum AntigravitySessionPhase: String, Codable, Equatable, Sendable {
    case idle
    case running
    case runningTool
    case backgroundActive
    case completed
    case failed
    case canceled

    var isCompleted: Bool { self == .completed }

    var label: String {
        switch self {
        case .idle: return "已就绪"
        case .running: return "运行中"
        case .runningTool: return "执行工具中"
        case .backgroundActive: return "后台仍在运行"
        case .completed: return "已完成"
        case .failed: return "执行异常"
        case .canceled: return "已取消"
        }
    }
}

struct AntigravityHookEvent: Codable, Equatable, Sendable {
    let conversationId: String
    let workspacePath: String
    let eventType: AntigravityHookEventType
    let modelName: String
    let invocationNum: UInt64?
    let stepIdx: UInt64?
    let executionNum: UInt64?
    let toolName: String?
    let terminationReason: String?
    let hasError: Bool
    let fullyIdle: Bool?
    let eventId: String?

    var sessionKey: AntigravitySessionKey {
        AntigravitySessionKey(conversationId: conversationId, workspacePath: workspacePath)
    }

    var eventKey: String {
        eventKey(prior: nil)
    }

    func eventKey(prior: AntigravitySessionRow?) -> String {
        if let eventId, !eventId.isEmpty {
            return "\(conversationId):\(workspacePath):\(eventId)"
        }
        let inv = invocationNum.map(String.init) ?? prior.map { String($0.invocationNum) } ?? "-1"
        let step = stepIdx.map(String.init) ?? prior.map { String($0.stepIdx) } ?? "-1"
        let exec = executionNum.map(String.init) ?? prior.map { String($0.executionNum) } ?? "-1"
        let tool = toolName ?? ""
        let term = terminationReason ?? ""
        let err = hasError ? "1" : "0"
        let idle = fullyIdle.map { $0 ? "1" : "0" } ?? "-"
        return "\(conversationId):\(workspacePath):\(eventType.rawValue):\(inv):\(step):\(exec):\(tool):\(term):\(err):\(idle)"
    }
}

struct AntigravitySessionRow: Codable, Identifiable, Equatable, Sendable {
    let conversationId: String
    let workspacePath: String
    let modelName: String
    let phase: AntigravitySessionPhase
    let lastEventType: AntigravityHookEventType
    let invocationNum: UInt64
    let stepIdx: UInt64
    let executionNum: UInt64
    let lastToolName: String?
    let terminationReason: String?
    let fullyIdle: Bool
    let transitionCount: UInt64

    var key: AntigravitySessionKey {
        AntigravitySessionKey(conversationId: conversationId, workspacePath: workspacePath)
    }

    var id: String { "\(conversationId)@\(workspacePath)" }

    var valid: Bool {
        AntigravityStatusSnapshot.validID(conversationId, limit: 256)
            && AntigravityStatusSnapshot.validID(workspacePath, limit: 4096)
            && modelName.utf8.count <= 256
            && invocationNum <= 1_000_000_000
            && stepIdx <= 1_000_000_000
            && executionNum <= 1_000_000_000
            && (lastToolName == nil || AntigravityStatusSnapshot.validID(lastToolName!, limit: 256))
            && (terminationReason == nil || AntigravityStatusSnapshot.validID(terminationReason!, limit: 128))
            && transitionCount > 0 && transitionCount <= 1_000_000_000
            && (phase != .completed || (fullyIdle && terminationReason == "NO_TOOL_CALL"))
            && (phase != .backgroundActive || !fullyIdle)
    }

    var statusLabel: String {
        if phase == .runningTool, let tool = lastToolName, !tool.isEmpty {
            return "\(phase.label)（\(tool)）"
        }
        return phase.label
    }

    static func reduce(prior: AntigravitySessionRow?, event: AntigravityHookEvent) -> AntigravitySessionRow {
        let model = event.modelName.isEmpty ? (prior?.modelName ?? "") : event.modelName
        let inv = event.invocationNum ?? prior?.invocationNum ?? 0
        let step = event.stepIdx ?? prior?.stepIdx ?? 0
        let exec = event.executionNum ?? prior?.executionNum ?? 0
        let phase: AntigravitySessionPhase
        let tool: String?
        let reason: String?
        let idle: Bool

        switch event.eventType {
        case .sessionStart:
            phase = .idle
            tool = nil
            reason = nil
            idle = true
        case .preInvocation:
            phase = .running
            tool = prior?.lastToolName
            reason = nil
            idle = false
        case .preToolUse:
            phase = .runningTool
            tool = (event.toolName?.isEmpty == false) ? event.toolName : prior?.lastToolName
            reason = nil
            idle = false
        case .postToolUse:
            phase = .running
            tool = (event.toolName?.isEmpty == false) ? event.toolName : prior?.lastToolName
            reason = nil
            idle = false
        case .postInvocation:
            phase = event.hasError ? .failed : .running
            tool = prior?.lastToolName
            reason = event.hasError ? "ERROR" : nil
            idle = false
        case .stop:
            tool = prior?.lastToolName
            reason = (event.terminationReason?.isEmpty == false) ? event.terminationReason : nil
            if reason == "ERROR" || event.hasError {
                phase = .failed
                idle = event.fullyIdle ?? false
            } else if reason == "USER_CANCELED" {
                phase = .canceled
                idle = event.fullyIdle ?? false
            } else if event.fullyIdle == true && reason == "NO_TOOL_CALL" {
                phase = .completed
                idle = true
            } else {
                phase = .backgroundActive
                idle = false
            }
        }

        return AntigravitySessionRow(
            conversationId: event.conversationId,
            workspacePath: event.workspacePath,
            modelName: model,
            phase: phase,
            lastEventType: event.eventType,
            invocationNum: inv,
            stepIdx: step,
            executionNum: exec,
            lastToolName: tool,
            terminationReason: reason,
            fullyIdle: idle,
            transitionCount: (prior?.transitionCount ?? 0) + 1
        )
    }
}

struct AntigravityEventChange: Codable, Equatable, Sendable {
    let sequence: UInt64
    let eventKey: String
    let row: AntigravitySessionRow
}

struct AntigravityStatusSnapshot: Equatable, Sendable {
    let instanceId: String
    let generation: UInt64
    let allowedWorkspaces: [String]
    let capabilities: AntigravityCapabilities
    let cursor: UInt64
    let rows: [AntigravitySessionRow]
    let changes: [AntigravityEventChange]
    let transcriptReads: UInt64

    static func validID(_ value: String, limit: Int = 4096) -> Bool {
        !value.isEmpty && value.utf8.count <= limit
    }

    func validate(after previous: UInt64? = nil) throws {
        let workspaceScope = Set(allowedWorkspaces)
        let rowKeys = Set(rows.map(\.key))
        guard Self.validID(instanceId, limit: 256), generation > 0,
              !workspaceScope.isEmpty, workspaceScope.count <= 4096,
              workspaceScope.count == allowedWorkspaces.count,
              allowedWorkspaces.allSatisfy({ Self.validID($0, limit: 4096) }),
              capabilities.valid,
              transcriptReads == 0,
              rows.count == rowKeys.count,
              rows.allSatisfy({ $0.valid && workspaceScope.contains($0.workspacePath) }),
              changes.count <= 1024,
              changes.allSatisfy({
                  Self.validID($0.eventKey, limit: 1024)
                      && $0.row.valid
                      && workspaceScope.contains($0.row.workspacePath)
              }) else {
            throw AntigravityFailure.invalidReply
        }
        if let previous {
            guard cursor >= previous, cursor - previous == UInt64(changes.count) else {
                throw AntigravityFailure.gap
            }
            for (offset, change) in changes.enumerated() {
                guard change.sequence == previous + UInt64(offset) + 1 else {
                    throw AntigravityFailure.gap
                }
            }
        } else if !changes.isEmpty {
            throw AntigravityFailure.invalidReply
        }
    }
}

enum AntigravityFailure: Error, Equatable, Sendable {
    case grant, denied, stale, unavailable, invalidReply, gap

    var label: String {
        switch self {
        case .grant: return "请先配置 Antigravity 状态桥授权"
        case .denied: return "Antigravity 状态桥拒绝此请求"
        case .stale: return "Antigravity 事件流已断开或实例已换代"
        case .unavailable: return "Antigravity 状态桥连接中断"
        case .invalidReply: return "Antigravity 状态回执无法核对"
        case .gap: return "Antigravity 事件日志存在缺口，请重连"
        }
    }
}

enum AntigravityConnectionState: Equatable {
    case disabled, connecting, ready, unavailable(AntigravityFailure)

    var label: String {
        switch self {
        case .disabled: return "Antigravity 状态连接未开启"
        case .connecting: return "正在连接 Antigravity 状态桥"
        case .ready: return "Antigravity 状态连接已就绪"
        case .unavailable(let reason): return reason.label
        }
    }
}

protocol AntigravityStatusServing: Sendable {
    func connect() async throws -> AntigravityStatusSnapshot
    func read() async throws -> AntigravityStatusSnapshot
    func close() async
}
