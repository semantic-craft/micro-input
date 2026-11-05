import Foundation

struct DSHSessionRow: Codable, Identifiable, Equatable, Sendable {
    let sessionId: String
    let workspaceId: String
    let title: String
    let active: Bool
    let running: Bool
    let pendingInput: UInt64
    let currentTurn: Int?
    let queuedCount: Int

    init(
        sessionId: String,
        workspaceId: String,
        title: String,
        active: Bool,
        running: Bool,
        pendingInput: UInt64,
        currentTurn: Int? = nil,
        queuedCount: Int = 0
    ) {
        self.sessionId = sessionId
        self.workspaceId = workspaceId
        self.title = title
        self.active = active
        self.running = running
        self.pendingInput = pendingInput
        self.currentTurn = currentTurn
        self.queuedCount = queuedCount
    }

    private enum CodingKeys: String, CodingKey {
        case sessionId, workspaceId, title, active, running, pendingInput, currentTurn, queuedCount
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        sessionId = try container.decode(String.self, forKey: .sessionId)
        workspaceId = try container.decode(String.self, forKey: .workspaceId)
        title = try container.decode(String.self, forKey: .title)
        active = try container.decode(Bool.self, forKey: .active)
        running = try container.decode(Bool.self, forKey: .running)
        pendingInput = try container.decode(UInt64.self, forKey: .pendingInput)
        currentTurn = try container.decodeIfPresent(Int.self, forKey: .currentTurn)
        queuedCount = try container.decodeIfPresent(Int.self, forKey: .queuedCount) ?? 0
    }

    var id: String { sessionId }
    var valid: Bool {
        DSHSessionSnapshot.validID(sessionId) && workspaceId.utf8.count <= 4096
            && title.utf8.count <= 4096 && pendingInput <= 1_000_000 && (!running || active)
            && (currentTurn == nil || (currentTurn! >= 0 && currentTurn! <= 1_000_000))
            && queuedCount >= 0 && queuedCount <= 1_000_000
    }
    var turnStatusLabel: String {
        if let currentTurn {
            return running ? "轮次 #\(currentTurn) 运行中" : "轮次 #\(currentTurn) 已结束"
        }
        return running ? "轮次 未知（运行中）" : "轮次 空闲"
    }
    var queueStatusLabel: String {
        queuedCount > 0 ? "队列 \(queuedCount) 条" : "队列 空"
    }
    var statusLabel: String {
        let base = pendingInput > 0 ? "等待回应（\(pendingInput)）" : running ? "运行中" : active ? "空闲" : "未激活"
        return "\(base) · \(turnStatusLabel) · \(queueStatusLabel)"
    }
}

struct DSHSessionChange: Codable, Equatable, Sendable {
    let sequence: UInt64
    let row: DSHSessionRow
}

struct DSHSessionSnapshot: Equatable, Sendable {
    let hostId: String
    let generation: UInt64
    let sessionIds: [String]
    let cursor: UInt64
    let rows: [DSHSessionRow]
    let unavailableSessionIds: [String]
    let changes: [DSHSessionChange]
    let selected: String?

    static func validID(_ value: String) -> Bool { !value.isEmpty && value.utf8.count <= 4096 }
    func validate(after previous: UInt64? = nil) throws {
        let scope = Set(sessionIds), known = Set(rows.map(\.sessionId)), unavailable = Set(unavailableSessionIds)
        guard Self.validID(hostId), hostId.utf8.count <= 256, generation > 0,
              !scope.isEmpty, scope.count <= 4096, scope.count == sessionIds.count,
              sessionIds.allSatisfy(Self.validID), rows.count == known.count,
              unavailable.count == unavailableSessionIds.count, known.isDisjoint(with: unavailable),
              known.union(unavailable) == scope, rows.allSatisfy(\.valid),
              changes.count <= 4096, changes.allSatisfy({ $0.row.valid && scope.contains($0.row.sessionId) }),
              selected == nil || known.contains(selected!) else { throw DSHFailure.invalidReply }
        if let previous {
            guard cursor >= previous, cursor - previous == UInt64(changes.count) else { throw DSHFailure.gap }
            for (offset, change) in changes.enumerated() {
                guard change.sequence == previous + UInt64(offset) + 1 else { throw DSHFailure.gap }
            }
        } else if !changes.isEmpty { throw DSHFailure.invalidReply }
    }
}

struct DSHSelection: Equatable, Sendable {
    let sessionId: String
    let workspaceId: String
    let hostId: String
    let generation: UInt64
    let localEpoch: UInt64
}

struct DSHOpenReply: Sendable {
    let hostId: String
    let generation: UInt64
    let status: DSHOpenResult
    let selected: String?
}

enum DSHOpenResult: String, Codable, Sendable {
    case confirmed, unconfirmed, stale, denied, unavailable
    var label: String {
        switch self {
        case .confirmed: return "已确认打开指定 DSH 会话"
        case .unconfirmed: return "DSH 未确认选中目标"
        case .stale: return "DSH 目标或连接已变化，打开未确认"
        case .denied: return "DSH 未授权此操作"
        case .unavailable: return "DSH 桥不可用，未重试打开"
        }
    }
}

enum DSHPromptMode: String, Codable, Equatable, Sendable {
    case queue
    case steer
}

struct DSHCreateIntent: Equatable, Sendable {
    let workspaceId: String
    let requestId: String
    let hostId: String
    let generation: UInt64
    let localEpoch: UInt64
}

struct DSHPromptIntent: Equatable, Sendable {
    let sessionId: String
    let workspaceId: String
    let requestId: String
    let mode: DSHPromptMode
    let text: String
    let hostId: String
    let generation: UInt64
    let localEpoch: UInt64
}

struct DSHCancelIntent: Equatable, Sendable {
    let sessionId: String
    let workspaceId: String
    let requestId: String
    let expectedTurn: Int
    let hostId: String
    let generation: UInt64
    let localEpoch: UInt64
}

struct DSHQueryIntent: Equatable, Sendable {
    let sessionId: String?
    let workspaceId: String?
    let requestId: String
    let hostId: String
    let generation: UInt64
    let localEpoch: UInt64
}

enum DSHQueuedCommand: Equatable, Sendable {
    case create(DSHCreateIntent)
    case prompt(DSHPromptIntent)
    case cancel(DSHCancelIntent)
    case query(DSHQueryIntent)

    var actionName: String {
        switch self {
        case .create: return "dsh_create"
        case .prompt: return "dsh_prompt"
        case .cancel: return "dsh_cancel"
        case .query: return "dsh_query"
        }
    }

    var requestId: String {
        switch self {
        case .create(let intent): return intent.requestId
        case .prompt(let intent): return intent.requestId
        case .cancel(let intent): return intent.requestId
        case .query(let intent): return intent.requestId
        }
    }

    var hostId: String {
        switch self {
        case .create(let intent): return intent.hostId
        case .prompt(let intent): return intent.hostId
        case .cancel(let intent): return intent.hostId
        case .query(let intent): return intent.hostId
        }
    }

    var generation: UInt64 {
        switch self {
        case .create(let intent): return intent.generation
        case .prompt(let intent): return intent.generation
        case .cancel(let intent): return intent.generation
        case .query(let intent): return intent.generation
        }
    }

    var localEpoch: UInt64 {
        switch self {
        case .create(let intent): return intent.localEpoch
        case .prompt(let intent): return intent.localEpoch
        case .cancel(let intent): return intent.localEpoch
        case .query(let intent): return intent.localEpoch
        }
    }
}

struct DSHCommandReply: Equatable, Sendable {
    let hostId: String
    let generation: UInt64
    let status: String
    let sessionId: String?
    let workspaceId: String?
    let selected: String?
    let requestId: String
    let delivery: DSHPromptMode?
    let expectedTurn: Int?
    let currentTurn: Int?
    let queuedCount: Int?
    let duplicate: Bool
    let reasonCode: String?
}

enum DSHCommandResult: Equatable, Sendable {
    case created(sessionId: String, workspaceId: String, selected: String, duplicate: Bool)
    case queued(sessionId: String, requestId: String, currentTurn: Int?, queuedCount: Int, duplicate: Bool)
    case steered(sessionId: String, requestId: String, currentTurn: Int?, queuedCount: Int, duplicate: Bool)
    case cancelled(sessionId: String, requestId: String, expectedTurn: Int, currentTurn: Int?, queuedCount: Int, duplicate: Bool)
    case cancelledQueueRemaining(sessionId: String, requestId: String, expectedTurn: Int, currentTurn: Int?, queuedCount: Int, duplicate: Bool)
    case noop(reasonCode: String, currentTurn: Int?, queuedCount: Int?)
    case unknown(requestId: String, reasonCode: String)
    case unconfirmed(selected: String?, reasonCode: String)
    case stale
    case denied(reasonCode: String?)
    case unavailable

    var rawValue: String {
        switch self {
        case .created: return "confirmed"
        case .queued: return "queued"
        case .steered: return "steered"
        case .cancelled: return "cancelled"
        case .cancelledQueueRemaining: return "cancelledQueueRemaining"
        case .noop: return "noop"
        case .unknown: return "unknown"
        case .unconfirmed: return "unconfirmed"
        case .stale: return "stale"
        case .denied: return "denied"
        case .unavailable: return "unavailable"
        }
    }

    var label: String {
        switch self {
        case .created:
            return "已新建并确认选中 DSH 会话"
        case .queued(_, _, let currentTurn, let queuedCount, _):
            return "已入队下一轮次（\(currentTurn.map { "轮次 #\($0)" } ?? "轮次空闲") · 队列 \(queuedCount) 条）"
        case .steered(_, _, let currentTurn, let queuedCount, _):
            return "已转向当前轮次（\(currentTurn.map { "轮次 #\($0)" } ?? "运行中") · 队列 \(queuedCount) 条）"
        case .cancelled(_, _, let expectedTurn, _, _, _):
            return "已停止当前轮次 #\(expectedTurn)（队列已空）"
        case .cancelledQueueRemaining(_, _, let expectedTurn, _, let queuedCount, _):
            return "已停止当前轮次 #\(expectedTurn)（队列仍有 \(queuedCount) 条可能继续）"
        case .noop(let reasonCode, let currentTurn, _):
            return "未取消任何轮次（\(reasonCode)，\(currentTurn.map { "当前轮次 #\($0)" } ?? "当前空闲")）"
        case .unknown(let requestId, _):
            return "DSH 命令结果未知（\(requestId)），需先对账"
        case .unconfirmed:
            return "DSH 未确认选中目标会话，未执行命令"
        case .stale:
            return "DSH 目标或连接已变化，命令未确认"
        case .denied(let reasonCode):
            if reasonCode == "guard.unprovableTurnAtomicity" {
                return "DSH 未能证明同 Host 轮次原子核对，已拒绝发送"
            }
            return "DSH 拒绝此命令\(reasonCode.map { "（\($0)）" } ?? "")"
        case .unavailable:
            return "DSH 桥不可用，未盲目重试"
        }
    }
}

enum DSHFailure: Error, Equatable, Sendable {
    case grant, denied, stale, unavailable, invalidReply, gap
    var label: String {
        switch self {
        case .grant: return "请先在 DSH 授权会话连接"
        case .denied: return "DSH 未授权此会话"
        case .stale: return "DSH Host 已变化，请重新连接"
        case .unavailable: return "DSH 桥连接中断"
        case .invalidReply: return "DSH 返回的会话状态无法核对"
        case .gap: return "DSH 状态变化存在缺口，请重新连接"
        }
    }
}

enum DSHConnectionState: Equatable {
    case disabled, connecting, ready, unavailable(DSHFailure)
    var label: String {
        switch self {
        case .disabled: return "DSH 会话连接未开启"
        case .connecting: return "正在连接 DSH"
        case .ready: return "DSH 会话连接已就绪"
        case .unavailable(let reason): return reason.label
        }
    }
}

protocol DSHSessionServing: Sendable {
    func connect() async throws -> DSHSessionSnapshot
    func read() async throws -> DSHSessionSnapshot
    func open(_ selection: DSHSelection) async throws -> DSHOpenReply
    func create(_ intent: DSHCreateIntent) async throws -> DSHCommandReply
    func prompt(_ intent: DSHPromptIntent) async throws -> DSHCommandReply
    func cancel(_ intent: DSHCancelIntent) async throws -> DSHCommandReply
    func query(_ intent: DSHQueryIntent) async throws -> DSHCommandReply
    func close() async
}

extension DSHSessionServing {
    func create(_ intent: DSHCreateIntent) async throws -> DSHCommandReply {
        throw DSHFailure.unavailable
    }
    func prompt(_ intent: DSHPromptIntent) async throws -> DSHCommandReply {
        throw DSHFailure.unavailable
    }
    func cancel(_ intent: DSHCancelIntent) async throws -> DSHCommandReply {
        throw DSHFailure.unavailable
    }
    func query(_ intent: DSHQueryIntent) async throws -> DSHCommandReply {
        throw DSHFailure.unavailable
    }
}
