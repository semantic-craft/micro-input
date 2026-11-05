import Foundation

struct CindyScope: Codable, Equatable, Hashable, Sendable {
    var workspacePath: String

    var isValid: Bool {
        !workspacePath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
        workspacePath.hasPrefix("/")
    }
}

struct CindyTaskAddress: Codable, Equatable, Hashable, Sendable {
    var workspacePath: String
    var sessionId: String

    var id: String {
        "\(workspacePath)#\(sessionId)"
    }

    var isValid: Bool {
        !workspacePath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
        workspacePath.hasPrefix("/") &&
        !sessionId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
        sessionId.count <= 128 &&
        !sessionId.contains("/") &&
        !sessionId.contains("?") &&
        !sessionId.contains("#")
    }

    var deepLink: String {
        "cindy://session/\(sessionId)"
    }
}

struct CindyTaskRow: Codable, Equatable, Identifiable, Sendable {
    static let validPhases: Set<String> = [
        "idle",
        "running",
        "needs-interaction",
        "completed",
        "error",
    ]

    var address: CindyTaskAddress
    var title: String
    var phase: String
    var attention: Bool
    var pinned: Bool

    var id: String { address.id }

    var isValid: Bool {
        address.isValid &&
        !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
        Self.validPhases.contains(phase)
    }

    var workspaceLabel: String {
        let trimmed = address.workspacePath.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        return trimmed.split(separator: "/").last.map(String.init) ?? address.workspacePath
    }
}

struct CindySessionSnapshot: Equatable, Sendable {
    var instanceId: String
    var generation: UInt64
    var ownerEpoch: String
    var scopes: [CindyScope]
    var rows: [CindyTaskRow]
    var selected: CindyTaskAddress?
}

struct CindySelection: Equatable, Sendable {
    var address: CindyTaskAddress
    var instanceId: String
    var generation: UInt64
    var ownerEpoch: String
    var localEpoch: UInt64
}

enum CindyOpenResult: String, Codable, Equatable, Sendable {
    case delivered
    case confirmed
    case unconfirmed
    case stale
    case denied
    case unavailable

    var label: String {
        switch self {
        case .delivered: return "深链已投递"
        case .confirmed: return "导航已确认"
        case .unconfirmed: return "未确认"
        case .stale: return "任务已失效"
        case .denied: return "越权拒绝"
        case .unavailable: return "桥接不可用"
        }
    }
}

struct CindyOpenReply: Equatable, Sendable {
    var instanceId: String
    var generation: UInt64
    var ownerEpoch: String
    var status: CindyOpenResult
    var delivered: CindyTaskAddress?
    var selected: CindyTaskAddress?
    var deepLink: String?
}

enum CindyFailure: Error, Equatable, Sendable {
    case grant
    case denied
    case stale
    case unavailable
    case invalidReply

    var label: String {
        switch self {
        case .grant: return "缺少或无效授权"
        case .denied: return "越权拒绝"
        case .stale: return "实例或账号已换代"
        case .unavailable: return "Cindy 未连接"
        case .invalidReply: return "应答无效"
        }
    }
}

enum CindyConnectionState: Equatable {
    case disabled
    case connecting
    case ready
    case unavailable(CindyFailure)

    var label: String {
        switch self {
        case .disabled:
            return "未启用"
        case .connecting:
            return "连接中"
        case .ready:
            return "已连接"
        case .unavailable(let failure):
            return failure.label
        }
    }
}

protocol CindySessionServing: Sendable {
    func connect() async -> Result<CindySessionSnapshot, CindyFailure>
    func read() async -> Result<CindySessionSnapshot, CindyFailure>
    func open(_ selection: CindySelection) async -> CindyOpenReply
    func close() async
}
