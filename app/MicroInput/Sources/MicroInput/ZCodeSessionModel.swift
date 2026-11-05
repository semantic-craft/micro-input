import Foundation

struct ZCodeScope: Codable, Hashable, Sendable {
    let workspacePath: String
    var workspaceIdentity: String? = nil
    var sourceServerId: String? = nil
    var remoteSessionId: String? = nil

    var valid: Bool {
        workspacePath.hasPrefix("/") && workspacePath.utf8.count <= 4096
            && [workspaceIdentity, sourceServerId, remoteSessionId].allSatisfy { $0 == nil || ($0?.isEmpty == false && ($0?.utf8.count ?? 0) <= 4096) }
            && (remoteSessionId == nil || workspaceIdentity != nil)
    }
}

struct ZCodeTaskAddress: Codable, Hashable, Sendable {
    let workspacePath: String
    var workspaceIdentity: String? = nil
    var sourceServerId: String? = nil
    var remoteSessionId: String? = nil
    let taskId: String
    var scope: ZCodeScope {
        ZCodeScope(workspacePath: workspacePath, workspaceIdentity: workspaceIdentity,
                   sourceServerId: sourceServerId, remoteSessionId: remoteSessionId)
    }
    var valid: Bool { scope.valid && !taskId.isEmpty && taskId.utf8.count <= 4096 }
}

enum ZCodeInteractionType: String, Codable, Equatable, Sendable {
    case permission, question, plan
}

enum ZCodeInteractionDecision: String, Codable, Equatable, Sendable {
    case approve, allowAlways, reject, accept, cancel
}

struct ZCodePendingInteraction: Codable, Equatable, Sendable {
    let interactionId: String
    let interactionType: ZCodeInteractionType
    var toolName: String? = nil
    var options: [String]? = nil

    var valid: Bool {
        !interactionId.isEmpty && interactionId.utf8.count <= 4096
            && (toolName == nil || (toolName?.isEmpty == false && (toolName?.utf8.count ?? 0) <= 4096))
            && (options == nil || ((options?.count ?? 0) <= 64 && (options?.allSatisfy { !$0.isEmpty && $0.utf8.count <= 4096 } ?? true)))
    }
}

struct ZCodeModelOption: Codable, Equatable, Sendable {
    let provider: String
    let model: String
    var defaultThought: String? = nil
    let supportedThoughts: [String]

    var valid: Bool {
        !provider.isEmpty && provider.utf8.count <= 4096
            && !model.isEmpty && model.utf8.count <= 4096
            && (defaultThought == nil || (defaultThought?.isEmpty == false && (defaultThought?.utf8.count ?? 0) <= 4096))
            && supportedThoughts.count <= 64
            && supportedThoughts.allSatisfy { !$0.isEmpty && $0.utf8.count <= 4096 }
    }
}

struct ZCodeModelConfig: Codable, Equatable, Sendable {
    let provider: String
    let model: String
    let thought: String
    let supportedThoughts: [String]
    var availableModels: [ZCodeModelOption]? = nil

    var valid: Bool {
        !provider.isEmpty && provider.utf8.count <= 4096
            && !model.isEmpty && model.utf8.count <= 4096
            && !thought.isEmpty && thought.utf8.count <= 4096
            && supportedThoughts.count <= 64
            && supportedThoughts.allSatisfy { !$0.isEmpty && $0.utf8.count <= 4096 }
            && (availableModels == nil || ((availableModels?.count ?? 0) <= 128 && (availableModels?.allSatisfy(\.valid) ?? true)))
    }

    func supportsThought(_ requestedThought: String, provider targetProvider: String, model targetModel: String) -> Bool {
        if let available = availableModels,
           let matched = available.first(where: { $0.provider == targetProvider && $0.model == targetModel }) {
            return matched.supportedThoughts.contains(requestedThought)
        }
        if provider == targetProvider && model == targetModel {
            return supportedThoughts.contains(requestedThought)
        }
        return false
    }
}

struct ZCodeTaskRow: Codable, Identifiable, Equatable, Sendable {
    let address: ZCodeTaskAddress
    let title: String
    let liveStatus: String
    var executionId: String? = nil
    var pendingInteraction: ZCodePendingInteraction? = nil
    var modelConfig: ZCodeModelConfig? = nil
    var id: ZCodeTaskAddress { address }
}

struct ZCodeSessionSnapshot: Equatable, Sendable {
    let instanceId: String
    let generation: UInt64
    let scopes: [ZCodeScope]
    let rows: [ZCodeTaskRow]
    let selected: ZCodeTaskAddress?
    func validate() throws {
        guard !instanceId.isEmpty, instanceId.utf8.count <= 256, generation > 0,
              !scopes.isEmpty, scopes.count <= 64, scopes.allSatisfy(\.valid),
              Set(scopes).count == scopes.count, rows.count <= 4096,
              Set(rows.map(\.address)).count == rows.count,
              rows.allSatisfy({ $0.address.valid && scopes.contains($0.address.scope)
                  && $0.title.utf8.count <= 4096
                  && ["idle", "running", "waiting", "completed", "error"].contains($0.liveStatus)
                  && ($0.executionId == nil || ($0.executionId?.isEmpty == false && ($0.executionId?.utf8.count ?? 0) <= 4096))
                  && ($0.pendingInteraction?.valid ?? true)
                  && ($0.modelConfig?.valid ?? true) }),
              selected == nil || rows.contains(where: { $0.address == selected }) else { throw ZCodeFailure.invalidReply }
    }
}

struct ZCodeSelection: Equatable, Sendable {
    let address: ZCodeTaskAddress
    let instanceId: String
    let generation: UInt64
    let localEpoch: UInt64
}

enum ZCodeDelivery: String, Codable, Equatable, Sendable {
    case startNow, queue, guide
}

struct ZCodeCreateIntent: Equatable, Sendable {
    let scope: ZCodeScope
    let commandId: String
    let clientId: String
    var sessionId: String? = nil
    var firstInputText: String? = nil
    let instanceId: String
    let generation: UInt64
    let localEpoch: UInt64
}

struct ZCodeSendIntent: Equatable, Sendable {
    let selection: ZCodeSelection
    let commandId: String
    let clientId: String
    let text: String
    var requestedDelivery: ZCodeDelivery? = nil
}

struct ZCodeStopIntent: Equatable, Sendable {
    let selection: ZCodeSelection
    let commandId: String
    let clientId: String
    let expectedExecutionId: String
}

struct ZCodeResolveIntent: Equatable, Sendable {
    let selection: ZCodeSelection
    let commandId: String
    let clientId: String
    let interactionId: String
    let interactionType: ZCodeInteractionType
    let decision: ZCodeInteractionDecision
    var optionId: String? = nil
}

struct ZCodeSwitchModelIntent: Equatable, Sendable {
    let selection: ZCodeSelection
    let commandId: String
    let clientId: String
    let provider: String
    let model: String
    let thought: String
}

struct ZCodeQueryIntent: Equatable, Sendable {
    let scope: ZCodeScope
    var target: ZCodeTaskAddress? = nil
    let commandId: String
    let instanceId: String
    let generation: UInt64
    let localEpoch: UInt64
}

enum ZCodeQueuedCommand: Equatable, Sendable {
    case create(ZCodeCreateIntent)
    case send(ZCodeSendIntent)
    case stop(ZCodeStopIntent)
    case resolve(ZCodeResolveIntent)
    case switchModel(ZCodeSwitchModelIntent)
    case query(ZCodeQueryIntent)
}

struct ZCodeOpenReply: Sendable {
    let instanceId: String
    let generation: UInt64
    let status: ZCodeOpenResult
    let selected: ZCodeTaskAddress?
}

enum ZCodeOpenResult: String, Codable, Sendable {
    case confirmed, unconfirmed, stale, denied, unavailable
    var label: String {
        switch self {
        case .confirmed: return "已确认打开指定 ZCode 会话"
        case .unconfirmed: return "ZCode 未确认选中目标"
        case .stale: return "目标或连接已变化，打开未确认"
        case .denied: return "ZCode 未授权此操作"
        case .unavailable: return "ZCode 桥不可用，未重试打开"
        }
    }
}

enum ZCodeCommandResult: String, Codable, Equatable, Sendable {
    case confirmed
    case started
    case queued
    case steered
    case stopped
    case resolved
    case modelSwitched
    case noop
    case unknown
    case unconfirmed
    case interactionTypeMismatch
    case unsupportedThought
    case stale
    case denied
    case unavailable

    var label: String {
        switch self {
        case .confirmed: return "已确认新建并选中 ZCode 会话"
        case .started: return "已向选中 ZCode 会话发送并启动执行"
        case .queued: return "目标 ZCode 会话运行中，输入已进入等待队列（非实时 steering）"
        case .steered: return "已向运行中的 ZCode 会话确认实时 steering（guide）"
        case .stopped: return "已按指定 execution ID 停止 ZCode 执行"
        case .resolved: return "已按指定 interactionId 和类型处理 ZCode 交互请求"
        case .modelSwitched: return "已通过 switchModelConfig 切换并回读确认模型与思考档位"
        case .noop: return "目标请求或执行已变更或结束，迟到请求未影响当前状态"
        case .unknown: return "命令超时且结果未知，保持原 commandId 等待对账，不换 ID 重放"
        case .unconfirmed: return "ZCode 未确认当前命令目标或回读状态不一致"
        case .interactionTypeMismatch: return "交互类型不匹配（问题不得当权限批准），未发送"
        case .unsupportedThought: return "目标模型不支持该思考档位，未发送 switchModelConfig"
        case .stale: return "目标或连接已变化，命令未执行"
        case .denied: return "ZCode 未授权此操作"
        case .unavailable: return "ZCode 桥不可用，未换 ID 重放"
        }
    }
}

struct ZCodeCommandReply: Equatable, Sendable {
    let instanceId: String
    let generation: UInt64
    let status: ZCodeCommandResult
    let selected: ZCodeTaskAddress?
    var commandId: String? = nil
    var duplicate: Bool = false
    var delivery: ZCodeDelivery? = nil
    var executionId: String? = nil
    var interactionId: String? = nil
    var interactionType: ZCodeInteractionType? = nil
    var modelConfig: ZCodeModelConfig? = nil
    var reasonCode: String? = nil
}

enum ZCodeFailure: Error, Equatable, Sendable {
    case grant, denied, stale, unavailable, invalidReply, transportTimeout
    var label: String {
        switch self {
        case .grant: return "请先在 ZCode 授权工作区连接"
        case .denied: return "ZCode 未授权此工作区"
        case .stale: return "ZCode 实例已变化，请重新连接"
        case .unavailable: return "ZCode 桥连接中断"
        case .invalidReply: return "ZCode 返回的任务身份无法核对"
        case .transportTimeout: return "ZCode 命令响应超时，等待原 ID 对账"
        }
    }
}

enum ZCodeConnectionState: Equatable {
    case disabled, connecting, ready, unavailable(ZCodeFailure)
    var label: String {
        switch self {
        case .disabled: return "ZCode 会话连接未开启"
        case .connecting: return "正在连接 ZCode"
        case .ready: return "ZCode 会话连接已就绪"
        case .unavailable(let reason): return reason.label
        }
    }
}

protocol ZCodeSessionServing: Sendable {
    func connect() async throws -> ZCodeSessionSnapshot
    func read() async throws -> ZCodeSessionSnapshot
    func open(_ selection: ZCodeSelection) async throws -> ZCodeOpenReply
    func create(_ intent: ZCodeCreateIntent) async throws -> ZCodeCommandReply
    func send(_ intent: ZCodeSendIntent) async throws -> ZCodeCommandReply
    func stop(_ intent: ZCodeStopIntent) async throws -> ZCodeCommandReply
    func resolve(_ intent: ZCodeResolveIntent) async throws -> ZCodeCommandReply
    func switchModel(_ intent: ZCodeSwitchModelIntent) async throws -> ZCodeCommandReply
    func query(_ intent: ZCodeQueryIntent) async throws -> ZCodeCommandReply
    func close() async
}
