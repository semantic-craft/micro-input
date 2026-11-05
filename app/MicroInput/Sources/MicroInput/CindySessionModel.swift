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

enum CindyInteractionType: String, Codable, Equatable, Hashable, Sendable {
    case permission
    case question
    case plan
}

enum CindyInteractionDecision: String, Codable, Equatable, Hashable, Sendable {
    case approve
    case allowForSession
    case reject
    case accept
    case cancel
}

enum CindyDeliveryMode: String, Codable, Equatable, Hashable, Sendable {
    case auto
    case steer
    case queue
}

struct CindyPendingInteraction: Codable, Equatable, Hashable, Sendable {
    var interactionId: String
    var interactionType: CindyInteractionType
    var toolName: String?
    var options: [String]?

    init(
        interactionId: String,
        interactionType: CindyInteractionType,
        toolName: String? = nil,
        options: [String]? = nil
    ) {
        self.interactionId = interactionId
        self.interactionType = interactionType
        self.toolName = toolName
        self.options = options
    }

    var isValid: Bool {
        !interactionId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
        interactionId.count <= 128 &&
        !interactionId.contains("\0")
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
    var runId: String?
    var unread: Bool?
    var eventWatermark: UInt64?
    var queuedCount: Int
    var pendingInteraction: CindyPendingInteraction?

    init(
        address: CindyTaskAddress,
        title: String,
        phase: String,
        attention: Bool,
        pinned: Bool,
        runId: String? = nil,
        unread: Bool? = nil,
        eventWatermark: UInt64? = nil,
        queuedCount: Int = 0,
        pendingInteraction: CindyPendingInteraction? = nil
    ) {
        self.address = address
        self.title = title
        self.phase = phase
        self.attention = attention
        self.pinned = pinned
        self.runId = runId
        self.unread = unread
        self.eventWatermark = eventWatermark
        self.queuedCount = queuedCount
        self.pendingInteraction = pendingInteraction
    }

    private enum CodingKeys: String, CodingKey {
        case address
        case title
        case phase
        case attention
        case pinned
        case runId
        case unread
        case eventWatermark
        case queuedCount
        case pendingInteraction
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        address = try container.decode(CindyTaskAddress.self, forKey: .address)
        title = try container.decode(String.self, forKey: .title)
        phase = try container.decode(String.self, forKey: .phase)
        attention = try container.decode(Bool.self, forKey: .attention)
        pinned = try container.decode(Bool.self, forKey: .pinned)
        runId = try container.decodeIfPresent(String.self, forKey: .runId)
        unread = try container.decodeIfPresent(Bool.self, forKey: .unread)
        eventWatermark = try container.decodeIfPresent(UInt64.self, forKey: .eventWatermark)
        queuedCount = try container.decodeIfPresent(Int.self, forKey: .queuedCount) ?? 0
        pendingInteraction = try container.decodeIfPresent(CindyPendingInteraction.self, forKey: .pendingInteraction)
    }

    var id: String { address.id }

    var isValid: Bool {
        address.isValid &&
        !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
        Self.validPhases.contains(phase) &&
        queuedCount >= 0 &&
        (pendingInteraction?.isValid ?? true)
    }

    var workspaceLabel: String {
        let trimmed = address.workspacePath.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        return trimmed.split(separator: "/").last.map(String.init) ?? address.workspacePath
    }

    /// True only when backed by an explicit interaction, explicit unread fact, or error attention.
    /// Completed sessions with `unread == false` or `unread == nil` are never fabricated as unread.
    var isEligibleForAttentionRound: Bool {
        if phase == "needs-interaction" || pendingInteraction != nil {
            return true
        }
        if unread == true {
            return true
        }
        if phase == "error" && attention {
            return true
        }
        return false
    }

    var attentionPriorityRank: Int {
        if phase == "needs-interaction" || pendingInteraction != nil {
            return 0
        }
        if phase == "error" && (unread == true || attention) {
            return 1
        }
        if unread == true {
            return 2
        }
        return 3
    }

    var roundWatermarkSignature: String {
        "\(eventWatermark ?? 0):\(pendingInteraction?.interactionId ?? ""):\(unread == true ? "1" : "0"):\(phase)"
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

struct CindyCreateIntent: Equatable, Sendable {
    var commandId: String
    var scope: CindyScope
    var text: String?
    var instanceId: String
    var generation: UInt64
    var ownerEpoch: String
    var localEpoch: UInt64
}

struct CindySendIntent: Equatable, Sendable {
    var commandId: String
    var selection: CindySelection
    var text: String
    var deliveryMode: CindyDeliveryMode
}

struct CindyStopIntent: Equatable, Sendable {
    var commandId: String
    var selection: CindySelection
    var expectedRunId: String
}

struct CindyResolveIntent: Equatable, Sendable {
    var commandId: String
    var selection: CindySelection
    var interactionId: String
    var interactionType: CindyInteractionType
    var decision: CindyInteractionDecision
    var optionId: String?
}

struct CindyQueryIntent: Equatable, Sendable {
    var commandId: String
    var scope: CindyScope
    var target: CindyTaskAddress?
    var instanceId: String
    var generation: UInt64
    var ownerEpoch: String
    var localEpoch: UInt64
}

enum CindyQueuedCommand: Codable, Equatable, Hashable, Sendable {
    case create(scope: CindyScope, text: String?, commandId: String?)
    case send(
        address: CindyTaskAddress,
        text: String,
        deliveryMode: CindyDeliveryMode,
        commandId: String?
    )
    case stop(address: CindyTaskAddress, expectedRunId: String, commandId: String?)
    case resolve(
        address: CindyTaskAddress,
        interactionId: String,
        interactionType: CindyInteractionType,
        decision: CindyInteractionDecision,
        optionId: String?,
        commandId: String?
    )
    case query(scope: CindyScope, target: CindyTaskAddress?, commandId: String)
    case nextAttention

    var label: String {
        switch self {
        case .create(let scope, _, _):
            return "Cindy 新建任务 \(scope.workspacePath)"
        case .send(let address, _, let mode, _):
            return "Cindy \(mode.rawValue) \(address.sessionId)"
        case .stop(let address, let runId, _):
            return "Cindy 停止 \(address.sessionId)#\(runId)"
        case .resolve(let address, let id, let type, let decision, _, _):
            return "Cindy 响应 \(address.sessionId)#\(type.rawValue):\(id)->\(decision.rawValue)"
        case .query(_, _, let commandId):
            return "Cindy 查询 \(commandId)"
        case .nextAttention:
            return "Cindy 下一待办任务"
        }
    }
}

enum CindyCommandResult: String, Codable, Equatable, Sendable {
    case confirmed
    case delivered
    case started
    case steered
    case queued
    case stopped
    case resolved
    case noop
    case interactionTypeMismatch
    case unconfirmed
    case stale
    case denied
    case unavailable
    case unknown

    var label: String {
        switch self {
        case .confirmed: return "已确认"
        case .delivered: return "深链已投递"
        case .started: return "已启动轮次"
        case .steered: return "已中途引导"
        case .queued: return "已排队"
        case .stopped: return "已停止"
        case .resolved: return "已响应请求"
        case .noop: return "幂等无操作"
        case .interactionTypeMismatch: return "请求类型不匹配"
        case .unconfirmed: return "未确认"
        case .stale: return "任务已失效"
        case .denied: return "越权拒绝"
        case .unavailable: return "桥接不可用"
        case .unknown: return "结果待核对"
        }
    }
}

struct CindyCommandReply: Equatable, Sendable {
    var commandId: String
    var instanceId: String
    var generation: UInt64
    var ownerEpoch: String
    var status: CindyCommandResult
    var duplicate: Bool = false
    var delivered: CindyTaskAddress? = nil
    var selected: CindyTaskAddress? = nil
    var deepLink: String? = nil
    var delivery: CindyDeliveryMode? = nil
    var runId: String? = nil
    var queuedCount: Int = 0
    var interactionId: String? = nil
    var interactionType: CindyInteractionType? = nil
    var reasonCode: String? = nil
}

enum CindyFailure: Error, Equatable, Sendable {
    case grant
    case denied
    case stale
    case unavailable
    case invalidReply
    case transportTimeout

    var label: String {
        switch self {
        case .grant: return "缺少或无效授权"
        case .denied: return "越权拒绝"
        case .stale: return "实例或账号已换代"
        case .unavailable: return "Cindy 未连接"
        case .invalidReply: return "应答无效"
        case .transportTimeout: return "请求超时待核对"
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
    func create(_ intent: CindyCreateIntent) async -> CindyCommandReply
    func send(_ intent: CindySendIntent) async -> CindyCommandReply
    func stop(_ intent: CindyStopIntent) async -> CindyCommandReply
    func resolve(_ intent: CindyResolveIntent) async -> CindyCommandReply
    func query(_ intent: CindyQueryIntent) async -> CindyCommandReply
    func close() async
}

extension CindySessionServing {
    func create(_ intent: CindyCreateIntent) async -> CindyCommandReply {
        CindyCommandReply(
            commandId: intent.commandId,
            instanceId: intent.instanceId,
            generation: intent.generation,
            ownerEpoch: intent.ownerEpoch,
            status: .unavailable,
            reasonCode: "bridge.createUnsupported"
        )
    }

    func send(_ intent: CindySendIntent) async -> CindyCommandReply {
        CindyCommandReply(
            commandId: intent.commandId,
            instanceId: intent.selection.instanceId,
            generation: intent.selection.generation,
            ownerEpoch: intent.selection.ownerEpoch,
            status: .unavailable,
            reasonCode: "bridge.sendUnsupported"
        )
    }

    func stop(_ intent: CindyStopIntent) async -> CindyCommandReply {
        CindyCommandReply(
            commandId: intent.commandId,
            instanceId: intent.selection.instanceId,
            generation: intent.selection.generation,
            ownerEpoch: intent.selection.ownerEpoch,
            status: .unavailable,
            reasonCode: "bridge.stopUnsupported"
        )
    }

    func resolve(_ intent: CindyResolveIntent) async -> CindyCommandReply {
        CindyCommandReply(
            commandId: intent.commandId,
            instanceId: intent.selection.instanceId,
            generation: intent.selection.generation,
            ownerEpoch: intent.selection.ownerEpoch,
            status: .unavailable,
            reasonCode: "bridge.resolveUnsupported"
        )
    }

    func query(_ intent: CindyQueryIntent) async -> CindyCommandReply {
        CindyCommandReply(
            commandId: intent.commandId,
            instanceId: intent.instanceId,
            generation: intent.generation,
            ownerEpoch: intent.ownerEpoch,
            status: .unavailable,
            reasonCode: "bridge.queryUnsupported"
        )
    }
}
