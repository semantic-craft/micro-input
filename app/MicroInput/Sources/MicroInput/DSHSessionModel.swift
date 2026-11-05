import Foundation

struct DSHSessionRow: Codable, Identifiable, Equatable, Sendable {
    let sessionId: String
    let workspaceId: String
    let title: String
    let active: Bool
    let running: Bool
    let pendingInput: UInt64
    var id: String { sessionId }
    var valid: Bool {
        DSHSessionSnapshot.validID(sessionId) && workspaceId.utf8.count <= 4096
            && title.utf8.count <= 4096 && pendingInput <= 1_000_000 && (!running || active)
    }
    var statusLabel: String {
        pendingInput > 0 ? "等待回应（\(pendingInput)）" : running ? "运行中" : active ? "空闲" : "未激活"
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
    func close() async
}
