import Foundation
import SwiftUI
import MicroInputCore

/// Real semantic status of a coding agent application session.
enum FourAppAgentPhase: String, Codable, Equatable, Sendable {
    case idle = "idle"
    case thinking = "thinking"
    case awaitingInput = "awaiting_input"
    case completed = "completed"
    case error = "error"
    case backgroundActive = "background_active"

    var label: String {
        switch self {
        case .idle: return "空闲/就绪"
        case .thinking: return "生成中/思考中"
        case .awaitingInput: return "待输入/待审批"
        case .completed: return "会话已完成"
        case .error: return "异常/失败"
        case .backgroundActive: return "后台任务运行中"
        }
    }

    var symbol: String {
        switch self {
        case .idle: return "checkmark.circle"
        case .thinking: return "ellipsis.circle"
        case .awaitingInput: return "exclamationmark.circle"
        case .completed: return "sparkles"
        case .error: return "xmark.octagon"
        case .backgroundActive: return "arrow.triangle.2.circlepath"
        }
    }
}

/// Dual-zone lighting feedback specification.
struct DualZoneLightingFeedback: Equatable, Sendable {
    var backlightColor: String
    var backlightEffect: String
    var backlightBrightness: Double
    var backlightSpeed: Double
    var underglowColor: String
    var underglowEffect: String
    var underglowBrightness: Double
    var underglowSpeed: Double
}

/// Per-key status feedback specification.
struct PerKeyLightingFeedback: Equatable, Sendable {
    var bindingId: String
    var color: String
    var phase: FourAppAgentPhase
    var label: String
}

/// Controller coordinating status derivation, dual-zone and per-key feedback,
/// and enforcing the Single Lighting Sender invariant across Antigravity, Cindy, ZCode, and DSH.
@MainActor
final class FourAppLightingController: ObservableObject {
    enum UpdateResult: String, Equatable, Sendable {
        case applied = "applied"
        case yieldedToNative = "yielded_to_native"
        case suspendedByProtectedLayer = "suspended_by_protected_layer"
        case deviceBusy = "device_busy"
        case released = "released"
        case noDevice = "no_device"
    }

    @Published private(set) var activePhase: FourAppAgentPhase = .idle
    @Published private(set) var dualZoneFeedback: DualZoneLightingFeedback?
    @Published private(set) var perKeyFeedback: [PerKeyLightingFeedback] = []
    @Published private(set) var lastUpdateResult: UpdateResult = .released
    @Published private(set) var activeRuntimeToken: String?
    @Published var lightingEnabled: Bool = true
    @Published var sessionLightingLinked: Bool = true
    @Published var onlyGlowOnAttentionOrCompleted: Bool = true
    @Published var glowOnWorkingSessions: Bool = false
    @Published var brightnessScale: Double = 0.85
    @Published var idleColorHex: String = "#FFFFFF"
    @Published var manualPreviewPhase: FourAppAgentPhase? = nil
    @Published private(set) var acknowledgedWatermarks: [KeypadNavigationPlace: String] = [:]
    @Published private(set) var hardwareSynced: Bool = false
    @Published private(set) var lastHardwareSyncResponse: String = ""

    init() {
        if NSClassFromString("XCTestCase") == nil,
           ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] == nil {
            self.glowOnWorkingSessions = (UserDefaults.standard.object(forKey: "mi_glow_on_working_sessions") as? Bool) ?? true
        }
    }

    func slotWatermark(for place: KeypadNavigationPlace, in store: Store) -> String {
        switch place {
        case .antigravity(let conv, let ws):
            if let row = store.antigravityStatus.rows.first(where: { $0.conversationId == conv && $0.workspacePath == ws }) {
                return "\(row.transitionCount):\(row.stepIdx):\(row.phase.rawValue)"
            }
            return "ag:\(conv)"
        case .cindy(let ws, let sess):
            if let row = store.cindySessions.rows.first(where: { $0.address.workspacePath == ws && $0.address.sessionId == sess }) {
                return "\(row.eventWatermark ?? 0):\(row.phase)"
            }
            return "cindy:\(sess)"
        case .zcode(_, _, let ws, let tid):
            if let row = store.zcodeSessions.rows.first(where: { $0.address.workspacePath == ws && $0.address.taskId == tid }) {
                return "\(row.executionId ?? ""):\(row.liveStatus)"
            }
            return "zcode:\(tid)"
        case .dsh(let sess, _, _):
            if let row = store.dshSessions.rows.first(where: { $0.sessionId == sess }) {
                return "\(row.currentTurn ?? 0):\(row.running):\(row.pendingInput)"
            }
            return "dsh:\(sess)"
        default:
            return place.receiptTarget
        }
    }

    func isSlotAttentionNeeded(_ slot: FourAppSlot, in store: Store? = nil) -> Bool {
        guard let place = slot.place else { return false }
        let st = slot.status
        if ["waiting", "waiting_approval", "needs-interaction", "needs_interaction", "error", "failed"].contains(st) {
            return true
        }
        if st == "completed" {
            if let store {
                let wm = slotWatermark(for: place, in: store)
                return acknowledgedWatermarks[place] != wm
            }
            return true
        }
        return false
    }

    func glowingAttentionSlots(in store: Store) -> [FourAppSlot] {
        store.fourAppSlots.slots.filter { isSlotAttentionNeeded($0, in: store) }
    }

    func isSlotWorking(_ slot: FourAppSlot) -> Bool {
        guard slot.place != nil else { return false }
        return ["running", "active", "runningTool", "generating", "busy", "background_active", "backgroundActive"].contains(slot.status)
    }

    func workingSlots(in store: Store) -> [FourAppSlot] {
        store.fourAppSlots.slots.filter { isSlotWorking($0) }
    }

    func acknowledgeSlot(place: KeypadNavigationPlace, in store: Store) {
        let wm = slotWatermark(for: place, in: store)
        acknowledgedWatermarks[place] = wm
    }

    func acknowledgeAllCompleted(in store: Store) async {
        for slot in store.fourAppSlots.slots where slot.status == "completed" {
            if let place = slot.place {
                acknowledgedWatermarks[place] = slotWatermark(for: place, in: store)
            }
        }
        await forceSyncNow(in: store)
    }

    func setOnlyGlowOnAttentionOrCompleted(_ value: Bool, in store: Store) async {
        onlyGlowOnAttentionOrCompleted = value
        await forceSyncNow(in: store)
    }

    func setGlowOnWorkingSessions(_ value: Bool, in store: Store) async {
        glowOnWorkingSessions = value
        if NSClassFromString("XCTestCase") == nil {
            UserDefaults.standard.set(value, forKey: "mi_glow_on_working_sessions")
        }
        await forceSyncNow(in: store)
    }

    /// Evaluates whether Micro Input is currently the sole allowed lighting sender.
    func canSendLighting(in store: Store) -> Bool {
        // Invariant 1: Native ownership must not be active
        if case .native = store.nativeOwnership.state { return false }
        guard store.nativeOwnership.allowsExecution else { return false }
        // Invariant 2: Protected vendor layer (Mg1 / KV_OAI_*) must not be active
        guard !store.isProtectedLayerSelected else { return false }
        // Invariant 3: Device operations / previews must not be in conflict
        guard !store.startingLightingPreview && !store.endingLightingPreview else { return false }
        return true
    }

    /// Derives the real status phase for any of the 4 supported desktop agents.
    func deriveState(for app: String, in store: Store) -> FourAppAgentPhase {
        switch app {
        case "antigravity":
            let topRows = Array(
                store.antigravityStatus.rows
                    .sorted(by: { $0.transitionCount > $1.transitionCount })
                    .prefix(6)
            )
            if topRows.contains(where: { $0.pendingInteraction != nil }) {
                return .awaitingInput
            }
            let unacknowledgedCompleted = topRows.contains { row in
                guard row.phase == .completed else { return false }
                let place = KeypadNavigationPlace.antigravity(conversationId: row.conversationId, workspacePath: row.workspacePath)
                let wm = "\(row.transitionCount):\(row.stepIdx):\(row.phase.rawValue)"
                return acknowledgedWatermarks[place] != wm
            }
            if let selected = store.antigravityStatus.selected,
               let matched = store.antigravityStatus.rows.first(where: { $0.conversationId == selected.conversationId && $0.workspacePath == selected.workspacePath }) {
                if matched.pendingInteraction != nil {
                    return .awaitingInput
                }
                if matched.phase == .completed {
                    let place = KeypadNavigationPlace.antigravity(conversationId: matched.conversationId, workspacePath: matched.workspacePath)
                    let wm = "\(matched.transitionCount):\(matched.stepIdx):\(matched.phase.rawValue)"
                    if acknowledgedWatermarks[place] != wm {
                        return .completed
                    }
                }
                if matched.phase == .failed {
                    return .error
                }
            }
            if unacknowledgedCompleted {
                return .completed
            }
            if topRows.contains(where: { $0.phase == .failed }) {
                return .error
            }
            if topRows.contains(where: { $0.phase == .running || $0.phase == .runningTool }) {
                return .thinking
            }
            if topRows.contains(where: { $0.phase == .backgroundActive }) {
                return .backgroundActive
            }
            return .idle

        case "cindy":
            if let selected = store.cindySessions.selected,
               let row = store.cindySessions.rows.first(where: { $0.address == selected }) {
                if row.phase == "needs-interaction" || row.pendingInteraction != nil {
                    return .awaitingInput
                }
                switch row.phase {
                case "running": return .thinking
                case "error": return .error
                default: return .idle
                }
            }
            return .idle

        case "zcode":
            if let selected = store.zcodeSessions.selected,
               let row = store.zcodeSessions.rows.first(where: { $0.address == selected }) {
                if row.liveStatus == "waiting" || row.liveStatus == "waiting_approval" || row.pendingInteraction != nil {
                    return .awaitingInput
                }
                switch row.liveStatus {
                case "running", "busy", "generating": return .thinking
                case "error": return .error
                default: return .idle
                }
            }
            return .idle

        case "dsh-desktop", "dsh":
            if let selected = store.dshSessions.selected,
               let row = store.dshSessions.rows.first(where: { $0.sessionId == selected }) {
                if row.pendingInput > 0 || row.pendingInteraction != nil {
                    return .awaitingInput
                }
                if row.running {
                    return .thinking
                }
                return .idle
            }
            return .idle

        default:
            return .idle
        }
    }

    /// Derives dual-zone lighting feedback based on real agent phase and hardware state.
    func deriveDualZoneFeedback(
        phase: FourAppAgentPhase,
        dictationActive: Bool,
        nativeOwnershipActive: Bool
    ) -> DualZoneLightingFeedback {
        let blColor: String
        let blEffect: String
        let blBrightness: Double
        let blSpeed: Double

        switch phase {
        case .idle:
            blColor = "#FFFFFF"
            blEffect = "solid"
            blBrightness = 0.5
            blSpeed = 0.5
        case .thinking:
            blColor = "#0088FF"
            blEffect = "breath"
            blBrightness = 0.8
            blSpeed = 0.7
        case .awaitingInput:
            blColor = "#FFAA00"
            blEffect = "breath"
            blBrightness = 1.0
            blSpeed = 0.8
        case .completed:
            blColor = "#00FF66"
            blEffect = "breath"
            blBrightness = 0.95
            blSpeed = 0.65
        case .error:
            blColor = "#FF2200"
            blEffect = "breath"
            blBrightness = 1.0
            blSpeed = 0.9
        case .backgroundActive:
            blColor = "#AA44FF"
            blEffect = "breath"
            blBrightness = 0.7
            blSpeed = 0.5
        }

        let ugColor: String
        let ugEffect: String
        let ugBrightness: Double
        let ugSpeed: Double

        if nativeOwnershipActive {
            ugColor = "#000000"
            ugEffect = "off"
            ugBrightness = 0.0
            ugSpeed = 0.0
        } else if dictationActive {
            ugColor = "#8800FF"
            ugEffect = "solid"
            ugBrightness = 1.0
            ugSpeed = 0.5
        } else if phase == .completed {
            ugColor = "#00FF66"
            ugEffect = "breath"
            ugBrightness = 0.9
            ugSpeed = 0.65
        } else {
            ugColor = "#00CCAA"
            ugEffect = "solid"
            ugBrightness = 0.6
            ugSpeed = 0.5
        }

        return DualZoneLightingFeedback(
            backlightColor: blColor,
            backlightEffect: blEffect,
            backlightBrightness: blBrightness,
            backlightSpeed: blSpeed,
            underglowColor: ugColor,
            underglowEffect: ugEffect,
            underglowBrightness: ugBrightness,
            underglowSpeed: ugSpeed
        )
    }

    /// Derives per-key feedback for slots (AG00..AG05), wide key (AG10/11), and controls.
    func derivePerKeyFeedback(
        slots: [FourAppSlot],
        dictationActive: Bool
    ) -> [PerKeyLightingFeedback] {
        var result: [PerKeyLightingFeedback] = []

        let keyIds = ["AG00", "AG01", "AG02", "AG03", "AG04", "AG05"]
        for (i, keyId) in keyIds.enumerated() {
            if slots.indices.contains(i), slots[i].place != nil {
                let slotStatus = slots[i].status
                let phase: FourAppAgentPhase
                let color: String
                if slotStatus == "running" || slotStatus == "runningTool" || slotStatus == "generating" || slotStatus == "busy" || slotStatus == "active" {
                    phase = .thinking
                    color = "#0088FF"
                } else if slotStatus == "waiting" || slotStatus == "waiting_approval" || slotStatus == "needs-interaction" || slotStatus == "needs_interaction" {
                    phase = .awaitingInput
                    color = "#FFAA00"
                } else if slotStatus == "error" {
                    phase = .error
                    color = "#FF2200"
                } else if slotStatus == "background_active" {
                    phase = .backgroundActive
                    color = "#AA44FF"
                } else if slotStatus == "completed" {
                    phase = .completed
                    color = "#00FF66"
                } else {
                    phase = .idle
                    color = "#00FF66"
                }
                result.append(PerKeyLightingFeedback(
                    bindingId: keyId,
                    color: color,
                    phase: phase,
                    label: slots[i].title.isEmpty ? "\(slots[i].app) slot \(i)" : slots[i].title
                ))
            } else {
                result.append(PerKeyLightingFeedback(
                    bindingId: keyId,
                    color: "#222222",
                    phase: .idle,
                    label: "空槽位 \(i)"
                ))
            }
        }

        let wideColor = dictationActive ? "#FF00FF" : "#00CCAA"
        result.append(PerKeyLightingFeedback(
            bindingId: "AG10",
            color: wideColor,
            phase: dictationActive ? .thinking : .idle,
            label: dictationActive ? "听写中" : "听写就绪"
        ))

        result.append(PerKeyLightingFeedback(
            bindingId: "ENC_CLK",
            color: "#FFFFFF",
            phase: .idle,
            label: "旋钮确认/跳底"
        ))

        return result
    }

    /// Derives full 14-control per-key feedback (`AG00..AG12` + `ENC_CLK`) for `v.oai.thstatus` hardware LED dispatch.
    func deriveFullKeypadFeedback(
        slots: [FourAppSlot],
        dictationActive: Bool,
        activePhase: FourAppAgentPhase,
        hasAttention: Bool,
        isBottomRowSplit: Bool = false
    ) -> [PerKeyLightingFeedback] {
        var entries = derivePerKeyFeedback(slots: slots, dictationActive: dictationActive)
        let baseByKey = Dictionary(uniqueKeysWithValues: entries.map { ($0.bindingId, $0) })
        entries.removeAll()
        for keyId in ["AG00", "AG01", "AG02", "AG03", "AG04", "AG05"] {
            if let existing = baseByKey[keyId] {
                entries.append(existing)
            }
        }
        entries.append(PerKeyLightingFeedback(
            bindingId: "AG06",
            color: hasAttention ? "#FFB800" : "#334455",
            phase: hasAttention ? .awaitingInput : .idle,
            label: "全局待办跳转"
        ))
        let approvalReady = activePhase == .awaitingInput || hasAttention
        entries.append(PerKeyLightingFeedback(
            bindingId: "AG07",
            color: approvalReady ? "#00FF66" : "#224433",
            phase: approvalReady ? .awaitingInput : .idle,
            label: "审批通过"
        ))
        entries.append(PerKeyLightingFeedback(
            bindingId: "AG08",
            color: approvalReady ? "#FF3B30" : "#442222",
            phase: approvalReady ? .awaitingInput : .idle,
            label: "审批拒绝"
        ))
        let running = activePhase == .thinking
        entries.append(PerKeyLightingFeedback(
            bindingId: "AG09",
            color: running ? "#FF3B30" : "#00AAFF",
            phase: running ? .thinking : .idle,
            label: running ? "停止生成" : "新建会话"
        ))
        let wideColor = dictationActive ? "#FF00FF" : "#00CCAA"
        if isBottomRowSplit {
            entries.append(PerKeyLightingFeedback(
                bindingId: "AG10",
                color: "#00AAFF",
                phase: .idle,
                label: "聚焦输入框"
            ))
            entries.append(PerKeyLightingFeedback(
                bindingId: "AG11",
                color: wideColor,
                phase: dictationActive ? .thinking : .idle,
                label: dictationActive ? "听写中" : "按住说话"
            ))
        } else {
            entries.append(PerKeyLightingFeedback(
                bindingId: "AG10",
                color: wideColor,
                phase: dictationActive ? .thinking : .idle,
                label: dictationActive ? "听写中 (2U 左)" : "按住说话 (2U 左)"
            ))
            entries.append(PerKeyLightingFeedback(
                bindingId: "AG11",
                color: wideColor,
                phase: dictationActive ? .thinking : .idle,
                label: dictationActive ? "听写中 (2U 右)" : "按住说话 (2U 右)"
            ))
        }
        entries.append(PerKeyLightingFeedback(
            bindingId: "AG12",
            color: "#0066FF",
            phase: activePhase,
            label: "停止听写并提交"
        ))
        entries.append(PerKeyLightingFeedback(
            bindingId: "ENC_CLK",
            color: "#FFFFFF",
            phase: .idle,
            label: "旋钮确认/跳底"
        ))
        return entries
    }

    @Published private(set) var fullKeypadFeedback: [PerKeyLightingFeedback] = []
    @Published private(set) var lastHardwareSyncJSON: String?
    private var lastSyncedHardwarePayload: String?
    private var lastSyncedDeviceID: String?

    /// Update lighting with single-sender invariant checks.
    func updateFeedback(in store: Store) async -> UpdateResult {
        let app = store.activeDesktopAgentAdapter() ?? "antigravity"
        let livePhase = deriveState(for: app, in: store)
        let phase: FourAppAgentPhase
        if let manualPreviewPhase {
            phase = manualPreviewPhase
        } else if !sessionLightingLinked {
            phase = .idle
        } else {
            phase = livePhase
        }
        self.activePhase = phase

        // Invariant 1: Native ownership must not be active
        if case .native = store.nativeOwnership.state {
            await release(in: store)
            lastUpdateResult = .yieldedToNative
            return .yieldedToNative
        }
        guard store.nativeOwnership.allowsExecution else {
            await release(in: store)
            lastUpdateResult = .yieldedToNative
            return .yieldedToNative
        }

        // Invariant 2: Protected layer must not be selected
        guard !store.isProtectedLayerSelected else {
            await release(in: store)
            lastUpdateResult = .suspendedByProtectedLayer
            return .suspendedByProtectedLayer
        }

        // Invariant 3: Device operations must not be in conflict
        guard !store.startingLightingPreview && !store.endingLightingPreview else {
            lastUpdateResult = .deviceBusy
            return .deviceBusy
        }

        let dictationActive = store.fourAppVoice.dictationState.isRecording || store.pushToTalk.isActive
        let clampedScale = min(max(brightnessScale, 0.15), 1.0)
        let attentionSlots = glowingAttentionSlots(in: store)
        let hasAnyAttentionOrCompleted = !attentionSlots.isEmpty || store.highestPriorityFourAppTarget() != nil
        var dualZone = deriveDualZoneFeedback(
            phase: phase,
            dictationActive: dictationActive,
            nativeOwnershipActive: false
        )
        if !lightingEnabled {
            dualZone = DualZoneLightingFeedback(
                backlightColor: "#000000",
                backlightEffect: "off",
                backlightBrightness: 0.0,
                backlightSpeed: 0.0,
                underglowColor: "#000000",
                underglowEffect: "off",
                underglowBrightness: 0.0,
                underglowSpeed: 0.0
            )
        } else if onlyGlowOnAttentionOrCompleted && manualPreviewPhase == nil && sessionLightingLinked {
            // Selective attention-only mode: keep background keys dark so ONLY completed / awaiting-input session keys glow!
            dualZone.backlightColor = "#000000"
            dualZone.backlightEffect = "off"
            dualZone.backlightBrightness = 0.0
            dualZone.backlightSpeed = 0.0
            if dictationActive {
                dualZone.underglowColor = "#8800FF"
                dualZone.underglowEffect = "solid"
                dualZone.underglowBrightness = min(1.0 * clampedScale, 1.0)
                dualZone.underglowSpeed = 0.5
            } else if phase == .awaitingInput || attentionSlots.contains(where: { $0.status != "completed" }) {
                dualZone.underglowColor = "#FFAA00"
                dualZone.underglowEffect = "breath"
                dualZone.underglowBrightness = min(0.95 * clampedScale, 1.0)
                dualZone.underglowSpeed = 0.75
            } else if phase == .completed || !attentionSlots.isEmpty {
                dualZone.underglowColor = "#00FF66"
                dualZone.underglowEffect = "breath"
                dualZone.underglowBrightness = min(0.95 * clampedScale, 1.0)
                dualZone.underglowSpeed = 0.65
            } else if glowOnWorkingSessions && (phase == .thinking || !workingSlots(in: store).isEmpty) {
                dualZone.underglowColor = "#0088FF"
                dualZone.underglowEffect = "breath"
                dualZone.underglowBrightness = min(0.85 * clampedScale, 1.0)
                dualZone.underglowSpeed = 0.65
            } else {
                dualZone.underglowColor = "#000000"
                dualZone.underglowEffect = "off"
                dualZone.underglowBrightness = 0.0
                dualZone.underglowSpeed = 0.0
            }
        } else {
            if phase == .idle && !dictationActive {
                dualZone.backlightColor = idleColorHex
                dualZone.underglowColor = idleColorHex
                dualZone.backlightBrightness = 0.72
                dualZone.underglowBrightness = 0.78
            }
            dualZone.backlightBrightness = min(max(dualZone.backlightBrightness * clampedScale, 0.0), 1.0)
            dualZone.underglowBrightness = min(max(dualZone.underglowBrightness * clampedScale, 0.0), 1.0)
        }
        self.dualZoneFeedback = dualZone
        self.perKeyFeedback = derivePerKeyFeedback(
            slots: store.fourAppSlots.slots,
            dictationActive: dictationActive
        )
        let hasAttention = store.highestPriorityFourAppTarget() != nil
        var fullKeys = deriveFullKeypadFeedback(
            slots: store.fourAppSlots.slots,
            dictationActive: dictationActive,
            activePhase: phase,
            hasAttention: hasAttention || hasAnyAttentionOrCompleted,
            isBottomRowSplit: store.isBottomRowSplit
        )
        if onlyGlowOnAttentionOrCompleted && manualPreviewPhase == nil && sessionLightingLinked {
            let slotIds = ["AG00", "AG01", "AG02", "AG03", "AG04", "AG05"]
            fullKeys = fullKeys.map { item in
                if let slotIdx = slotIds.firstIndex(of: item.bindingId) {
                    let slot = store.fourAppSlots.slots.indices.contains(slotIdx) ? store.fourAppSlots.slots[slotIdx] : nil
                    if let slot, (isSlotAttentionNeeded(slot, in: store) || (glowOnWorkingSessions && isSlotWorking(slot))) {
                        return item
                    }
                    return PerKeyLightingFeedback(bindingId: item.bindingId, color: "#000000", phase: .idle, label: item.label)
                }
                if item.bindingId == "AG06" && hasAnyAttentionOrCompleted {
                    let attnColor = (phase == .awaitingInput) ? "#FFAA00" : "#00FF66"
                    return PerKeyLightingFeedback(bindingId: "AG06", color: attnColor, phase: phase, label: item.label)
                }
                if (item.bindingId == "AG10" || item.bindingId == "AG11") && dictationActive {
                    return item
                }
                return PerKeyLightingFeedback(bindingId: item.bindingId, color: "#000000", phase: .idle, label: item.label)
            }
        }
        self.fullKeypadFeedback = fullKeys

        let perKeyColorOverride: String? = !lightingEnabled ? "#000000" : nil
        let payloadObject: [String: Any] = [
            "backlight": [
                "effect": dualZone.backlightEffect,
                "color": dualZone.backlightColor,
                "brightness": dualZone.backlightBrightness,
                "speed": dualZone.backlightSpeed,
                "magic": 0.5
            ],
            "underglow": [
                "effect": dualZone.underglowEffect,
                "color": dualZone.underglowColor,
                "brightness": dualZone.underglowBrightness,
                "speed": dualZone.underglowSpeed,
                "magic": 0.5
            ],
            "per_key": fullKeys.map { item -> [String: Any] in
                let hex = perKeyColorOverride ?? item.color
                let isLit = lightingEnabled && hex != "#000000" && hex != "#222222"
                let keyBrightness = isLit ? max(0.85 * clampedScale, 0.35) : 0.0
                let keyEffect: Int = {
                    guard isLit else { return 0 }
                    if item.phase == .completed || item.phase == .awaitingInput || item.phase == .error || item.phase == .thinking {
                        return 4 // breathing glow for attention/completed/working keys
                    }
                    return 1
                }()
                return [
                    "k": item.bindingId,
                    "color": isLit ? hex : "#000000",
                    "brightness": keyBrightness,
                    "e": keyEffect,
                    "s": 0.65
                ]
            }
        ]
        if let payloadData = try? JSONSerialization.data(withJSONObject: payloadObject),
           let payloadString = String(data: payloadData, encoding: .utf8) {
            self.lastHardwareSyncJSON = payloadString
            if let deviceID = store.connectedID {
                if payloadString != self.lastSyncedHardwarePayload || deviceID != self.lastSyncedDeviceID || !self.hardwareSynced {
                    if let response = try? await store.backend.syncFourAppHardwareLighting(payloadJson: payloadString) {
                        self.lastHardwareSyncResponse = response
                        if response.contains("\"synced\":true") {
                            self.lastSyncedHardwarePayload = payloadString
                            self.lastSyncedDeviceID = deviceID
                            self.hardwareSynced = true
                        } else {
                            self.hardwareSynced = false
                        }
                    } else {
                        self.hardwareSynced = false
                    }
                }
            } else {
                self.lastSyncedHardwarePayload = nil
                self.lastSyncedDeviceID = nil
                self.hardwareSynced = false
            }
        }

        if let token = store.lightingPreviewToken {
            do {
                try store.backend.updateLightingPreview(token: token, zone: "backlight", field: "color", value: dualZone.backlightColor)
                try store.backend.updateLightingPreview(token: token, zone: "backlight", field: "effect", value: dualZone.backlightEffect)
                try store.backend.updateLightingPreview(token: token, zone: "underglow", field: "color", value: dualZone.underglowColor)
                lastUpdateResult = .applied
                return .applied
            } catch {
                lastUpdateResult = .deviceBusy
                return .deviceBusy
            }
        }

        lastUpdateResult = .applied
        return .applied
    }

    /// Explicitly releases any runtime lighting and tokens.
    func release(in store: Store) async {
        activeRuntimeToken = nil
        dualZoneFeedback = nil
        lastSyncedHardwarePayload = nil
        lastSyncedDeviceID = nil
        hardwareSynced = false
        lastUpdateResult = .released
    }

    func forceSyncNow(in store: Store) async {
        lastSyncedHardwarePayload = nil
        hardwareSynced = false
        _ = await updateFeedback(in: store)
    }

    func triggerManualPreview(_ phase: FourAppAgentPhase?, in store: Store) async {
        manualPreviewPhase = phase
        if phase != nil && !lightingEnabled {
            lightingEnabled = true
        }
        await forceSyncNow(in: store)
    }

    func setLightingEnabled(_ enabled: Bool, in store: Store) async {
        lightingEnabled = enabled
        if !enabled {
            manualPreviewPhase = nil
        }
        await forceSyncNow(in: store)
    }

    func setSessionLightingLinked(_ linked: Bool, in store: Store) async {
        sessionLightingLinked = linked
        if linked {
            manualPreviewPhase = nil
        }
        await forceSyncNow(in: store)
    }

    func setBrightnessScale(_ scale: Double, in store: Store) async {
        brightnessScale = min(max(scale, 0.15), 1.0)
        if !lightingEnabled {
            lightingEnabled = true
        }
        await forceSyncNow(in: store)
    }

    func setIdleColor(_ hex: String, in store: Store) async {
        idleColorHex = hex
        if !lightingEnabled {
            lightingEnabled = true
        }
        await forceSyncNow(in: store)
    }
}
