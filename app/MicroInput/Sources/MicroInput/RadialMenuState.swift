import Foundation
import MicroInputCore

enum RadialIntent: Equatable {
    case openApp(String)
    case t3(T3Selection)
    case cheatSheet
}
struct RadialSlot: Identifiable {
    let label: String
    let intent: RadialIntent?
    let unavailableReason: String?
    var id: String { label }
}
struct RadialRoute: Equatable {
    let documentID: String
    let documentRevision: UInt64
    let hostRevision: UInt64
    let profileID: String?
    init(_ state: HostRoutingState) {
        documentID = state.document_id; documentRevision = state.document_revision
        hostRevision = state.host_state_revision; profileID = state.profile_id
    }
}
struct RadialMenuSession {
    let inputSession: UInt64
    let routing: HostRoutingState
    let slots: [RadialSlot]
    var selected: Int?
    var intent: RadialIntent? {
        guard let selected, slots.indices.contains(selected) else { return nil }
        return slots[selected].intent
    }
}
struct RadialMenuState {
    enum Phase { case disabled, waitingForCenter, idle, showing(RadialMenuSession), settling }
    private(set) var phase: Phase = .disabled
    var enabled: Bool { if case .disabled = phase { return false }; return true }
    var session: RadialMenuSession? { if case .showing(let session) = phase { return session }; return nil }
    var capturesConfirm: Bool {
        switch phase { case .showing, .settling: return true; default: return false }
    }
    mutating func setEnabled(_ enabled: Bool) { phase = enabled ? .waitingForCenter : .disabled }
    mutating func cancel() { if enabled { phase = .waitingForCenter } }
    static func sector(_ position: RadialPositionInfo) -> Int? {
        guard position.angleTurns.isFinite, position.distance.isFinite,
              (0...1).contains(position.angleTurns), (0...1).contains(position.distance), position.distance >= 0.5 else { return nil }
        return Int((position.angleTurns * 8 + 0.5).rounded(.down)) % 8
    }
    mutating func update(_ position: RadialPositionInfo, makeSession: () -> RadialMenuSession?) {
        guard enabled, position.distance.isFinite, (0...1).contains(position.distance) else { return }
        if position.distance < 0.35 { phase = .idle; return }
        switch phase {
        case .idle:
            guard let sector = Self.sector(position), var session = makeSession() else { return }
            session.selected = sector; phase = .showing(session)
        case .showing(var session):
            session.selected = Self.sector(position); phase = .showing(session)
        default: break
        }
    }
    mutating func confirm() -> (RadialMenuSession, RadialIntent)? {
        guard let session, let intent = session.intent else { return nil }
        phase = .settling
        return (session, intent)
    }
}

extension RadialInputReadiness {
    var label: String {
        switch self {
        case .ready: return "摇杆选择 · 旋钮按下确认 · 回中取消"
        case .disconnected: return "尚未连接已核验的键盘"
        case .stale: return "请读取当前设备身份与配置"
        case .unverifiedMapping: return "当前摇杆或旋钮映射不可用于菜单"
        case .paused: return "设备操作期间或映射重验前暂停菜单"
        }
    }
}
