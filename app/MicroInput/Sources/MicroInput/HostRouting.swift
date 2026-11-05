import Foundation
import AppKit
import MicroInputCore

struct HostRoutingState: Decodable {
    let document_id: String, document_revision: UInt64, host_state_revision: UInt64
    let profile_id: String?, profile_name: String?
    let execution_enabled: Bool, auto_follow_enabled: Bool, manual_override: Bool
    let command_execution_enabled: Bool
    let app_profile_links: [String: String]
    let keys: [HostKey]
}
struct HostKey: Decodable, Identifiable {
    let binding_id: String, control: String, label: String, available: Bool
    var id: String { binding_id }
}
struct InputWatermark: Decodable {
    let session_id: UInt64?, ingress_sequence: UInt64
}
struct ResolvedInput: Decodable {
    let profile_id: String, document_id: String, document_revision: UInt64, host_state_revision: UInt64
    let action: JSONValue
}

// Intake always advances physical state, including discarded, buffered pre-cutoff events.
struct HostInputAdmission {
    var pressed = false
    var radial = false
}
struct HostInputBoundary {
    var session: UInt64?
    var cutoff: UInt64 = 0
    var radialHeld = false
    var radialQuarantined = false
    var recovery = false
    var released: Set<String> = []
    var held: [String: UInt64] = [:]
    mutating func advance(_ watermark: InputWatermark) {
        if session != watermark.session_id { radialHeld = false; radialQuarantined = true }
        session = watermark.session_id; cutoff = watermark.ingress_sequence
        held.removeAll()
        radialQuarantined = radialQuarantined || radialHeld
    }
    mutating func consume(_ event: InputEventInfo, gateOpen: Bool) -> HostInputAdmission {
        guard session == event.sessionId else { return HostInputAdmission() }
        if let center = event.radialCenter {
            radialHeld = !center
            if center && event.ingressSequence > cutoff { radialQuarantined = false }
            else if !gateOpen || event.ingressSequence <= cutoff { radialQuarantined = true }
        }
        if event.kind == .released, let binding = event.bindingId { released.insert(binding); held.removeValue(forKey: binding) }
        if event.kind == .pressed, let binding = event.bindingId { held[binding] = event.ingressSequence }
        guard gateOpen, event.ingressSequence > cutoff else { return HostInputAdmission() }
        var admission = HostInputAdmission(radial: event.radialPosition != nil && !radialQuarantined)
        guard event.kind == .pressed, event.actionEligible else { return admission }
        if event.radialCenter != nil { admission.pressed = !radialQuarantined }
        else if recovery, event.actionRole != "encoder_step", let binding = event.bindingId { admission.pressed = released.contains(binding) }
        else { admission.pressed = true }
        return admission
    }
}
struct HostMutation {
    let automatic: Bool
    let run: () async -> (() -> Void)
    let cancel: () -> Void
}
struct PendingHostAction {
    let json: String, binding: String, profile: String
    let state: HostRoutingState
    let deviceSession: UInt64?
    var deviceIngress: UInt64? = nil
    let admitted: ContinuousClock.Instant
    let requiresEnabled: Bool
    let done: (() -> Void)?
    var zcodeSelection: ZCodeSelection? = nil
    var zcodeCommand: ZCodeQueuedCommand? = nil
    var dshSelection: DSHSelection? = nil
    var dshCommand: DSHQueuedCommand? = nil
    var cindySelection: CindySelection? = nil
    var antigravitySelection: AntigravitySelection? = nil
    var antigravityCommand: AntigravityQueuedCommand? = nil
    var t3Selection: T3Selection? = nil
    var radialIntent: RadialIntent? = nil
}

enum KeypadNavigationPlace: Equatable, Hashable, Codable {
    case app(bundleID: String)
    case session(adapter: String, session: String)
    case t3Thread(environmentID: String, threadID: String)

    var receiptTarget: String {
        switch self {
        case .app(let bundleID): return bundleID
        case .session(let adapter, let session): return "\(adapter):\(session)"
        case .t3Thread: return "t3"
        }
    }
}

struct KeypadNavigationHistory: Equatable {
    static let maxHistory = 50
    private(set) var entries: [KeypadNavigationPlace] = []
    private(set) var index: Int? = nil

    var current: KeypadNavigationPlace? {
        guard let index, entries.indices.contains(index) else { return nil }
        return entries[index]
    }

    mutating func visit(_ place: KeypadNavigationPlace) {
        if current == place { return }
        if let index {
            if index + 1 < entries.count {
                entries.removeSubrange((index + 1)..<entries.count)
            }
        } else {
            entries.removeAll()
        }
        entries.append(place)
        if entries.count > Self.maxHistory {
            entries.removeFirst(entries.count - Self.maxHistory)
        }
        index = entries.count - 1
    }

    mutating func back(available: (KeypadNavigationPlace) -> Bool = { _ in true }) -> KeypadNavigationPlace? {
        guard let current = index, current > 0 else { return nil }
        for candidate in stride(from: current - 1, through: 0, by: -1) {
            if available(entries[candidate]) {
                index = candidate
                return entries[candidate]
            }
        }
        return nil
    }

    mutating func forward(available: (KeypadNavigationPlace) -> Bool = { _ in true }) -> KeypadNavigationPlace? {
        guard let current = index, current + 1 < entries.count else { return nil }
        for candidate in (current + 1)..<entries.count {
            if available(entries[candidate]) {
                index = candidate
                return entries[candidate]
            }
        }
        return nil
    }

    func canGoBack(available: (KeypadNavigationPlace) -> Bool = { _ in true }) -> Bool {
        guard let current = index, current > 0 else { return false }
        return stride(from: current - 1, through: 0, by: -1).contains { available(entries[$0]) }
    }

    func canGoForward(available: (KeypadNavigationPlace) -> Bool = { _ in true }) -> Bool {
        guard let current = index, current + 1 < entries.count else { return false }
        return ((current + 1)..<entries.count).contains { available(entries[$0]) }
    }
}

extension Store {
    var routingMode: String {
        guard let routing else { return "配置尚未载入" }
        if routing.manual_override { return "手动保持" }
        return routing.auto_follow_enabled ? "自动跟随" : "自动跟随已关闭"
    }
    var dispatchOpen: Bool { nativeOwnership.allowsExecution && !hostOperationPaused && !mutationDraining && routingValid && !systemSleeping && !stopped && !overflowDraining }
    func refreshRouting() throws {
        let value = try JSONDecoder().decode(HostRoutingState.self, from: Data(backend.hostRoutingState().utf8))
        let changed = routing?.document_id != value.document_id || routing?.document_revision != value.document_revision || routing?.host_state_revision != value.host_state_revision
        if changed { nativeOwnership.invalidate("路由已换代，等待原生确认") }
        routing = value; hostActiveProfileId = value.profile_id
        hostExecutionEnabled = value.execution_enabled
        commandExecutionEnabled = value.command_execution_enabled
        routingValid = true
        nativeOwnership.setDesiredOwner(activeDesktopAgentAdapter(for: value.profile_id ?? "") == "cindy" ? "native" : "micro-input")
    }
    func readInputBoundary() {
        cancelRadialMenu()
        guard let watermark = try? JSONDecoder().decode(InputWatermark.self, from: Data(backend.hostInputWatermark().utf8)) else {
            inputBoundary.session = nil; return
        }
        inputBoundary.advance(watermark)
    }
    func clearPendingActions() {
        cancelRadialMenu()
        pushToTalk.cancel()
        let discarded = pendingHostActions; pendingHostActions.removeAll()
        discarded.forEach { $0.done?() }
    }
    @discardableResult
    func cancelHostExecution() -> Task<Void, Never>? {
        let execution = hostExecutionTask
        hostExecutionTask = nil
        hostExecutionGeneration += 1
        clearPendingActions()
        execution?.cancel()
        if overflowDraining { readInputBoundary(); overflowDraining = false }
        return execution
    }
    @discardableResult func invalidateHostWork() -> Task<Void, Never>? {
        observationSequence += 1
        foregroundTask?.cancel()
        let queued = hostMutations; hostMutations.removeAll()
        queued.forEach { $0.cancel() }
        let execution = cancelHostExecution(); overlay.hide()
        return execution
    }
    // Document edits and preference changes share this drain. Manual work precedes queued observations.
    func hostMutation<T>(automatic: Bool = false, _ operation: @escaping () async throws -> T) async throws -> T {
        guard !systemSleeping, !stopped else { throw CancellationError() }
        return try await withCheckedThrowingContinuation { continuation in
            let item = HostMutation(automatic: automatic, run: {
                do { let value = try await operation(); return { continuation.resume(returning: value) } }
                catch { return { continuation.resume(throwing: error) } }
            }, cancel: { continuation.resume(throwing: CancellationError()) })
            if automatic {
                let replaced = hostMutations.filter(\.automatic)
                hostMutations.removeAll(where: \.automatic)
                replaced.forEach { $0.cancel() }
                hostMutations.append(item)
            }
            else { hostMutations.insert(item, at: hostMutations.firstIndex(where: \.automatic) ?? hostMutations.count) }
            guard !mutationDraining else { return }
            mutationDraining = true; clearPendingActions()
            Task { @MainActor in
                await pushToTalk.waitUntilStopped()
                let before = routing?.profile_id
                var completions: [() -> Void] = []
                while !hostMutations.isEmpty {
                    let next = hostMutations.removeFirst()
                    completions.append(await next.run())
                    do { try refreshRouting() } catch { routingValid = false }
                    readInputBoundary()
                }
                mutationDraining = false
                if !systemSleeping, !stopped, routingValid, before != nil, before != routing?.profile_id, let name = routing?.profile_name {
                    overlay.showNotice(name: name, mode: routingMode)
                }
                overlay.refresh()
                completions.forEach { $0() }
            }
        }
    }
    func routingChange(_ operation: [String: Any], automatic: Bool = false, observation: UInt64? = nil) async {
        // Even a failed explicit choice supersedes older, not-yet-submitted app observations.
        if operation["kind"] as? String == "select" { observationSequence += 1 }
        do {
            try await hostMutation(automatic: automatic) { [self] in
                if let observation, observation != observationSequence { return }
                try refreshRouting()
                guard let state = routing else { return }
                if automatic && (!state.auto_follow_enabled || state.manual_override) { return }
                let request: [String: Any] = ["document_id": state.document_id, "document_revision": state.document_revision,
                    "host_state_revision": state.host_state_revision, "operation": operation]
                let data = try JSONSerialization.data(withJSONObject: request)
                _ = try await backend.updateHostRouting(requestJson: String(decoding: data, as: UTF8.self))
            }
            if !automatic { reevaluateForeground() }
        } catch is CancellationError { } catch { status = readableError(error) }
    }
    func foregroundChanged(bundleID: String?) {
        if let target = pushToTalk.targetBundle, bundleID != target { pushToTalk.cancel() }
        guard !systemSleeping, !stopped, let bundleID, bundleID != Bundle.main.bundleIdentifier else { return }
        externalBundleID = bundleID; observationSequence += 1
        guard let routing, routing.auto_follow_enabled, !routing.manual_override,
              let target = routing.app_profile_links[bundleID], hostProfiles.contains(where: { $0.id.text == target }) else { return }
        if !mutationDraining, target == routing.profile_id { return }
        let sequence = observationSequence
        foregroundTask = Task {
            await routingChange(["kind":"follow", "bundle_id":bundleID], automatic: true, observation: sequence)
            syncWorkbenchToActiveProfile()
        }
    }
    func reevaluateForeground() {
        foregroundChanged(bundleID: externalBundleID ?? NSWorkspace.shared.frontmostApplication?.bundleIdentifier)
    }
    func setAutoFollow(_ enabled: Bool) async { await routingChange(["kind":"enable", "enabled":enabled]) }
    func resumeFollowing() async { await routingChange(["kind":"resume"]) }
    func linkApp(_ bundleID: String, profileID: String) async { await routingChange(["kind":"upsert", "bundle_id":bundleID, "profile_id":profileID]) }
    func unlinkApp(_ bundleID: String) async { await routingChange(["kind":"remove", "bundle_id":bundleID]) }
    func resolveHostAction(_ binding: String) -> (String, String, HostRoutingState)? {
        guard dispatchOpen, let state = routing,
              let raw = backend.resolveHostInput(bindingId: binding, expectedDocumentId: state.document_id, expectedDocumentRevision: state.document_revision, expectedHostStateRevision: state.host_state_revision),
              let resolved = try? JSONDecoder().decode(ResolvedInput.self, from: Data(raw.utf8)),
              let data = try? JSONEncoder().encode(resolved.action) else { return nil }
        return (String(decoding: data, as: UTF8.self), resolved.profile_id, state)
    }
    @discardableResult func enqueueHostAction(json: String, binding: String, profile: String, state: HostRoutingState, deviceSession: UInt64? = nil, deviceIngress: UInt64? = nil, requiresEnabled: Bool = true, radialIntent: RadialIntent? = nil, zcodeSelection: ZCodeSelection? = nil, zcodeCommand: ZCodeQueuedCommand? = nil, dshSelection: DSHSelection? = nil, dshCommand: DSHQueuedCommand? = nil, cindySelection: CindySelection? = nil, antigravitySelection: AntigravitySelection? = nil, antigravityCommand: AntigravityQueuedCommand? = nil, done: (() -> Void)? = nil) -> Bool {
        guard dispatchOpen, !requiresEnabled || hostExecutionEnabled else { status = "正在切换或输入已暂停，请稍后重试"; done?(); return false }
        let payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(json.utf8))
        if radialIntent == nil,
           let payload, payload.action_type == "agent_command",
           let adapter = payload.adapter, let cmd = payload.command,
           let incomingScroll = DialScrollRequest.from(command: cmd) {
            if incomingScroll.mode == .bottom {
                let duplicate = pendingHostActions.contains { existing in
                    guard existing.profile == profile,
                          existing.state.document_id == state.document_id,
                          existing.state.document_revision == state.document_revision,
                          existing.state.host_state_revision == state.host_state_revision,
                          let existingPayload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(existing.json.utf8)),
                          existingPayload.action_type == "agent_command",
                          existingPayload.adapter == adapter,
                          let existingCmd = existingPayload.command,
                          let existingScroll = DialScrollRequest.from(command: existingCmd) else { return false }
                    return existingScroll.mode == .bottom
                }
                if duplicate {
                    done?()
                    return true
                }
            } else if let last = pendingHostActions.last,
                      last.profile == profile,
                      last.state.document_id == state.document_id,
                      last.state.document_revision == state.document_revision,
                      last.state.host_state_revision == state.host_state_revision,
                      last.radialIntent == nil,
                      var lastPayload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(last.json.utf8)),
                      lastPayload.action_type == "agent_command",
                      lastPayload.adapter == adapter,
                      let lastCmd = lastPayload.command,
                      let waitingScroll = DialScrollRequest.from(command: lastCmd),
                      let mergedScroll = waitingScroll.coalesced(with: incomingScroll) {
                lastPayload.command = mergedScroll.canonicalCommand
                if let mergedData = try? JSONEncoder().encode(lastPayload) {
                    let previousDone = last.done
                    let combinedDone: (() -> Void)? = (previousDone == nil && done == nil) ? nil : {
                        previousDone?()
                        done?()
                    }
                    pendingHostActions[pendingHostActions.count - 1] = PendingHostAction(
                        json: String(decoding: mergedData, as: UTF8.self),
                        binding: binding,
                        profile: profile,
                        state: state,
                        deviceSession: deviceSession ?? last.deviceSession,
                        deviceIngress: deviceIngress ?? last.deviceIngress,
                        admitted: .now,
                        requiresEnabled: requiresEnabled,
                        done: combinedDone,
                        t3Selection: last.t3Selection,
                        radialIntent: nil
                    )
                    return true
                }
            }
        }
        if pendingHostActions.count >= 32 {
            clearPendingActions(); overflowDraining = true
            status = "输入过快，已丢弃积压；当前动作结束后恢复"
            done?(); return false
        }
        let selection: T3Selection?
        if case .t3(let frozen) = radialIntent { selection = frozen }
        else { selection = payload?.adapter == "t3" && payload?.command == "next-attention" ? t3Attention.selectNext() : nil }
        pendingHostActions.append(PendingHostAction(json: json, binding: binding, profile: profile, state: state, deviceSession: deviceSession, deviceIngress: deviceIngress, admitted: .now, requiresEnabled: requiresEnabled, done: done, zcodeSelection: zcodeSelection, zcodeCommand: zcodeCommand, dshSelection: dshSelection, dshCommand: dshCommand, cindySelection: cindySelection, antigravitySelection: antigravitySelection, antigravityCommand: antigravityCommand, t3Selection: selection, radialIntent: radialIntent))
        guard hostExecutionTask == nil else { return true }
        let executionGeneration = hostExecutionGeneration
        hostExecutionTask = Task { @MainActor [self] in
            while hostExecutionIsCurrent(executionGeneration), !pendingHostActions.isEmpty {
                let entry = pendingHostActions.removeFirst()
                defer { entry.done?() }
                guard dispatchOpen, let current = routing,
                      current.document_id == entry.state.document_id, current.document_revision == entry.state.document_revision,
                      current.host_state_revision == entry.state.host_state_revision,
                      !entry.requiresEnabled || current.execution_enabled,
                      entry.deviceSession == nil || (entry.deviceSession == inputBoundary.session && (entry.deviceIngress ?? 0) > inputBoundary.cutoff),
                      entry.admitted.duration(to: .now) < .seconds(1), radialEntryReady(entry) else { continue }
                if let payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(entry.json.utf8)),
                   payload.action_type == "agent_command", payload.command == "push-to-talk", let adapter = payload.adapter {
                    if entry.deviceSession != nil {
                        guard inputBoundary.held[entry.binding] == entry.deviceIngress else { continue }
                        startHeldDictation(adapter: adapter, binding: entry.binding, profile: entry.profile, ingress: entry.deviceIngress ?? 0)
                    } else {
                        pushToTalk.cancel(); await pushToTalk.waitUntilStopped()
                        guard admitAfterDictationCleanup(entry, executionGeneration: executionGeneration) else { continue }
                        var toggle = payload; toggle.command = "dictation"
                        if let data = try? JSONEncoder().encode(toggle) {
                            _ = await executeHostAction(actionJSON: String(decoding: data, as: UTF8.self), bindingId: entry.binding, profileId: entry.profile)
                        }
                    }
                } else {
                    pushToTalk.cancel(); await pushToTalk.waitUntilStopped()
                    guard admitAfterDictationCleanup(entry, executionGeneration: executionGeneration) else {
                        if !pushToTalk.cleanupConfirmed,
                           let payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(entry.json.utf8)) {
                            let meta = RecordingHostActionExecutor.describeMetadata(payload)
                            let blockedReceipt = HostActionReceipt(
                                timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                                profileId: entry.profile,
                                bindingId: entry.binding,
                                actionType: payload.action_type,
                                target: meta.target,
                                textByteLen: meta.textByteLen,
                                outcome: "blocked",
                                detail: "dictation_cleanup_unconfirmed:\(meta.detail)"
                            )
                            AuditLogWriter.append(blockedReceipt)
                            lastHostActionReceipt = blockedReceipt
                        }
                        continue
                    }
                    if entry.radialIntent == .cheatSheet {
                        showRadialCheatSheet(entry)
                        continue
                    }
                    _ = await executeHostAction(actionJSON: entry.json, bindingId: entry.binding, profileId: entry.profile, t3Selection: entry.t3Selection, zcodeSelection: entry.zcodeSelection, zcodeCommand: entry.zcodeCommand, dshSelection: entry.dshSelection, dshCommand: entry.dshCommand, cindySelection: entry.cindySelection, antigravitySelection: entry.antigravitySelection, antigravityCommand: entry.antigravityCommand, stillAdmitted: { [weak self] in
                        self?.admitAfterDictationCleanup(entry, executionGeneration: executionGeneration, checkAge: false) == true
                    })
                }
            }
            guard hostExecutionIsCurrent(executionGeneration) else { return }
            if overflowDraining { readInputBoundary(); overflowDraining = false }
            hostExecutionTask = nil
        }
        return true
    }
    private func hostExecutionIsCurrent(_ generation: Int) -> Bool {
        generation == hostExecutionGeneration && !Task.isCancelled
    }
    private func admitAfterDictationCleanup(_ entry: PendingHostAction, executionGeneration: Int, checkAge: Bool = true) -> Bool {
        hostExecutionIsCurrent(executionGeneration) && dispatchOpen && pushToTalk.cleanupConfirmed && radialEntryReady(entry) && routing?.host_state_revision == entry.state.host_state_revision
            && routing?.document_revision == entry.state.document_revision && routing?.document_id == entry.state.document_id
            && (!entry.requiresEnabled || hostExecutionEnabled)
            && (entry.deviceSession == nil || (entry.deviceSession == inputBoundary.session && (entry.deviceIngress ?? 0) > inputBoundary.cutoff))
            && (!checkAge || entry.admitted.duration(to: .now) < .seconds(1))
    }
    @discardableResult func openZCodeSession(_ selection: ZCodeSelection) -> Bool {
        guard let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"zcode","command":"open-session"}"#,
                                 binding: "menu-zcode-session", profile: profile, state: state,
                                 requiresEnabled: false, zcodeSelection: selection)
    }
    @discardableResult func createZCodeSession(_ intent: ZCodeCreateIntent) -> Bool {
        guard let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"zcode","command":"create-session"}"#,
                                 binding: "menu-zcode-create", profile: profile, state: state,
                                 requiresEnabled: false, zcodeCommand: .create(intent))
    }
    @discardableResult func sendZCodeText(_ intent: ZCodeSendIntent) -> Bool {
        guard let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"zcode","command":"send-text"}"#,
                                 binding: "menu-zcode-send", profile: profile, state: state,
                                 requiresEnabled: false, zcodeCommand: .send(intent))
    }
    @discardableResult func stopZCodeSession(_ intent: ZCodeStopIntent) -> Bool {
        guard let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"zcode","command":"stop-session"}"#,
                                 binding: "menu-zcode-stop", profile: profile, state: state,
                                 requiresEnabled: false, zcodeCommand: .stop(intent))
    }
    @discardableResult func resolveZCodeInteraction(_ intent: ZCodeResolveIntent) -> Bool {
        guard let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"zcode","command":"resolve-interaction"}"#,
                                 binding: "menu-zcode-resolve", profile: profile, state: state,
                                 requiresEnabled: false, zcodeCommand: .resolve(intent))
    }
    @discardableResult func switchZCodeModel(_ intent: ZCodeSwitchModelIntent) -> Bool {
        guard let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"zcode","command":"switch-model"}"#,
                                 binding: "menu-zcode-switch-model", profile: profile, state: state,
                                 requiresEnabled: false, zcodeCommand: .switchModel(intent))
    }
    @discardableResult func queryZCodeCommand(_ intent: ZCodeQueryIntent) -> Bool {
        guard let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"zcode","command":"query-command"}"#,
                                 binding: "menu-zcode-query", profile: profile, state: state,
                                 requiresEnabled: false, zcodeCommand: .query(intent))
    }
    @discardableResult func openDSHSession(_ selection: DSHSelection) -> Bool {
        guard let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"dsh-desktop","command":"open-session"}"#,
                                 binding: "menu-dsh-session", profile: profile, state: state,
                                 requiresEnabled: false, dshSelection: selection)
    }
    @discardableResult func createDSHSession(_ command: DSHQueuedCommand?) -> Bool {
        guard let command, case .create = command, let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"dsh-desktop","command":"create-session"}"#,
                                 binding: "menu-dsh-create", profile: profile, state: state,
                                 requiresEnabled: false, dshCommand: command)
    }
    @discardableResult func promptDSHSession(_ command: DSHQueuedCommand?) -> Bool {
        guard let command, case .prompt = command, let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"dsh-desktop","command":"prompt-session"}"#,
                                 binding: "menu-dsh-prompt", profile: profile, state: state,
                                 requiresEnabled: false, dshCommand: command)
    }
    @discardableResult func cancelDSHSession(_ command: DSHQueuedCommand?) -> Bool {
        guard let command, case .cancel = command, let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"dsh-desktop","command":"cancel-session"}"#,
                                 binding: "menu-dsh-cancel", profile: profile, state: state,
                                 requiresEnabled: false, dshCommand: command)
    }
    @discardableResult func queryDSHCommand(_ command: DSHQueuedCommand?) -> Bool {
        guard let command, case .query = command, let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"dsh-desktop","command":"query-command"}"#,
                                 binding: "menu-dsh-query", profile: profile, state: state,
                                 requiresEnabled: false, dshCommand: command)
    }
    @discardableResult func openCindySession(_ selection: CindySelection) -> Bool {
        guard let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"cindy","command":"open-session"}"#,
                                 binding: "menu-cindy-session", profile: profile, state: state,
                                 requiresEnabled: false, cindySelection: selection)
    }
    @discardableResult func openAntigravitySession(_ selection: AntigravitySelection) -> Bool {
        guard let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"antigravity","command":"open-session"}"#,
                                 binding: "menu-antigravity-session", profile: profile, state: state,
                                 requiresEnabled: false, antigravitySelection: selection)
    }
    @discardableResult func createAntigravityConversation(_ command: AntigravityQueuedCommand?) -> Bool {
        guard let command, case .create = command.operation, let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"antigravity","command":"create-conversation"}"#,
                                 binding: "menu-antigravity-create", profile: profile, state: state,
                                 requiresEnabled: false, antigravityCommand: command)
    }
    @discardableResult func sendAntigravityMessage(_ command: AntigravityQueuedCommand?) -> Bool {
        guard let command, case .send = command.operation, let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"antigravity","command":"send-message"}"#,
                                 binding: "menu-antigravity-send", profile: profile, state: state,
                                 requiresEnabled: false, antigravityCommand: command)
    }
    @discardableResult func queryAntigravityCommand(_ command: AntigravityQueuedCommand?) -> Bool {
        guard let command, case .query = command.operation, let state = routing, let profile = state.profile_id else { return false }
        return enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"antigravity","command":"query-command"}"#,
                                 binding: "menu-antigravity-query", profile: profile, state: state,
                                 requiresEnabled: false, antigravityCommand: command)
    }
    func openNextT3Attention() {
        guard let state = routing, let profile = state.profile_id else { return }
        enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"t3","command":"next-attention"}"#,
                          binding: "menu-t3-attention", profile: profile, state: state, requiresEnabled: false)
    }
    func navigateBack() {
        guard let state = routing, let profile = state.profile_id else { return }
        enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"navigation","command":"back"}"#,
                          binding: "menu-navigate-back", profile: profile, state: state, requiresEnabled: false)
    }
    func navigateForward() {
        guard let state = routing, let profile = state.profile_id else { return }
        enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"navigation","command":"forward"}"#,
                          binding: "menu-navigate-forward", profile: profile, state: state, requiresEnabled: false)
    }
    func isNavigationPlaceAvailable(_ place: KeypadNavigationPlace) -> Bool {
        if let custom = navigationPlaceAvailable {
            return custom(place)
        }
        switch place {
        case .app(let bundleID):
            guard !bundleID.isEmpty else { return false }
            if actionExecutor is RecordingHostActionExecutor || ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] == "host-execution" {
                return true
            }
            return NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) != nil
        case .session(let adapter, let session):
            return !adapter.isEmpty && !session.isEmpty && session != "unselected"
        case .t3Thread(let environmentID, let threadID):
            return t3Attention.canNavigate(to: T3ThreadIdentity(environmentID: environmentID, threadID: threadID))
        }
    }
    func startHeldDictation(adapter: String, binding: String, profile: String, ingress: UInt64 = 0) {
        guard dispatchOpen, hostExecutionEnabled, pushToTalk.cleanupConfirmed, !pushToTalk.isActive, let lease = try? backend.beginHostExecution() else { return }
        inFlightHostActions += 1
        let started = pushToTalk.press(binding: binding, adapter: adapter, ingress: ingress, report: { [weak self] action, succeeded, detail in
            let receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: profile,
                bindingId: binding, actionType: action, target: adapter, textByteLen: nil,
                outcome: succeeded ? (self?.pushToTalk.simulated == true ? "simulated" : "executed") : "blocked", detail: detail)
            AuditLogWriter.append(receipt)
            self?.lastHostActionReceipt = receipt
            self?.dictationNeedsAttention = self?.pushToTalk.cleanupConfirmed == false
            if !succeeded { self?.status = detail }
        }, finished: { [self] in
            inFlightHostActions -= 1; backend.finishHostExecution(lease: lease)
        })
        if !started { inFlightHostActions -= 1; backend.finishHostExecution(lease: lease) }
    }
    func verifyDictationStopped() async {
        let confirmed = await pushToTalk.verifyStopped()
        dictationNeedsAttention = !confirmed
        status = confirmed ? "已确认原听写停止，可继续按住说话" : "原听写仍未确认停止；请在原应用停止录音后重试"
    }
    @discardableResult func consumeHostInput(_ event: InputEventInfo) -> Bool {
        guard event.sessionId == inputBoundary.session else { return true }
        if event.kind == .released, let binding = event.bindingId { pushToTalk.release(binding: binding, ingress: event.ingressSequence) }
        if event.kind == .inputStreamGap {
            clearPendingActions(); inputNeedsResume = true
            status = "输入流丢失，已暂停派发；请恢复输入并松开按键 / 摇杆回中"
            return false
        }
        let admission = inputBoundary.consume(event, gateOpen: dispatchOpen && !inputNeedsResume)
        if consumeRadialInput(event, admission: admission) { return true }
        if admission.pressed,
           let binding = event.bindingId, let (rawJSON, profile, state) = resolveHostAction(binding) {
            var json = rawJSON
            if ["AG13", "ENC_CC", "AG14", "ENC_CW"].contains(binding),
               inputBoundary.held["ENC_CLK"] != nil,
               var payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(rawJSON.utf8)),
               payload.action_type == "agent_command",
               let cmd = payload.command,
               let fastScroll = DialScrollRequest.from(command: cmd, dialHeld: true) {
                payload.command = fastScroll.canonicalCommand
                if let encoded = try? JSONEncoder().encode(payload) {
                    json = String(decoding: encoded, as: UTF8.self)
                }
            }
            enqueueHostAction(json: json, binding: binding, profile: profile, state: state,
                deviceSession: event.sessionId, deviceIngress: event.ingressSequence)
        }
        return true
    }
    func resumeInput() {
        guard inputNeedsResume, connectedID != nil else { return }
        inputBoundary.recovery = true; inputBoundary.released = []; inputBoundary.radialQuarantined = true
        inputNeedsResume = false; startHostInputLoop(token: generation)
    }
}
