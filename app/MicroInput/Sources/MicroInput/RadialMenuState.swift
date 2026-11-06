import Foundation
import MicroInputCore

enum RadialIntent: Equatable {
    case openApp(String)
    case cheatSheet
    case prompt(text: String, adapter: String?, submit: Bool)
    case submit(adapter: String?)
    case stop(adapter: String?)
    case navigateBack
    case navigateForward
}

struct RadialSlotConfig: Codable, Equatable {
    enum Kind: String, Codable, Equatable {
        case cheatSheet = "cheat_sheet"
        case openApp = "open_app"
        case prompt = "prompt"
        case submit = "submit"
        case stop = "stop"
        case navigateBack = "navigate_back"
        case navigateForward = "navigate_forward"
    }
    var kind: Kind
    var label: String
    var text: String?
    var bundleID: String?
    var adapter: String?
    var submit: Bool
    var symbol: String? = nil
    var colorHex: String? = nil

    static func prompt(label: String, text: String, adapter: String? = nil, submit: Bool = false, symbol: String? = nil, colorHex: String? = nil) -> RadialSlotConfig {
        RadialSlotConfig(kind: .prompt, label: label, text: text, bundleID: nil, adapter: adapter, submit: submit, symbol: symbol, colorHex: colorHex)
    }
    static func submit(label: String = "提交", adapter: String? = nil, symbol: String? = "paperplane.fill", colorHex: String? = "#0066FF") -> RadialSlotConfig {
        RadialSlotConfig(kind: .submit, label: label, text: nil, bundleID: nil, adapter: adapter, submit: true, symbol: symbol, colorHex: colorHex)
    }
    static func stop(label: String = "Stop · 停止", adapter: String? = nil, symbol: String? = "stop.circle.fill", colorHex: String? = "#FF3B30") -> RadialSlotConfig {
        RadialSlotConfig(kind: .stop, label: label, text: nil, bundleID: nil, adapter: adapter, submit: false, symbol: symbol, colorHex: colorHex)
    }
    static func navigateBack(label: String = "后退", symbol: String? = "chevron.backward", colorHex: String? = "#00CCAA") -> RadialSlotConfig {
        RadialSlotConfig(kind: .navigateBack, label: label, text: nil, bundleID: nil, adapter: nil, submit: false, symbol: symbol, colorHex: colorHex)
    }
    static func navigateForward(label: String = "前进", symbol: String? = "chevron.forward", colorHex: String? = "#00CCAA") -> RadialSlotConfig {
        RadialSlotConfig(kind: .navigateForward, label: label, text: nil, bundleID: nil, adapter: nil, submit: false, symbol: symbol, colorHex: colorHex)
    }
    static func cheatSheet(label: String = "按键速查", symbol: String? = "rectangle.grid.3x2", colorHex: String? = "#FFFFFF") -> RadialSlotConfig {
        RadialSlotConfig(kind: .cheatSheet, label: label, text: nil, bundleID: nil, adapter: nil, submit: false, symbol: symbol, colorHex: colorHex)
    }
    static func openApp(label: String, bundleID: String, symbol: String? = "app.badge", colorHex: String? = nil) -> RadialSlotConfig {
        RadialSlotConfig(kind: .openApp, label: label, text: nil, bundleID: bundleID, adapter: nil, submit: false, symbol: symbol, colorHex: colorHex)
    }

    static let paseoEngineeringSlots: [RadialSlotConfig] = [
        .navigateBack(label: "Back · 后退", symbol: "chevron.backward"),
        .prompt(label: "Test", text: "Run the tests and fix any failures", submit: false, symbol: "checkmark.shield", colorHex: "#00FF66"),
        .prompt(label: "Continue", text: "Continue", submit: false, symbol: "play.fill", colorHex: "#00AAFF"),
        .prompt(label: "Commit", text: "Commit this work", submit: false, symbol: "arrow.triangle.branch", colorHex: "#FFB800"),
        .navigateForward(label: "Forward · 前进", symbol: "chevron.forward"),
        .prompt(label: "Review", text: "Review your diff for bugs", submit: false, symbol: "magnifyingglass", colorHex: "#AA44FF"),
        .submit(label: "Submit · 提交", symbol: "paperplane.fill"),
        .prompt(label: "Summary", text: "Summarize where things stand and what's left", submit: false, symbol: "list.bullet.rectangle", colorHex: "#00CCAA")
    ]

    static let fourAppEngineeringSlots: [RadialSlotConfig] = [
        .navigateBack(label: "Back · 后退", symbol: "chevron.backward"),
        .prompt(label: "Test", text: "Run the tests and fix any failures", submit: false, symbol: "checkmark.shield", colorHex: "#00FF66"),
        .prompt(label: "Continue", text: "Continue", submit: false, symbol: "play.fill", colorHex: "#00AAFF"),
        .prompt(label: "Commit", text: "Commit this work", submit: false, symbol: "arrow.triangle.branch", colorHex: "#FFB800"),
        .navigateForward(label: "Forward · 前进", symbol: "chevron.forward"),
        .prompt(label: "Review", text: "Review your diff for bugs", submit: false, symbol: "magnifyingglass", colorHex: "#AA44FF"),
        .stop(label: "Stop · 停止", symbol: "stop.circle.fill"),
        .prompt(label: "Summary", text: "Summarize where things stand and what's left", submit: false, symbol: "list.bullet.rectangle", colorHex: "#00CCAA")
    ]
}
struct RadialSlot: Identifiable {
    let label: String
    let intent: RadialIntent?
    let unavailableReason: String?
    let symbol: String?
    let colorHex: String?
    var id: String { label }

    init(label: String, intent: RadialIntent?, unavailableReason: String?, symbol: String? = nil, colorHex: String? = nil) {
        self.label = label
        self.intent = intent
        self.unavailableReason = unavailableReason
        self.symbol = symbol
        self.colorHex = colorHex
    }
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
    enum ConfirmMode: String, Codable, Equatable, Sendable {
        case encoderPress = "encoder_press"
        case springBack = "spring_back"
    }

    enum Phase { case disabled, waitingForCenter, idle, showing(RadialMenuSession), settling }
    private(set) var phase: Phase = .disabled
    var confirmMode: ConfirmMode = .encoderPress
    private var sectorEnteredAt: Date?
    private(set) var armedAtPerimeter: Bool = false
    private var lastArmedSector: Int?
    private var perimeterExitedAt: Date?

    static let restDistance: Double = 0.15
    static let openDistance: Double = 0.50
    static let selectDistance: Double = 0.75
    static let springBackMaxDuration: TimeInterval = 0.50 // 500ms total flick envelope
    static let perimeterSnapBackMaxDuration: TimeInterval = 0.25 // 250ms snap-back from >=0.75 perimeter

    var enabled: Bool { if case .disabled = phase { return false }; return true }
    var session: RadialMenuSession? { if case .showing(let session) = phase { return session }; return nil }
    var capturesConfirm: Bool {
        switch phase { case .showing, .settling: return true; default: return false }
    }
    mutating func setEnabled(_ enabled: Bool) { phase = enabled ? .waitingForCenter : .disabled }
    mutating func setConfirmMode(_ mode: ConfirmMode) { confirmMode = mode }
    mutating func cancel() {
        if enabled {
            phase = .waitingForCenter
            sectorEnteredAt = nil
            armedAtPerimeter = false
            lastArmedSector = nil
            perimeterExitedAt = nil
        }
    }
    static func sector(_ position: RadialPositionInfo) -> Int? {
        guard position.angleTurns.isFinite, position.distance.isFinite,
              (0...1).contains(position.angleTurns), (0...1).contains(position.distance), position.distance >= Self.openDistance else { return nil }
        return Int((position.angleTurns * 8 + 0.5).rounded(.down)) % 8
    }
    @discardableResult
    mutating func update(_ position: RadialPositionInfo, now: Date = Date(), makeSession: () -> RadialMenuSession?) -> (RadialMenuSession, RadialIntent)? {
        guard enabled, position.distance.isFinite, (0...1).contains(position.distance) else { return nil }
        if position.distance < 0.35 {
            // Returned to center
            if confirmMode == .springBack,
               case .showing(var session) = phase,
               armedAtPerimeter,
               let entered = sectorEnteredAt,
               now.timeIntervalSince(entered) <= Self.springBackMaxDuration,
               (perimeterExitedAt == nil || now.timeIntervalSince(perimeterExitedAt!) <= Self.perimeterSnapBackMaxDuration),
               let targetSector = session.selected ?? lastArmedSector,
               session.slots.indices.contains(targetSector),
               let intent = session.slots[targetSector].intent {
                session.selected = targetSector
                phase = .settling
                sectorEnteredAt = nil
                armedAtPerimeter = false
                lastArmedSector = nil
                perimeterExitedAt = nil
                return (session, intent)
            }
            // Slow return, shallow (<0.75) deflection, or encoderPress mode: cancel back to idle (zero action)
            phase = .idle
            sectorEnteredAt = nil
            armedAtPerimeter = false
            lastArmedSector = nil
            perimeterExitedAt = nil
            return nil
        }
        switch phase {
        case .idle:
            guard let sector = Self.sector(position), var session = makeSession() else { return nil }
            session.selected = sector
            phase = .showing(session)
            sectorEnteredAt = now
            if position.distance >= Self.selectDistance {
                armedAtPerimeter = true
                lastArmedSector = sector
                perimeterExitedAt = nil
            } else {
                armedAtPerimeter = false
                lastArmedSector = nil
                perimeterExitedAt = nil
            }
        case .showing(var session):
            let newSector = Self.sector(position)
            if newSector != session.selected {
                session.selected = newSector
                if newSector != nil {
                    sectorEnteredAt = now
                    armedAtPerimeter = position.distance >= Self.selectDistance
                    lastArmedSector = armedAtPerimeter ? newSector : nil
                    perimeterExitedAt = nil
                } else if armedAtPerimeter && perimeterExitedAt == nil {
                    perimeterExitedAt = now
                }
            } else if let currentSector = newSector {
                if position.distance >= Self.selectDistance {
                    armedAtPerimeter = true
                    lastArmedSector = currentSector
                    perimeterExitedAt = nil
                } else if armedAtPerimeter && perimeterExitedAt == nil {
                    perimeterExitedAt = now
                }
            }
            phase = .showing(session)
        default: break
        }
        return nil
    }
    mutating func confirm() -> (RadialMenuSession, RadialIntent)? {
        guard let session, let intent = session.intent else { return nil }
        phase = .settling
        sectorEnteredAt = nil
        armedAtPerimeter = false
        lastArmedSector = nil
        perimeterExitedAt = nil
        return (session, intent)
    }
}

extension RadialInputReadiness {
    var label: String {
        switch self {
        case .ready: return "摇杆选择 · 旋钮按下或快速回弹确认 · 慢回中取消"
        case .disconnected: return "尚未连接已核验的键盘"
        case .stale: return "请读取当前设备身份与配置"
        case .unverifiedMapping: return "当前摇杆或旋钮映射不可用于菜单"
        case .paused: return "设备操作期间或映射重验前暂停菜单"
        }
    }
}
