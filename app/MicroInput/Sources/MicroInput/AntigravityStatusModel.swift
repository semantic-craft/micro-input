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
    let schemaVerified: Bool

    init(
        bundleId: String,
        installedVersion: String,
        languageServerCl: String,
        agentapiCommands: [String],
        sidecarSdkVersion: String,
        hookEvents: [String],
        supportsSessionCatalog: Bool,
        supportsTargetFocus: Bool,
        supportsStopTurn: Bool,
        supportsApprovalGate: Bool,
        schemaVerified: Bool = true
    ) {
        self.bundleId = bundleId
        self.installedVersion = installedVersion
        self.languageServerCl = languageServerCl
        self.agentapiCommands = agentapiCommands
        self.sidecarSdkVersion = sidecarSdkVersion
        self.hookEvents = hookEvents
        self.supportsSessionCatalog = supportsSessionCatalog
        self.supportsTargetFocus = supportsTargetFocus
        self.supportsStopTurn = supportsStopTurn
        self.supportsApprovalGate = supportsApprovalGate
        self.schemaVerified = schemaVerified
    }

    enum CodingKeys: String, CodingKey {
        case bundleId
        case installedVersion
        case languageServerCl
        case agentapiCommands
        case sidecarSdkVersion
        case hookEvents
        case supportsSessionCatalog
        case supportsTargetFocus
        case supportsStopTurn
        case supportsApprovalGate
        case schemaVerified
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        bundleId = try c.decode(String.self, forKey: .bundleId)
        installedVersion = try c.decode(String.self, forKey: .installedVersion)
        languageServerCl = try c.decode(String.self, forKey: .languageServerCl)
        agentapiCommands = try c.decode([String].self, forKey: .agentapiCommands)
        sidecarSdkVersion = try c.decode(String.self, forKey: .sidecarSdkVersion)
        hookEvents = try c.decode([String].self, forKey: .hookEvents)
        supportsSessionCatalog = try c.decode(Bool.self, forKey: .supportsSessionCatalog)
        supportsTargetFocus = try c.decode(Bool.self, forKey: .supportsTargetFocus)
        supportsStopTurn = try c.decode(Bool.self, forKey: .supportsStopTurn)
        supportsApprovalGate = try c.decode(Bool.self, forKey: .supportsApprovalGate)
        schemaVerified = try c.decodeIfPresent(Bool.self, forKey: .schemaVerified) ?? true
    }

    static let installed2_29_0 = AntigravityCapabilities(
        bundleId: "com.google.antigravity",
        installedVersion: "2.29.0",
        languageServerCl: "1013060670",
        agentapiCommands: ["get-conversation-metadata", "new-conversation", "send-message"],
        sidecarSdkVersion: "1.0.0",
        hookEvents: ["SessionStart", "PreInvocation", "PostInvocation", "PreToolUse", "PostToolUse", "Stop"],
        supportsSessionCatalog: true,
        supportsTargetFocus: true,
        supportsStopTurn: false,
        supportsApprovalGate: false,
        schemaVerified: true
    )

    var valid: Bool {
        bundleId == "com.google.antigravity"
            && !installedVersion.isEmpty && installedVersion.utf8.count <= 64
            && !languageServerCl.isEmpty && languageServerCl.utf8.count <= 64
            && agentapiCommands == ["get-conversation-metadata", "new-conversation", "send-message"]
            && !sidecarSdkVersion.isEmpty && sidecarSdkVersion.utf8.count <= 64
            && hookEvents == ["SessionStart", "PreInvocation", "PostInvocation", "PreToolUse", "PostToolUse", "Stop"]
            && !supportsStopTurn && !supportsApprovalGate
            && (!supportsSessionCatalog && !supportsTargetFocus || schemaVerified)
    }

    var supportsCatalogNavigation: Bool {
        valid && schemaVerified && supportsSessionCatalog && supportsTargetFocus
    }

    var supportsCreateConversation: Bool {
        valid && schemaVerified && agentapiCommands.contains("new-conversation")
    }

    var supportsSendMessage: Bool {
        valid && schemaVerified && agentapiCommands.contains("send-message")
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
    let title: String?
    let projectId: String?

    init(
        conversationId: String,
        workspacePath: String,
        eventType: AntigravityHookEventType,
        modelName: String,
        invocationNum: UInt64?,
        stepIdx: UInt64?,
        executionNum: UInt64?,
        toolName: String?,
        terminationReason: String?,
        hasError: Bool,
        fullyIdle: Bool?,
        eventId: String?,
        title: String? = nil,
        projectId: String? = nil
    ) {
        self.conversationId = conversationId
        self.workspacePath = workspacePath
        self.eventType = eventType
        self.modelName = modelName
        self.invocationNum = invocationNum
        self.stepIdx = stepIdx
        self.executionNum = executionNum
        self.toolName = toolName
        self.terminationReason = terminationReason
        self.hasError = hasError
        self.fullyIdle = fullyIdle
        self.eventId = eventId
        self.title = title
        self.projectId = projectId
    }

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
    let title: String
    let projectId: String
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

    init(
        conversationId: String,
        workspacePath: String,
        title: String? = nil,
        projectId: String = "",
        modelName: String,
        phase: AntigravitySessionPhase,
        lastEventType: AntigravityHookEventType,
        invocationNum: UInt64,
        stepIdx: UInt64,
        executionNum: UInt64,
        lastToolName: String?,
        terminationReason: String?,
        fullyIdle: Bool,
        transitionCount: UInt64
    ) {
        self.conversationId = conversationId
        self.workspacePath = workspacePath
        self.title = (title?.isEmpty == false) ? title! : conversationId
        self.projectId = projectId
        self.modelName = modelName
        self.phase = phase
        self.lastEventType = lastEventType
        self.invocationNum = invocationNum
        self.stepIdx = stepIdx
        self.executionNum = executionNum
        self.lastToolName = lastToolName
        self.terminationReason = terminationReason
        self.fullyIdle = fullyIdle
        self.transitionCount = transitionCount
    }

    enum CodingKeys: String, CodingKey {
        case conversationId
        case workspacePath
        case title
        case projectId
        case modelName
        case phase
        case lastEventType
        case invocationNum
        case stepIdx
        case executionNum
        case lastToolName
        case terminationReason
        case fullyIdle
        case transitionCount
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let conv = try c.decode(String.self, forKey: .conversationId)
        conversationId = conv
        workspacePath = try c.decode(String.self, forKey: .workspacePath)
        let decodedTitle = try c.decodeIfPresent(String.self, forKey: .title)
        title = (decodedTitle?.isEmpty == false) ? decodedTitle! : conv
        projectId = try c.decodeIfPresent(String.self, forKey: .projectId) ?? ""
        modelName = try c.decode(String.self, forKey: .modelName)
        phase = try c.decode(AntigravitySessionPhase.self, forKey: .phase)
        lastEventType = try c.decode(AntigravityHookEventType.self, forKey: .lastEventType)
        invocationNum = try c.decode(UInt64.self, forKey: .invocationNum)
        stepIdx = try c.decode(UInt64.self, forKey: .stepIdx)
        executionNum = try c.decode(UInt64.self, forKey: .executionNum)
        lastToolName = try c.decodeIfPresent(String.self, forKey: .lastToolName)
        terminationReason = try c.decodeIfPresent(String.self, forKey: .terminationReason)
        fullyIdle = try c.decode(Bool.self, forKey: .fullyIdle)
        transitionCount = try c.decode(UInt64.self, forKey: .transitionCount)
    }

    var key: AntigravitySessionKey {
        AntigravitySessionKey(conversationId: conversationId, workspacePath: workspacePath)
    }

    var id: String { "\(conversationId)@\(workspacePath)" }

    var valid: Bool {
        AntigravityStatusSnapshot.validID(conversationId, limit: 256)
            && AntigravityStatusSnapshot.validID(workspacePath, limit: 4096)
            && AntigravityStatusSnapshot.validID(title, limit: 512)
            && projectId.utf8.count <= 256
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
        let title = (event.title?.isEmpty == false) ? event.title! : (prior?.title ?? event.conversationId)
        let projectId = (event.projectId?.isEmpty == false) ? event.projectId! : (prior?.projectId ?? "")
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
            title: title,
            projectId: projectId,
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
    let selected: AntigravitySessionKey?
    let transcriptReads: UInt64
    let tokenReads: UInt64

    init(
        instanceId: String,
        generation: UInt64,
        allowedWorkspaces: [String],
        capabilities: AntigravityCapabilities,
        cursor: UInt64,
        rows: [AntigravitySessionRow],
        changes: [AntigravityEventChange],
        selected: AntigravitySessionKey? = nil,
        transcriptReads: UInt64,
        tokenReads: UInt64 = 0
    ) {
        self.instanceId = instanceId
        self.generation = generation
        self.allowedWorkspaces = allowedWorkspaces
        self.capabilities = capabilities
        self.cursor = cursor
        self.rows = rows
        self.changes = changes
        self.selected = selected
        self.transcriptReads = transcriptReads
        self.tokenReads = tokenReads
    }

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
              tokenReads == 0,
              rows.count == rowKeys.count,
              rows.allSatisfy({ $0.valid && workspaceScope.contains($0.workspacePath) }),
              selected == nil || rowKeys.contains(selected!),
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

struct AntigravitySelection: Equatable, Sendable {
    let key: AntigravitySessionKey
    let instanceId: String
    let generation: UInt64
    let localEpoch: UInt64
}

struct AntigravityOpenReply: Equatable, Sendable {
    let instanceId: String
    let generation: UInt64
    let status: String
    let selected: AntigravitySessionKey?
    let transcriptReads: UInt64
    let tokenReads: UInt64

    init(
        instanceId: String,
        generation: UInt64,
        status: String = "ok",
        selected: AntigravitySessionKey?,
        transcriptReads: UInt64 = 0,
        tokenReads: UInt64 = 0
    ) {
        self.instanceId = instanceId
        self.generation = generation
        self.status = status
        self.selected = selected
        self.transcriptReads = transcriptReads
        self.tokenReads = tokenReads
    }
}

enum AntigravityOpenResult: String, Equatable, Sendable {
    case confirmed, unconfirmed, stale, denied, unavailable

    var label: String {
        switch self {
        case .confirmed: return "已聚焦并读回确认 Antigravity 会话"
        case .unconfirmed: return "已发送聚焦请求，但 Antigravity 选中读回未确认"
        case .stale: return "目标 Antigravity 会话已过期，未发送跳转"
        case .denied: return "Antigravity 拒绝会话聚焦或缺少合法 Schema/鉴权"
        case .unavailable: return "Antigravity 状态桥不可达，无法跳转"
        }
    }
}

enum AntigravityDeliveryMode: String, Codable, Equatable, Sendable {
    case auto
    case steer
    case queue
}

enum AntigravityDeliveryOutcome: String, Codable, Equatable, Sendable {
    case started
    case nextInvocation
    case steered
    case none
}

struct AntigravityCreateIntent: Codable, Equatable, Sendable {
    let requestId: String
    let workspacePath: String
    let projectId: String
    let prompt: String
    let title: String?
    let modelName: String?
    let conversationId: String?

    init(
        requestId: String = UUID().uuidString,
        workspacePath: String,
        projectId: String,
        prompt: String,
        title: String? = nil,
        modelName: String? = nil,
        conversationId: String? = nil
    ) {
        self.requestId = requestId
        self.workspacePath = workspacePath
        self.projectId = projectId
        self.prompt = prompt
        self.title = title
        self.modelName = modelName
        self.conversationId = conversationId
    }

    var valid: Bool {
        AntigravityStatusSnapshot.validID(requestId, limit: 256)
            && AntigravityStatusSnapshot.validID(workspacePath, limit: 4096)
            && AntigravityStatusSnapshot.validID(projectId, limit: 256)
            && AntigravityStatusSnapshot.validID(prompt, limit: 32768)
            && (title == nil || AntigravityStatusSnapshot.validID(title!, limit: 512))
            && (modelName == nil || AntigravityStatusSnapshot.validID(modelName!, limit: 256))
            && (conversationId == nil || AntigravityStatusSnapshot.validID(conversationId!, limit: 256))
    }
}

struct AntigravitySendIntent: Codable, Equatable, Sendable {
    let requestId: String
    let target: AntigravitySessionKey
    let projectId: String?
    let message: String
    let deliveryMode: AntigravityDeliveryMode

    init(
        requestId: String = UUID().uuidString,
        target: AntigravitySessionKey,
        projectId: String? = nil,
        message: String,
        deliveryMode: AntigravityDeliveryMode = .auto
    ) {
        self.requestId = requestId
        self.target = target
        self.projectId = projectId
        self.message = message
        self.deliveryMode = deliveryMode
    }

    var valid: Bool {
        AntigravityStatusSnapshot.validID(requestId, limit: 256)
            && AntigravityStatusSnapshot.validID(target.conversationId, limit: 256)
            && AntigravityStatusSnapshot.validID(target.workspacePath, limit: 4096)
            && (projectId == nil || AntigravityStatusSnapshot.validID(projectId!, limit: 256))
            && AntigravityStatusSnapshot.validID(message, limit: 32768)
    }
}

struct AntigravityQueryIntent: Codable, Equatable, Sendable {
    let requestId: String
    let expectedOp: String?
    let conversationId: String?
    let workspacePath: String?
    let projectId: String?

    init(
        requestId: String,
        expectedOp: String? = nil,
        conversationId: String? = nil,
        workspacePath: String? = nil,
        projectId: String? = nil
    ) {
        self.requestId = requestId
        self.expectedOp = expectedOp
        self.conversationId = conversationId
        self.workspacePath = workspacePath
        self.projectId = projectId
    }

    var valid: Bool {
        AntigravityStatusSnapshot.validID(requestId, limit: 256)
            && (expectedOp == nil || AntigravityStatusSnapshot.validID(expectedOp!, limit: 32))
            && (conversationId == nil || AntigravityStatusSnapshot.validID(conversationId!, limit: 256))
            && (workspacePath == nil || AntigravityStatusSnapshot.validID(workspacePath!, limit: 4096))
            && (projectId == nil || projectId!.utf8.count <= 256)
    }
}

enum AntigravityCommandOperation: Equatable, Sendable {
    case create(AntigravityCreateIntent)
    case send(AntigravitySendIntent)
    case query(AntigravityQueryIntent)

    var requestId: String {
        switch self {
        case .create(let intent): return intent.requestId
        case .send(let intent): return intent.requestId
        case .query(let intent): return intent.requestId
        }
    }

    var opName: String {
        switch self {
        case .create: return "create"
        case .send: return "send"
        case .query: return "query"
        }
    }
}

struct AntigravityQueuedCommand: Equatable, Sendable {
    let operation: AntigravityCommandOperation
    let instanceId: String
    let generation: UInt64
    let localEpoch: UInt64
}

struct AntigravityCommandReply: Equatable, Sendable {
    let instanceId: String
    let generation: UInt64
    let status: String
    let commandOp: String
    let requestId: String
    let conversationId: String
    let workspacePath: String
    let projectId: String
    let delivery: AntigravityDeliveryOutcome
    let inFlightSteering: Bool
    let reasonCode: String
    let duplicate: Bool
    let reconciled: Bool
    let cursor: UInt64
    let rows: [AntigravitySessionRow]
    let selected: AntigravitySessionKey?
    let transcriptReads: UInt64
    let tokenReads: UInt64

    init(
        instanceId: String,
        generation: UInt64,
        status: String,
        commandOp: String,
        requestId: String,
        conversationId: String,
        workspacePath: String,
        projectId: String,
        delivery: AntigravityDeliveryOutcome,
        inFlightSteering: Bool = false,
        reasonCode: String,
        duplicate: Bool = false,
        reconciled: Bool = false,
        cursor: UInt64,
        rows: [AntigravitySessionRow],
        selected: AntigravitySessionKey?,
        transcriptReads: UInt64 = 0,
        tokenReads: UInt64 = 0
    ) {
        self.instanceId = instanceId
        self.generation = generation
        self.status = status
        self.commandOp = commandOp
        self.requestId = requestId
        self.conversationId = conversationId
        self.workspacePath = workspacePath
        self.projectId = projectId
        self.delivery = delivery
        self.inFlightSteering = inFlightSteering
        self.reasonCode = reasonCode
        self.duplicate = duplicate
        self.reconciled = reconciled
        self.cursor = cursor
        self.rows = rows
        self.selected = selected
        self.transcriptReads = transcriptReads
        self.tokenReads = tokenReads
    }
}

enum AntigravityCommandResult: String, Equatable, Sendable {
    case created
    case started
    case queued
    case steered
    case unconfirmed
    case unknown
    case stale
    case denied
    case unavailable

    var label: String {
        switch self {
        case .created: return "已通过 agentapi 创建并确认 Antigravity 会话"
        case .started: return "已向 Antigravity 会话发送消息并开始执行"
        case .queued: return "Antigravity 会话运行中，消息已排入下一轮调用（nextInvocation）"
        case .steered: return "已向运行中的 Antigravity 会话注入 steering 消息"
        case .unconfirmed: return "Antigravity 命令未确认或被安全守卫拦截"
        case .unknown: return "Antigravity 命令响应超时，已锁定 requestId 待查询对账"
        case .stale: return "Antigravity 会话快照已过期，未发送命令"
        case .denied: return "Antigravity 拒绝此命令或缺少合法 Schema/鉴权"
        case .unavailable: return "Antigravity 状态桥不可达，无法执行命令"
        }
    }
}

enum AntigravityFailure: Error, Equatable, Sendable {
    case grant, denied, stale, unavailable, invalidReply, gap, transportTimeout

    var label: String {
        switch self {
        case .grant: return "请先配置 Antigravity 状态桥授权"
        case .denied: return "Antigravity 状态桥拒绝此请求"
        case .stale: return "Antigravity 事件流已断开或实例已换代"
        case .unavailable: return "Antigravity 状态桥连接中断"
        case .invalidReply: return "Antigravity 状态回执无法核对"
        case .gap: return "Antigravity 事件日志存在缺口，请重连"
        case .transportTimeout: return "Antigravity 命令传输超时"
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
    func open(_ target: AntigravitySessionKey) async throws -> AntigravityOpenReply
    func create(_ intent: AntigravityCreateIntent) async throws -> AntigravityCommandReply
    func send(_ intent: AntigravitySendIntent) async throws -> AntigravityCommandReply
    func query(_ intent: AntigravityQueryIntent) async throws -> AntigravityCommandReply
    func close() async
}

extension AntigravityStatusServing {
    func open(_ target: AntigravitySessionKey) async throws -> AntigravityOpenReply {
        throw AntigravityFailure.denied
    }

    func create(_ intent: AntigravityCreateIntent) async throws -> AntigravityCommandReply {
        throw AntigravityFailure.denied
    }

    func send(_ intent: AntigravitySendIntent) async throws -> AntigravityCommandReply {
        throw AntigravityFailure.denied
    }

    func query(_ intent: AntigravityQueryIntent) async throws -> AntigravityCommandReply {
        throw AntigravityFailure.denied
    }
}
