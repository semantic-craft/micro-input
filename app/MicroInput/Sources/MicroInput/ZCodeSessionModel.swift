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

struct ZCodeTaskRow: Codable, Identifiable, Equatable, Sendable {
    let address: ZCodeTaskAddress
    let title: String
    let liveStatus: String
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
                  && $0.title.utf8.count <= 4096 && ["idle", "running", "waiting", "completed", "error"].contains($0.liveStatus) }),
              selected == nil || rows.contains(where: { $0.address == selected }) else { throw ZCodeFailure.invalidReply }
    }
}

struct ZCodeSelection: Equatable, Sendable {
    let address: ZCodeTaskAddress
    let instanceId: String
    let generation: UInt64
    let localEpoch: UInt64
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

enum ZCodeFailure: Error, Equatable, Sendable {
    case grant, denied, stale, unavailable, invalidReply
    var label: String {
        switch self {
        case .grant: return "请先在 ZCode 授权工作区连接"
        case .denied: return "ZCode 未授权此工作区"
        case .stale: return "ZCode 实例已变化，请重新连接"
        case .unavailable: return "ZCode 桥连接中断"
        case .invalidReply: return "ZCode 返回的任务身份无法核对"
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
    func close() async
}
