import Foundation
import SwiftUI
import MicroInputCore

/// Real semantic status of a coding agent application session.
enum FourAppAgentPhase: String, Codable, Equatable, Sendable {
    case idle = "idle"
    case thinking = "thinking"
    case awaitingInput = "awaiting_input"
    case error = "error"
    case backgroundActive = "background_active"

    var label: String {
        switch self {
        case .idle: return "空闲/就绪"
        case .thinking: return "生成中/思考中"
        case .awaitingInput: return "待输入/待审批"
        case .error: return "异常/失败"
        case .backgroundActive: return "后台任务运行中"
        }
    }

    var symbol: String {
        switch self {
        case .idle: return "checkmark.circle"
        case .thinking: return "ellipsis.circle"
        case .awaitingInput: return "exclamationmark.circle"
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

    init() {}

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
            if let selected = store.antigravityStatus.selected,
               let row = store.antigravityStatus.rows.first(where: { $0.conversationId == selected.conversationId && $0.workspacePath == selected.workspacePath }) {
                if row.pendingInteraction != nil {
                    return .awaitingInput
                }
                switch row.phase {
                case .running, .runningTool: return .thinking
                case .backgroundActive: return .backgroundActive
                case .failed: return .error
                default: return .idle
                }
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
                if slotStatus == "running" || slotStatus == "generating" || slotStatus == "busy" {
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
        hasAttention: Bool
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

    /// Update lighting with single-sender invariant checks.
    func updateFeedback(in store: Store) async -> UpdateResult {
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

        let app = store.activeDesktopAgentAdapter() ?? "antigravity"
        let phase = deriveState(for: app, in: store)
        self.activePhase = phase

        let dictationActive = store.fourAppVoice.dictationState.isRecording || store.pushToTalk.isActive
        let dualZone = deriveDualZoneFeedback(
            phase: phase,
            dictationActive: dictationActive,
            nativeOwnershipActive: false
        )
        self.dualZoneFeedback = dualZone
        self.perKeyFeedback = derivePerKeyFeedback(
            slots: store.fourAppSlots.slots,
            dictationActive: dictationActive
        )
        let hasAttention = store.highestPriorityFourAppTarget() != nil
        let fullKeys = deriveFullKeypadFeedback(
            slots: store.fourAppSlots.slots,
            dictationActive: dictationActive,
            activePhase: phase,
            hasAttention: hasAttention
        )
        self.fullKeypadFeedback = fullKeys

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
            "per_key": fullKeys.map { ["k": $0.bindingId, "color": $0.color] }
        ]
        if let payloadData = try? JSONSerialization.data(withJSONObject: payloadObject),
           let payloadString = String(data: payloadData, encoding: .utf8) {
            self.lastHardwareSyncJSON = payloadString
            if store.connectedID != nil {
                _ = try? await store.backend.syncFourAppHardwareLighting(payloadJson: payloadString)
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
        lastUpdateResult = .released
    }
}
