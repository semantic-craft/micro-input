import SwiftUI
import MicroInputCore

struct DeviceOperationCapabilities: Codable {
    var apply = false
    var restore = false
    var preview = false
    var reason = "原生设备变更能力关闭"
}
struct DeviceOperationState: Decodable, Identifiable {
    let operation_id: String?
    let phase: String
    let running: Bool?
    let can_retry_local: Bool?
    let local_saved: Bool?
    var restores: String? = nil
    var id: String { operation_id ?? "not-sent" }
    var title: String {
        switch phase {
        case "Running": return "正在执行并核对"
        case "Verified": return "配置文件已核对"
        case "VerifiedLocalSaveFailed": return "配置已核对 · 本地保存待修复"
        case "OriginalVerified": return "读回仍为原配置"
        case "NotSent": return "未发送设备变更"
        case "DeviceStateUnknown": return "设备状态未知 · 请先核对"
        default: return "操作待核对"
        }
    }
}

extension Store {
    func refreshOperationCapabilities() {
        operationCapabilities = (try? JSONDecoder().decode(DeviceOperationCapabilities.self, from: Data(backend.deviceOperationCapabilities().utf8))) ?? DeviceOperationCapabilities()
    }

    func activeDesktopAgentAdapter(for profileId: String? = nil) -> String? {
        let targetProfileID = (profileId?.isEmpty == false) ? profileId : hostActiveProfileId
        if let targetProfileID,
           let matchedProfile = hostProfiles.first(where: { $0.id.text == targetProfileID }),
           let adapter = DesktopAgent.adapterForName(matchedProfile.name) {
            return adapter
        }
        if let idx = navigationHistory.index, navigationHistory.entries.indices.contains(idx) {
            for place in navigationHistory.entries[0...idx].reversed() {
                if case .app(let bundleID) = place,
                   let match = DesktopAgentControls.bundles.first(where: { $0.value == bundleID }) {
                    return match.key
                }
            }
        }
        return nil
    }

    @discardableResult
    func executeHostAction(actionJSON: String, bindingId: String, profileId: String, zcodeSelection: ZCodeSelection? = nil, zcodeCommand: ZCodeQueuedCommand? = nil, dshSelection: DSHSelection? = nil, dshCommand: DSHQueuedCommand? = nil, cindySelection: CindySelection? = nil, cindyCommand: CindyQueuedCommand? = nil, antigravitySelection: AntigravitySelection? = nil, antigravityCommand: AntigravityQueuedCommand? = nil, stillAdmitted: (() -> Bool)? = nil) async -> HostActionReceipt? {
        guard nativeOwnership.allowsExecution, !hostOperationPaused, let lease = try? backend.beginHostExecution() else { return nil }
        let epoch = hostRouteEpoch
        inFlightHostActions += 1
        defer { inFlightHostActions -= 1; backend.finishHostExecution(lease: lease) }
        let receipt: HostActionReceipt
        var effectiveJSON = actionJSON
        var payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(actionJSON.utf8))
        if let decoded = payload,
           decoded.action_type == "agent_command",
           decoded.adapter == "host",
           let cmd = decoded.command,
           (cmd == "submit" || cmd == "focus-input" || cmd == "approve" || cmd == "deny" || cmd == "new-or-stop" || cmd == "stop" || cmd.hasPrefix("prompt:") || cmd.hasPrefix("submit-prompt:")),
           let resolvedAdapter = activeDesktopAgentAdapter(for: profileId) {
            let rewritten = ResolvedHostActionPayload(
                action_type: decoded.action_type,
                key: decoded.key,
                key_code: decoded.key_code,
                modifiers: decoded.modifiers,
                target_bundle_id: DesktopAgentControls.bundles[resolvedAdapter] ?? decoded.target_bundle_id,
                text: decoded.text,
                bundle_id: decoded.bundle_id,
                url: decoded.url,
                working_directory: decoded.working_directory,
                adapter: resolvedAdapter,
                command: decoded.command,
                session: decoded.session,
                events: decoded.events,
                no_newline: decoded.no_newline,
                submit: decoded.submit
            )
            if let encoded = try? JSONEncoder().encode(rewritten) {
                effectiveJSON = String(decoding: encoded, as: UTF8.self)
                payload = rewritten
            }
        }
        if payload?.action_type == "agent_command",
           let command = payload?.command,
           ["back", "navigate-back", "forward", "navigate-forward"].contains(command),
           let adapter = payload?.adapter,
           ["navigation", "host", "cindy", "zcode", "qwenwork", "codex", "antigravity", "dsh-desktop", "herdr"].contains(adapter) {
            let isBack = command == "back" || command == "navigate-back"
            let destination = isBack
                ? navigationHistory.back(available: { [weak self] place in self?.isNavigationPlaceAvailable(place) == true })
                : navigationHistory.forward(available: { [weak self] place in self?.isNavigationPlaceAvailable(place) == true })
            receipt = await replayNavigationPlace(destination, isBack: isBack, bindingId: bindingId, profileId: profileId, epoch: epoch, stillAdmitted: stillAdmitted)
        } else if payload?.action_type == "agent_command", payload?.adapter == "zcode", payload?.command == "open-session" {
            let result = await zcodeSessions.open(zcodeSelection, admitted: { [weak self] in
                guard let self else { return false }
                return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
            })
            if result == .confirmed, let selection = zcodeSelection {
                navigationHistory.visit(.zcode(
                    sourceServer: selection.address.sourceServerId ?? "",
                    remoteSession: selection.address.remoteSessionId ?? "",
                    workspace: selection.address.workspacePath,
                    taskId: selection.address.taskId
                ))
            }
            receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: profileId,
                bindingId: bindingId, actionType: "zcode_open", target: "zcode", textByteLen: nil,
                outcome: result.rawValue, detail: result.label)
            AuditLogWriter.append(receipt)
        } else if payload?.action_type == "agent_command", payload?.adapter == "zcode",
                  let cmd = payload?.command,
                  ["create-session", "send-text", "stop-session", "resolve-interaction", "switch-model", "query-command"].contains(cmd) {
            let result = await zcodeSessions.execute(zcodeCommand, admitted: { [weak self] in
                guard let self else { return false }
                return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
            })
            if cmd == "create-session", result == .confirmed, let selected = zcodeSessions.selected {
                navigationHistory.visit(.zcode(
                    sourceServer: selected.sourceServerId ?? "",
                    remoteSession: selected.remoteSessionId ?? "",
                    workspace: selected.workspacePath,
                    taskId: selected.taskId
                ))
            }
            let actionType: String
            switch cmd {
            case "create-session": actionType = "zcode_create"
            case "send-text": actionType = "zcode_send"
            case "stop-session": actionType = "zcode_stop"
            case "resolve-interaction": actionType = "zcode_resolve"
            case "switch-model": actionType = "zcode_switch_model"
            default: actionType = "zcode_query"
            }
            receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: profileId,
                bindingId: bindingId, actionType: actionType, target: "zcode", textByteLen: nil,
                outcome: result.rawValue, detail: result.label)
            AuditLogWriter.append(receipt)
        } else if payload?.action_type == "agent_command", payload?.adapter == "dsh-desktop", payload?.command == "open-session" {
            let result = await dshSessions.open(dshSelection, admitted: { [weak self] in
                guard let self else { return false }
                return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
            })
            if result == .confirmed, let selection = dshSelection {
                navigationHistory.visit(.dsh(
                    sessionId: selection.sessionId,
                    workspaceId: selection.workspaceId,
                    hostId: selection.hostId
                ))
            }
            receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: profileId,
                bindingId: bindingId, actionType: "dsh_open", target: "dsh", textByteLen: nil,
                outcome: result.rawValue, detail: result.label)
            AuditLogWriter.append(receipt)
        } else if payload?.action_type == "agent_command", payload?.adapter == "dsh-desktop",
                  let cmd = payload?.command,
                  ["create-session", "prompt-session", "cancel-session", "query-command", "resolve-interaction", "switch-model"].contains(cmd) {
            let result = await dshSessions.execute(dshCommand, admitted: { [weak self] in
                guard let self else { return false }
                return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
            })
            let actionType: String
            switch cmd {
            case "create-session": actionType = "dsh_create"
            case "prompt-session": actionType = "dsh_prompt"
            case "cancel-session": actionType = "dsh_cancel"
            case "resolve-interaction": actionType = "dsh_resolve"
            case "switch-model": actionType = "dsh_switch_model"
            default: actionType = "dsh_query"
            }
            receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: profileId,
                bindingId: bindingId, actionType: actionType, target: "dsh", textByteLen: nil,
                outcome: result.rawValue, detail: result.label)
            AuditLogWriter.append(receipt)
        } else if payload?.action_type == "agent_command", payload?.adapter == "cindy", payload?.command == "open-session" {
            let result = await cindySessions.open(cindySelection, admitted: { [weak self] in
                guard let self else { return false }
                return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
            })
            if result == .confirmed, let selection = cindySelection {
                navigationHistory.visit(.cindy(
                    workspacePath: selection.address.workspacePath,
                    sessionId: selection.address.sessionId
                ))
            }
            receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: profileId,
                bindingId: bindingId, actionType: "cindy_open", target: "cindy", textByteLen: nil,
                outcome: result.rawValue, detail: result.label)
            AuditLogWriter.append(receipt)
        } else if payload?.action_type == "agent_command", payload?.adapter == "cindy",
                  let cmd = payload?.command,
                  ["create-session", "send-text", "stop-session", "resolve-interaction", "query-command", "next-attention"].contains(cmd) {
            let effectiveCmd = cindyCommand ?? (cmd == "next-attention" ? .nextAttention : nil)
            let reply: CindyCommandReply
            if let effectiveCmd {
                reply = await cindySessions.execute(effectiveCmd, admitted: { [weak self] in
                    guard let self else { return false }
                    return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
                })
            } else {
                reply = CindyCommandReply(commandId: "missing", instanceId: "", generation: 0, ownerEpoch: "", status: .stale, reasonCode: "guard.missingCommand")
            }
            if reply.status == .confirmed, let selected = reply.selected {
                navigationHistory.visit(.cindy(
                    workspacePath: selected.workspacePath,
                    sessionId: selected.sessionId
                ))
            }
            let actionType: String
            switch cmd {
            case "create-session": actionType = "cindy_create"
            case "send-text": actionType = "cindy_send"
            case "stop-session": actionType = "cindy_stop"
            case "resolve-interaction": actionType = "cindy_resolve"
            case "next-attention": actionType = "cindy_next_attention"
            default: actionType = "cindy_query"
            }
            receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: profileId,
                bindingId: bindingId, actionType: actionType, target: "cindy", textByteLen: nil,
                outcome: reply.status.rawValue, detail: reply.status.label)
            AuditLogWriter.append(receipt)
        } else if payload?.action_type == "agent_command", payload?.adapter == "antigravity", payload?.command == "open-session" {
            let result = await antigravityStatus.open(antigravitySelection, admitted: { [weak self] in
                guard let self else { return false }
                return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
            })
            if result == .confirmed, let selected = antigravitySelection {
                let place = KeypadNavigationPlace.antigravity(
                    conversationId: selected.key.conversationId,
                    workspacePath: selected.key.workspacePath
                )
                navigationHistory.visit(place)
                if !(actionExecutor is RecordingHostActionExecutor),
                   ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] == nil {
                    let rowTitle = antigravityStatus.row(for: selected.key)?.title ?? ""
                    _ = await DesktopAgentControls.selectAntigravityConversation(
                        title: rowTitle,
                        workspacePath: selected.key.workspacePath
                    )
                    fourAppLighting.acknowledgeSlot(place: place, in: self)
                    _ = await fourAppLighting.updateFeedback(in: self)
                }
            }
            receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: profileId,
                bindingId: bindingId, actionType: "antigravity_open", target: "antigravity", textByteLen: nil,
                outcome: result.rawValue, detail: result.label)
            AuditLogWriter.append(receipt)
        } else if payload?.action_type == "agent_command", payload?.adapter == "antigravity",
                  let cmd = payload?.command,
                  ["create-conversation", "send-message", "query-command", "stop-conversation", "resolve-interaction", "switch-model"].contains(cmd) {
            let result = await antigravityStatus.execute(antigravityCommand, admitted: { [weak self] in
                guard let self else { return false }
                return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
            })
            let actionType: String
            switch cmd {
            case "create-conversation": actionType = "antigravity_create"
            case "send-message": actionType = "antigravity_send"
            case "stop-conversation": actionType = "antigravity_stop"
            case "resolve-interaction": actionType = "antigravity_resolve"
            case "switch-model": actionType = "antigravity_switch_model"
            default: actionType = "antigravity_query"
            }
            receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: profileId,
                bindingId: bindingId, actionType: actionType, target: "antigravity", textByteLen: nil,
                outcome: result.rawValue, detail: result.label)
            AuditLogWriter.append(receipt)
        } else if payload?.action_type == "agent_command",
                  let cmd = payload?.command,
                  cmd.hasPrefix("slot-open:") {
            let indexStr = String(cmd.dropFirst("slot-open:".count))
            let slotIndex = Int(indexStr) ?? 0
            receipt = await fourAppSlots.executeSlot(
                slotIndex,
                in: self,
                profileId: profileId,
                bindingId: bindingId,
                admitted: { [weak self] in
                    guard let self else { return false }
                    return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
                }
            )
        } else if payload?.action_type == "agent_command",
                  let cmd = payload?.command,
                  let (filter, next) = AntigravitySessionFilter.from(command: cmd) {
            receipt = await cycleFilteredAntigravitySession(
                filter: filter,
                next: next,
                profileId: profileId,
                bindingId: bindingId,
                stillAdmitted: stillAdmitted
            )
        } else if payload?.action_type == "agent_command",
                  let cmd = payload?.command,
                  cmd == "four-app-attention" || cmd == "four-app-attention-wrap" {
            let allowWrap = cmd == "four-app-attention-wrap"
            receipt = await fourAppAttention.advanceAttentionRound(
                in: self,
                profileId: profileId,
                bindingId: bindingId,
                allowWrap: allowWrap,
                admitted: { [weak self] in
                    guard let self else { return false }
                    return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? false)
                }
            )
        } else if payload?.action_type == "agent_command",
                  let cmd = payload?.command,
                  cmd == "next-workspace" || cmd == "previous-workspace" {
            let (outcome, detail) = cmd == "next-workspace"
                ? workspaceNavigator.cycleNext(in: self)
                : workspaceNavigator.cyclePrevious(in: self)
            receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: cmd == "next-workspace" ? "next_workspace" : "previous_workspace",
                target: "workspace",
                textByteLen: nil,
                outcome: outcome,
                detail: detail
            )
            AuditLogWriter.append(receipt)
        } else if payload?.action_type == "agent_command",
                  let cmd = payload?.command,
                  ["voice-reply", "toggle-voice-reply"].contains(cmd),
                  let adapter = payload?.adapter {
            let (_, mode, detail) = await fourAppVoice.toggleVoiceReply(app: adapter, store: self)
            let outcome: String
            switch mode {
            case .supported(let enabled):
                outcome = enabled ? "voiceReplyEnabled" : "voiceReplyDisabled"
            case .unsupported:
                outcome = "unsupported"
            }
            receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: "\(adapter)_voice_reply",
                target: adapter,
                textByteLen: nil,
                outcome: outcome,
                detail: detail
            )
            AuditLogWriter.append(receipt)
        } else if payload?.action_type == "agent_command",
                  let cmd = payload?.command,
                  cmd == "stop" {
            let adapter = payload?.adapter ?? activeDesktopAgentAdapter(for: profileId) ?? ""
            receipt = await executeAgentStop(adapter: adapter, profileId: profileId, bindingId: bindingId, stillAdmitted: stillAdmitted)
        } else if payload?.action_type == "agent_command",
                  let cmd = payload?.command,
                  (cmd == "approve" || cmd == "deny") {
            let adapter = payload?.adapter ?? activeDesktopAgentAdapter(for: profileId) ?? ""
            receipt = await executeAgentApproveOrDeny(adapter: adapter, approve: cmd == "approve", profileId: profileId, bindingId: bindingId, stillAdmitted: stillAdmitted)
        } else if payload?.action_type == "agent_command",
                  let cmd = payload?.command,
                  cmd == "new-or-stop" {
            let adapter = payload?.adapter ?? activeDesktopAgentAdapter(for: profileId) ?? ""
            receipt = await executeAgentNewOrStop(adapter: adapter, profileId: profileId, bindingId: bindingId, stillAdmitted: stillAdmitted)
        } else {
            receipt = await actionExecutor.execute(actionJSON: effectiveJSON, bindingId: bindingId, profileId: profileId)
            if receipt.succeeded, let payload {
                recordNavigationVisitIfApplicable(payload)
            }
        }
        guard !Task.isCancelled, epoch == hostRouteEpoch, !hostOperationPaused else { return nil }
        lastHostActionReceipt = receipt
        return receipt
    }

    private func recordNavigationVisitIfApplicable(_ payload: ResolvedHostActionPayload) {
        switch payload.action_type {
        case "open_app":
            if let bundleID = payload.bundle_id, !bundleID.isEmpty {
                navigationHistory.visit(.app(bundleID: bundleID))
            }
        case "paste_text":
            if let targetBundle = payload.target_bundle_id, !targetBundle.isEmpty, targetBundle != "frontmost-editor" {
                navigationHistory.visit(.app(bundleID: targetBundle))
            } else if let adapter = payload.adapter, let bundleID = DesktopAgentControls.bundles[adapter] {
                navigationHistory.visit(.app(bundleID: bundleID))
            }
        case "focus_session":
            if let adapter = payload.adapter, let session = payload.session,
               !adapter.isEmpty, !session.isEmpty, session != "unselected" {
                navigationHistory.visit(.session(adapter: adapter, session: session))
            }
        case "agent_command":
            guard let adapter = payload.adapter, let cmd = payload.command else { return }
            if let bundleID = DesktopAgentControls.bundles[adapter], DialScrollRequest.from(command: cmd) == nil {
                navigationHistory.visit(.app(bundleID: bundleID))
            } else if adapter == "herdr" {
                navigationHistory.visit(.app(bundleID: "dev.herdr.app"))
            }
        case "shortcut":
            if let targetBundle = payload.target_bundle_id, !targetBundle.isEmpty, targetBundle != "frontmost-editor" {
                navigationHistory.visit(.app(bundleID: targetBundle))
            }
        default:
            break
        }
    }

    func executeAgentStop(adapter: String, profileId: String, bindingId: String, stillAdmitted: (() -> Bool)? = nil) async -> HostActionReceipt {
        let epoch = hostRouteEpoch
        let checkAdmitted: () -> Bool = { [weak self] in
            guard let self else { return false }
            return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? true)
        }
        let outcome: String
        let detail: String
        switch adapter {
        case "antigravity":
            if let selected = antigravityStatus.selected,
               let row = antigravityStatus.rows.first(where: { $0.key == selected }),
               (row.phase == .running || row.phase == .runningTool || row.phase == .backgroundActive),
               let cmd = antigravityStatus.stopIntent(for: selected, expectedInvocationNum: row.invocationNum, expectedExecutionNum: row.executionNum) {
                let rep = await antigravityStatus.execute(cmd, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "antigravity_stop: \(row.conversationId) (\(rep.label))"
            } else {
                outcome = "noop"
                detail = "guard.stopTargetChanged: 当前会话未在运行或无目标"
            }
        case "cindy":
            if let selected = cindySessions.selected,
               let row = cindySessions.rows.first(where: { $0.address == selected }),
               row.phase == "running", let runId = row.runId {
                let reply = await cindySessions.stop(address: selected, expectedRunId: runId, admitted: checkAdmitted)
                outcome = reply.status.rawValue
                detail = "cindy_stop: \(selected.sessionId) (\(reply.status.label))"
            } else {
                outcome = "noop"
                detail = "guard.stopTargetChanged: 当前会话未在运行或无目标"
            }
        case "zcode":
            if let selected = zcodeSessions.selected,
               let row = zcodeSessions.rows.first(where: { $0.address == selected }),
               (row.liveStatus == "busy" || row.liveStatus == "generating" || row.liveStatus == "running"),
               let intent = zcodeSessions.stopIntent(for: selected, expectedExecutionId: row.executionId) {
                let rep = await zcodeSessions.stop(intent, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "zcode_stop: \(selected.taskId) (\(rep.label))"
            } else {
                outcome = "noop"
                detail = "guard.stopTargetChanged: 当前会话未在运行或无目标"
            }
        case "dsh-desktop", "dsh":
            if let selected = dshSessions.selected,
               let row = dshSessions.rows.first(where: { $0.sessionId == selected }),
               row.running,
               let cmd = dshSessions.cancelIntent(for: selected, expectedTurnOverride: row.currentTurn) {
                let rep = await dshSessions.execute(cmd, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "dsh_cancel: \(selected) (\(rep.label))"
            } else {
                outcome = "noop"
                detail = "guard.stopTargetChanged: 当前会话未在运行或无目标"
            }
        default:
            outcome = "noop"
            detail = "未支持的停止适配器: \(adapter)"
        }
        let receipt = HostActionReceipt(
            timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
            profileId: profileId,
            bindingId: bindingId,
            actionType: "\(adapter)_stop",
            target: adapter,
            textByteLen: nil,
            outcome: outcome,
            detail: detail
        )
        AuditLogWriter.append(receipt)
        return receipt
    }

    func executeAgentApproveOrDeny(
        adapter: String,
        approve: Bool,
        profileId: String,
        bindingId: String,
        stillAdmitted: (() -> Bool)? = nil
    ) async -> HostActionReceipt {
        let epoch = hostRouteEpoch
        let checkAdmitted: () -> Bool = { [weak self] in
            guard let self else { return false }
            return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? true)
        }
        let effectiveAdapter: String = {
            if ["antigravity", "cindy", "zcode", "dsh-desktop", "dsh"].contains(adapter) {
                return adapter
            }
            if let highest = highestPriorityFourAppTarget() {
                switch highest.place {
                case .antigravity: return "antigravity"
                case .cindy: return "cindy"
                case .zcode: return "zcode"
                case .dsh: return "dsh-desktop"
                default: break
                }
            }
            return activeDesktopAgentAdapter(for: profileId) ?? "antigravity"
        }()
        let outcome: String
        let detail: String
        switch effectiveAdapter {
        case "antigravity":
            let targetRow = antigravityStatus.selected.flatMap { sel in
                antigravityStatus.rows.first(where: { $0.key == sel && $0.pendingInteraction != nil })
            } ?? antigravityStatus.rows.first(where: { $0.pendingInteraction != nil })
            if let row = targetRow, let pending = row.pendingInteraction {
                let decision: AntigravityInteractionDecision = approve
                    ? (pending.interactionType == .question && !pending.optionIds.isEmpty ? .answer : .allow)
                    : (pending.interactionType == .question ? .cancel : .deny)
                let selectedOpts = (decision == .answer) ? [pending.optionIds.first ?? ""].filter { !$0.isEmpty } : []
                if let cmd = antigravityStatus.resolveIntent(
                    for: row.key,
                    interactionId: pending.interactionId,
                    interactionType: pending.interactionType,
                    decision: decision,
                    selectedOptionIds: selectedOpts
                ) {
                    let rep = await antigravityStatus.execute(cmd, admitted: checkAdmitted)
                    outcome = rep.rawValue
                    detail = "antigravity_\(approve ? "approve" : "deny"): \(row.conversationId) (\(rep.label))"
                } else {
                    outcome = "noop"
                    detail = "guard.noPendingInteraction: 当前会话无待审批请求"
                }
            } else {
                outcome = "noop"
                detail = "guard.noPendingInteraction: 当前会话无待审批请求"
            }
        case "cindy":
            let targetRow = cindySessions.selected.flatMap { sel in
                cindySessions.rows.first(where: { $0.address == sel && $0.pendingInteraction != nil })
            } ?? cindySessions.rows.first(where: { $0.pendingInteraction != nil })
            if let row = targetRow, let pending = row.pendingInteraction {
                let isQuestion = pending.interactionType == .question
                let isPlan = pending.interactionType == .plan
                let decision: CindyInteractionDecision = approve
                    ? ((isQuestion || isPlan) ? .accept : .approve)
                    : (isQuestion ? .cancel : .reject)
                let optionId = isQuestion ? pending.options?.first : nil
                let rep = await cindySessions.resolve(
                    address: row.address,
                    interactionId: pending.interactionId,
                    interactionType: pending.interactionType,
                    decision: decision,
                    optionId: optionId,
                    admitted: checkAdmitted
                )
                outcome = rep.status.rawValue
                detail = "cindy_\(approve ? "approve" : "deny"): \(row.address.sessionId) (\(rep.status.label))"
            } else {
                outcome = "noop"
                detail = "guard.noPendingInteraction: 当前会话无待审批请求"
            }
        case "zcode":
            let targetRow = zcodeSessions.selected.flatMap { sel in
                zcodeSessions.rows.first(where: { $0.address == sel && $0.pendingInteraction != nil })
            } ?? zcodeSessions.rows.first(where: { $0.pendingInteraction != nil })
            if let row = targetRow, let pending = row.pendingInteraction {
                let isQuestion = pending.interactionType == .question
                let isPlan = pending.interactionType == .plan
                let decision: ZCodeInteractionDecision = approve
                    ? ((isQuestion || isPlan) ? .accept : .approve)
                    : (isQuestion ? .cancel : .reject)
                let optionId = isQuestion ? pending.options?.first : nil
                if let intent = zcodeSessions.resolveIntent(
                    for: row.address,
                    interactionId: pending.interactionId,
                    interactionType: pending.interactionType,
                    decision: decision,
                    optionId: optionId
                ) {
                    let rep = await zcodeSessions.resolve(intent, admitted: checkAdmitted)
                    outcome = rep.rawValue
                    detail = "zcode_\(approve ? "approve" : "deny"): \(row.address.taskId) (\(rep.label))"
                } else {
                    outcome = "noop"
                    detail = "guard.noPendingInteraction: 当前会话无待审批请求"
                }
            } else {
                outcome = "noop"
                detail = "guard.noPendingInteraction: 当前会话无待审批请求"
            }
        case "dsh-desktop", "dsh":
            let targetRow = dshSessions.selected.flatMap { sel in
                dshSessions.rows.first(where: { $0.sessionId == sel && $0.pendingInteraction != nil })
            } ?? dshSessions.rows.first(where: { $0.pendingInteraction != nil })
            if let row = targetRow, let pending = row.pendingInteraction {
                let isQuestion = pending.interactionType == .question
                let decision: DSHInteractionDecision = approve ? .approve : (isQuestion ? .cancel : .reject)
                let optionId = isQuestion ? pending.options?.first : nil
                if let cmd = dshSessions.resolveIntent(
                    for: row.sessionId,
                    interactionId: pending.interactionId,
                    interactionType: pending.interactionType,
                    decision: decision,
                    optionId: optionId
                ) {
                    let rep = await dshSessions.execute(cmd, admitted: checkAdmitted)
                    outcome = rep.rawValue
                    detail = "dsh_\(approve ? "approve" : "deny"): \(row.sessionId) (\(rep.label))"
                } else {
                    outcome = "noop"
                    detail = "guard.noPendingInteraction: 当前会话无待审批请求"
                }
            } else {
                outcome = "noop"
                detail = "guard.noPendingInteraction: 当前会话无待审批请求"
            }
        default:
            outcome = "noop"
            detail = "未支持的审批适配器: \(effectiveAdapter)"
        }
        let receipt = HostActionReceipt(
            timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
            profileId: profileId,
            bindingId: bindingId,
            actionType: "\(effectiveAdapter)_\(approve ? "approve" : "deny")",
            target: effectiveAdapter,
            textByteLen: nil,
            outcome: outcome,
            detail: detail
        )
        AuditLogWriter.append(receipt)
        return receipt
    }

    func executeAgentNewOrStop(
        adapter: String,
        profileId: String,
        bindingId: String,
        stillAdmitted: (() -> Bool)? = nil
    ) async -> HostActionReceipt {
        let effectiveAdapter = ["antigravity", "cindy", "zcode", "dsh-desktop", "dsh"].contains(adapter)
            ? adapter
            : (activeDesktopAgentAdapter(for: profileId) ?? "antigravity")
        let phase = fourAppLighting.deriveState(for: effectiveAdapter, in: self)
        if phase == .thinking {
            return await executeAgentStop(adapter: effectiveAdapter, profileId: profileId, bindingId: bindingId, stillAdmitted: stillAdmitted)
        }
        let epoch = hostRouteEpoch
        let checkAdmitted: () -> Bool = { [weak self] in
            guard let self else { return false }
            return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? true)
        }
        var outcome: String
        var detail: String
        switch effectiveAdapter {
        case "antigravity":
            let workspacePath = antigravityStatus.selected?.workspacePath
                ?? antigravityStatus.rows.first?.workspacePath
                ?? FileManager.default.currentDirectoryPath
            let projectId = antigravityStatus.rows.first(where: { $0.workspacePath == workspacePath && !$0.projectId.isEmpty })?.projectId
                ?? "default-project"
            if let cmd = antigravityStatus.createIntent(
                workspacePath: workspacePath,
                projectId: projectId,
                prompt: "New session"
            ) {
                let rep = await antigravityStatus.execute(cmd, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "antigravity_create: \(workspacePath) (\(rep.label))"
            } else {
                outcome = "noop"
                detail = "guard.createUnavailable: Antigravity 尚未就绪或无可用工作区"
            }
        case "cindy":
            if let scope = cindySessions.scopes.first {
                let rep = await cindySessions.create(scope: scope, text: nil, admitted: checkAdmitted)
                outcome = rep.status.rawValue
                detail = "cindy_create: \(scope.workspacePath) (\(rep.status.label))"
            } else {
                outcome = "noop"
                detail = "guard.createUnavailable: Cindy 尚未就绪或无授权工作区"
            }
        case "zcode":
            if let scope = zcodeSessions.defaultScope,
               let intent = zcodeSessions.createIntent(for: scope) {
                let rep = await zcodeSessions.create(intent, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "zcode_create: \(scope.workspacePath) (\(rep.label))"
            } else {
                outcome = "noop"
                detail = "guard.createUnavailable: ZCode 尚未就绪或无授权工作区"
            }
        case "dsh-desktop", "dsh":
            let workspaceId = dshSessions.selected.flatMap { sel in
                dshSessions.rows.first(where: { $0.sessionId == sel })?.workspaceId
            } ?? dshSessions.rows.first?.workspaceId ?? "default-workspace"
            if let cmd = dshSessions.createIntent(workspaceId: workspaceId) {
                let rep = await dshSessions.execute(cmd, admitted: checkAdmitted)
                outcome = rep.rawValue
                detail = "dsh_create: \(workspaceId) (\(rep.label))"
            } else {
                outcome = "noop"
                detail = "guard.createUnavailable: DSH 尚未就绪或无可用工作区"
            }
        default:
            outcome = "noop"
            detail = "未支持的新建会话适配器: \(effectiveAdapter)"
        }
        if outcome == "noop",
           !(actionExecutor is RecordingHostActionExecutor),
           ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] == nil,
           ["antigravity", "cindy", "zcode", "dsh-desktop", "dsh"].contains(effectiveAdapter) {
            let (ok, uiDetail) = await DesktopAgentControls.perform(adapter: effectiveAdapter, command: "new-chat", revealTools: false)
            if ok {
                outcome = "confirmed"
                detail = "\(effectiveAdapter)_create_ui: \(uiDetail)"
            }
        }
        let receipt = HostActionReceipt(
            timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
            profileId: profileId,
            bindingId: bindingId,
            actionType: "\(effectiveAdapter)_create",
            target: effectiveAdapter,
            textByteLen: nil,
            outcome: outcome,
            detail: detail
        )
        AuditLogWriter.append(receipt)
        return receipt
    }

    private func replayNavigationPlace(
        _ destination: KeypadNavigationPlace?,
        isBack: Bool,
        bindingId: String,
        profileId: String,
        epoch: Int,
        stillAdmitted: (() -> Bool)?
    ) async -> HostActionReceipt {
        let actionType = isBack ? "navigate_back" : "navigate_forward"
        guard let destination else {
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: actionType,
                target: "navigation",
                textByteLen: nil,
                outcome: "blocked",
                detail: isBack ? "没有可后退的打开历史" : "没有可前进的打开历史"
            )
            AuditLogWriter.append(receipt)
            return receipt
        }
        switch destination {
        case .app(let bundleID):
            if let data = try? JSONSerialization.data(withJSONObject: ["action_type": "open_app", "bundle_id": bundleID]),
               let replayJSON = String(data: data, encoding: .utf8) {
                return await actionExecutor.execute(actionJSON: replayJSON, bindingId: bindingId, profileId: profileId)
            }
        case .session(let adapter, let session):
            if let data = try? JSONSerialization.data(withJSONObject: ["action_type": "focus_session", "adapter": adapter, "session": session]),
               let replayJSON = String(data: data, encoding: .utf8) {
                return await actionExecutor.execute(actionJSON: replayJSON, bindingId: bindingId, profileId: profileId)
            }
        case .antigravity(let conversationId, let workspacePath):
            let key = AntigravitySessionKey(conversationId: conversationId, workspacePath: workspacePath)
            let result: AntigravityOpenResult
            if let selection = antigravityStatus.selection(for: key) {
                result = await antigravityStatus.open(selection, admitted: { [weak self] in
                    guard let self else { return false }
                    return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? true)
                })
            } else {
                result = .unconfirmed
            }
            let success = result == .confirmed
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: actionType,
                target: "antigravity",
                textByteLen: nil,
                outcome: result.rawValue,
                detail: success ? (isBack ? "已后退到 Antigravity 会话" : "已前进到 Antigravity 会话") : "Antigravity 历史目标未确认"
            )
            AuditLogWriter.append(receipt)
            return receipt
        case .cindy(let workspacePath, let sessionId):
            let address = CindyTaskAddress(workspacePath: workspacePath, sessionId: sessionId)
            let result: CindyOpenResult
            if let selection = cindySessions.selection(for: address) {
                result = await cindySessions.open(selection, admitted: { [weak self] in
                    guard let self else { return false }
                    return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? true)
                })
            } else {
                result = .unconfirmed
            }
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: actionType,
                target: "cindy",
                textByteLen: nil,
                outcome: result.rawValue,
                detail: result == .confirmed ? (isBack ? "已后退到 Cindy 会话" : "已前进到 Cindy 会话") : "Cindy 历史目标未确认"
            )
            AuditLogWriter.append(receipt)
            return receipt
        case .zcode(let sourceServer, let remoteSession, let workspace, let taskId):
            let address = ZCodeTaskAddress(workspacePath: workspace, workspaceIdentity: nil, sourceServerId: sourceServer.isEmpty ? nil : sourceServer, remoteSessionId: remoteSession.isEmpty ? nil : remoteSession, taskId: taskId)
            let result: ZCodeOpenResult
            if let selection = zcodeSessions.selection(for: address) {
                result = await zcodeSessions.open(selection, admitted: { [weak self] in
                    guard let self else { return false }
                    return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? true)
                })
            } else {
                result = .unconfirmed
            }
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: actionType,
                target: "zcode",
                textByteLen: nil,
                outcome: result.rawValue,
                detail: result == .confirmed ? (isBack ? "已后退到 ZCode 会话" : "已前进到 ZCode 会话") : "ZCode 历史目标未确认"
            )
            AuditLogWriter.append(receipt)
            return receipt
        case .dsh(let sessionId, _, _):
            let result: DSHOpenResult
            if let selection = dshSessions.selection(for: sessionId) {
                result = await dshSessions.open(selection, admitted: { [weak self] in
                    guard let self else { return false }
                    return epoch == self.hostRouteEpoch && self.dispatchOpen && (stillAdmitted?() ?? true)
                })
            } else {
                result = .unconfirmed
            }
            let success = result == .confirmed
            let receipt = HostActionReceipt(
                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                profileId: profileId,
                bindingId: bindingId,
                actionType: actionType,
                target: "dsh",
                textByteLen: nil,
                outcome: result.rawValue,
                detail: success ? (isBack ? "已后退到 DSH 会话" : "已前进到 DSH 会话") : "DSH 历史目标未确认"
            )
            AuditLogWriter.append(receipt)
            return receipt
        }
        let receipt = HostActionReceipt(
            timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
            profileId: profileId,
            bindingId: bindingId,
            actionType: actionType,
            target: destination.receiptTarget,
            textByteLen: nil,
            outcome: "blocked",
            detail: "历史目标编码失败"
        )
        AuditLogWriter.append(receipt)
        return receipt
    }
    func refreshDeviceOperations() async {
        refreshOperationCapabilities()
        do { operationRows = try JSONDecoder().decode([DeviceOperationState].self, from: Data(try await backend.deviceOperations().utf8)) }
        catch { status = readableError(error) }
    }
    func performDeviceOperation(token: String, restore: Bool = false) async {
        guard nativeOwnership.allowsExecution else { status = nativeOwnership.summary; return }
        guard !deviceOperationRunning else { return }
        guard lightingPreviewToken == nil else { status = "请先结束灯光预览"; return }
        refreshOperationCapabilities()
        guard restore ? operationCapabilities.restore : operationCapabilities.apply else { status = operationCapabilities.reason; return }
        deviceOperationRunning = true
        deviceCancellationRequested = false
        hostOperationPaused = true
        cancelHostExecution()
        hostRouteEpoch += 1
        hostInputTask?.cancel(); hostInputTask = nil
        let connection = generation, document = draft.document_id
        var drain: String?
        defer { deviceOperationRunning = false }
        do {
            let ticket = try backend.beginDeviceDrain(); drain = ticket
            let deadline = Date().addingTimeInterval(drainTimeout)
            while inFlightHostActions > 0 || backend.hostExecutionsPending() > 0 {
                guard Date() < deadline, connection == generation, !Task.isCancelled, !deviceCancellationRequested else {
                    backend.abandonDeviceDrain(token: ticket); drain = nil
                    operationResult = DeviceOperationState(operation_id: nil, phase: "NotSent", running: false, can_retry_local: false, local_saved: false)
                    status = "宿主动作尚未排空 · 未发送设备变更"
                    return
                }
                try await Task.sleep(nanoseconds: 10_000_000)
            }
            guard pushToTalk.cleanupConfirmed else {
                backend.abandonDeviceDrain(token: ticket); drain = nil
                operationResult = DeviceOperationState(operation_id: nil, phase: "NotSent", running: false, can_retry_local: false, local_saved: false)
                hostOperationPaused = false
                readInputBoundary()
                if hostExecutionEnabled { startHostInputLoop(token: generation) }
                status = "原听写停止未确认；未发送设备变更"
                return
            }
            guard connection == generation, !Task.isCancelled, !deviceCancellationRequested else { throw BridgeError.Disconnected }
            let id = try await backend.startDeviceApply(token: token, drainToken: ticket, restore: restore)
            drain = nil
            if restore { restorePreview = nil }
            operationResult = DeviceOperationState(operation_id: id, phase: "Running", running: true, can_retry_local: false, local_saved: false)
            let until = Date().addingTimeInterval(125)
            while true {
                let result = try JSONDecoder().decode(DeviceOperationState.self, from: Data(try backend.deviceOperationStatus(operationId: id).utf8))
                operationResult = result
                if result.running != true { break }
                if Task.isCancelled || deviceCancellationRequested || Date() > until { backend.cancelDeviceOperation(operationId: id) }
                if Date() > until.addingTimeInterval(5) { status = "操作仍待核对 · 已请求停止"; return }
                // Cancellation stops the Rust operation; keep observing its bounded terminal result.
                await Task.detached { try? await Task.sleep(nanoseconds: 50_000_000) }.value
            }
            if draft.document_id == document { try applyDocument(try await backend.documentView()) }
            await refreshDeviceOperations()
            status = operationResult?.title ?? "操作结束"
            if connection == generation, ["Verified", "NotSent"].contains(operationResult?.phase ?? "") {
                hostOperationPaused = false
                refreshPassthroughHealth()
                if hostExecutionEnabled { startHostInputLoop(token: generation) }
            }
            recordDeviceOperationEvidence()
        } catch {
            if let drain { backend.abandonDeviceDrain(token: drain) }
            status = readableError(error)
        }
    }
    func cancelActiveDeviceOperation() {
        deviceCancellationRequested = true
        if let id = operationResult?.operation_id, deviceOperationRunning { backend.cancelDeviceOperation(operationId: id) }
    }
    func discardDeviceRestore() { backend.discardDeviceRestore(); restorePreview = nil }
    func prepareDeviceRestore(_ id: String) async {
        guard !deviceOperationRunning else { return }
        deviceOperationRunning = true
        defer { deviceOperationRunning = false }
        do { restorePreview = try JSONDecoder().decode(ApplyPreview.self, from: Data(try await backend.prepareDeviceRestore(operationId: id).utf8)) }
        catch { status = readableError(error) }
    }
    func checkDeviceOperation(_ id: String) async {
        guard !deviceOperationRunning else { return }
        let connection = generation
        deviceOperationRunning = true
        defer { deviceOperationRunning = false }
        do {
            operationResult = try JSONDecoder().decode(DeviceOperationState.self, from: Data(try await backend.checkDeviceOperation(operationId: id).utf8))
            try applyDocument(try await backend.documentView())
            await refreshDeviceOperations(); status = operationResult?.title ?? "核对完成"
            if connection == generation, ["Verified", "OriginalVerified", "VerifiedLocalSaveFailed"].contains(operationResult?.phase ?? "") {
                hostOperationPaused = false
                refreshPassthroughHealth()
                if hostExecutionEnabled { startHostInputLoop(token: generation) }
            }
        } catch { status = readableError(error) }
    }
    func repairLocalOperation(_ id: String) async {
        guard !deviceOperationRunning else { return }
        deviceOperationRunning = true
        defer { deviceOperationRunning = false }
        do { try applyDocument(try await backend.retryLocalSave(operationId: id)); await refreshDeviceOperations(); status = "本地保存已修复 · 没有发送设备请求" }
        catch { status = readableError(error) }
    }
    func recordDeviceOperationEvidence() {
        guard let result = operationResult,
              let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"],
              let data = try? JSONSerialization.data(withJSONObject: ["operation_id": result.id, "phase": result.phase, "config_readback": result.phase == "Verified" ? "PASS" : "NOT_VERIFIED", "physical_key": "NOT_RUN", "lighting_visual": "NOT_RUN"], options: [.sortedKeys]) else { return }
        let url = URL(fileURLWithPath: audit).deletingLastPathComponent().appendingPathComponent("device-operations.jsonl")
        if !FileManager.default.fileExists(atPath: url.path) { FileManager.default.createFile(atPath: url.path, contents: nil) }
        if let handle = try? FileHandle(forWritingTo: url) { defer { try? handle.close() }; _ = try? handle.seekToEnd(); try? handle.write(contentsOf: data + Data([10])) }
    }
    #if MI_SYNTHETIC_BACKEND
    func runSyntheticDeviceOperationScenario() async {
        guard let core = backend as? MicroInputCore.MicroInput else { return }
        do {
            await installCodingAgentPresets()
            await setHostActiveProfile(hostProfiles.first(where: { $0.name == "Antigravity" })?.id.text)
            await setHostExecutionEnabled(true)
            let hostID = draft.document_id
            let hostBytes = try Data(contentsOf: documentURL)
            let preferenceURL = documentURL.deletingPathExtension().appendingPathExtension("preferences.json")
            let preferences = try Data(contentsOf: preferenceURL)
            let originalRoute = try JSONDecoder().decode(HostRoutingState.self, from: Data(core.hostRoutingState().utf8))
            func resolves(_ binding: String) -> Bool {
                core.resolveHostInput(bindingId: binding, expectedDocumentId: originalRoute.document_id, expectedDocumentRevision: originalRoute.document_revision, expectedHostStateRevision: originalRoute.host_state_revision) != nil
            }
            try await core.setSyntheticDiscovery(state: .twoDevices)
            await refresh(); selectDevice("synthetic-interface-0")
            await editDeviceConfiguration()
            guard editingDevice, hostProfiles.contains(where: { $0.id.text == originalRoute.profile_id }), resolves("AG00") else { throw BridgeError.Failed(message: "设备编辑改变宿主路由") }
            slot = 0; await assign("device-key")
            slot = 1; await assign("device-key")
            await editLighting(zone: "backlight", field: "color", value: "#3377CC", allLayers: false)
            await previewApply()
            guard applyPreview?.changes?.count == 3 else { throw BridgeError.Failed(message: "混合变更摘要不完整") }
            await prepareApplyBackup()
            guard let token = applyPreview?.token else { throw BridgeError.Failed(message: status) }
            await performDeviceOperation(token: token)
            guard operationResult?.phase == "Verified", let id = operationResult?.operation_id,
                  !resolves("AG00"), !resolves("AG01"), resolves("AG02") else { throw BridgeError.Failed(message: "应用或逐绑定路由核对失败") }
            await prepareDeviceRestore(id)
            guard let restore = restorePreview else { throw BridgeError.Failed(message: status) }
            await performDeviceOperation(token: restore.token, restore: true)
            guard operationResult?.phase == "Verified", resolves("AG00") else { throw BridgeError.Failed(message: "恢复未核验或原绑定未恢复") }
            await returnToHostDocument()
            guard !editingDevice, draft.document_id == hostID,
                  try Data(contentsOf: documentURL) == hostBytes,
                  try Data(contentsOf: preferenceURL) == preferences else { throw BridgeError.Failed(message: "宿主草稿或偏好发生变化") }
            if let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"] {
                let payload = ["status":"PASS", "host_document":"UNCHANGED", "host_preferences":"UNCHANGED", "mixed_changes":"3", "routing":"PASS"]
                try JSONEncoder().encode(payload).write(to: URL(fileURLWithPath: audit).deletingLastPathComponent().appendingPathComponent("device-editor.json"))
            }
            showingDeviceOperations = true
        } catch { status = readableError(error) }
    }

    #endif
}

struct DeviceOperationsView: View {
    @ObservedObject var store: Store
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack { Text("设备操作与恢复").font(.title2); Spacer(); Button("关闭") { store.showingDeviceOperations = false } }
            Text("支持已核验的 Creator Micro 2 / 0.6.3 当前连接。配置读回与实体按键、灯光效果分别核验。")
            if let result = store.operationResult { Text(result.title).font(.headline) }
            if store.deviceOperationRunning { Button("停止后续发送") { store.cancelActiveDeviceOperation() } }
            ScrollView {
                ForEach(store.operationRows) { row in
                    VStack(alignment: .leading, spacing: 8) {
                        Text("\(row.restores == nil ? "应用配置" : "恢复配置") · \(row.title)").font(.headline)
                        Text(row.id).font(.caption.monospaced()).textSelection(.enabled)
                        HStack {
                            Button("只读核对当前设备") { Task { await store.checkDeviceOperation(row.id) } }.disabled(!store.operationCapabilities.restore)
                            Button("审阅恢复原配置…") { Task { await store.prepareDeviceRestore(row.id) } }.disabled(!store.operationCapabilities.restore)
                            if row.can_retry_local == true { Button("仅修复本地保存") { Task { await store.repairLocalOperation(row.id) } } }
                        }.disabled(store.deviceOperationRunning)
                    }.padding(.vertical, 8)
                }
            }
            if let preview = store.restorePreview {
                Divider()
                Text("恢复审阅").font(.headline)
                Text("将当前 keymap 替换为关联原副本；保留当前 Smart Actions。执行前再次核对现状，变化后需重新审阅。")
                Text("目标摘要：\(preview.planned_keymap_sha256)").font(.caption.monospaced())
                HStack {
                    Button("确认恢复") { Task { await store.performDeviceOperation(token: preview.token, restore: true); store.restorePreview = nil } }.disabled(store.deviceOperationRunning || !store.operationCapabilities.restore)
                    Button("取消恢复准备") { store.discardDeviceRestore() }.disabled(store.deviceOperationRunning)
                }
            }
            Text(store.status).font(.caption)
        }.padding(24).frame(width: 720, height: 540).task { await store.refreshDeviceOperations() }
    }
}
