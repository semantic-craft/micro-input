import Foundation
import AppKit
import MicroInputCore

struct HostRoutingState: Decodable {
    let document_id: String, document_revision: UInt64, host_state_revision: UInt64
    let profile_id: String?, profile_name: String?
    let execution_enabled: Bool, auto_follow_enabled: Bool, manual_override: Bool
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
    mutating func consume(_ event: InputEventInfo, gateOpen: Bool) -> Bool {
        guard session == event.sessionId else { return false }
        if let center = event.radialCenter {
            radialHeld = !center
            if center { radialQuarantined = false }
            else if !gateOpen || event.ingressSequence <= cutoff { radialQuarantined = true }
        }
        if event.kind == .released, let binding = event.bindingId { released.insert(binding); held.removeValue(forKey: binding) }
        if event.kind == .pressed, let binding = event.bindingId { held[binding] = event.ingressSequence }
        guard gateOpen, event.ingressSequence > cutoff, event.kind == .pressed, event.actionEligible else { return false }
        if event.radialCenter != nil { return !radialQuarantined }
        if recovery, event.actionRole != "encoder_step", let binding = event.bindingId { return released.contains(binding) }
        return true
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
    var t3Selection: T3Selection? = nil
}

extension Store {
    var routingMode: String {
        guard let routing else { return "配置尚未载入" }
        if routing.manual_override { return "手动保持" }
        return routing.auto_follow_enabled ? "自动跟随" : "自动跟随已关闭"
    }
    var dispatchOpen: Bool { !hostOperationPaused && !mutationDraining && routingValid && !systemSleeping && !stopped && !overflowDraining }
    func refreshRouting() throws {
        let value = try JSONDecoder().decode(HostRoutingState.self, from: Data(backend.hostRoutingState().utf8))
        routing = value; hostActiveProfileId = value.profile_id; hostExecutionEnabled = value.execution_enabled
        routingValid = true
    }
    func readInputBoundary() {
        guard let watermark = try? JSONDecoder().decode(InputWatermark.self, from: Data(backend.hostInputWatermark().utf8)) else {
            inputBoundary.session = nil; return
        }
        inputBoundary.advance(watermark)
    }
    func clearPendingActions() {
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
        foregroundTask = Task { await routingChange(["kind":"follow", "bundle_id":bundleID], automatic: true, observation: sequence) }
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
    @discardableResult func enqueueHostAction(json: String, binding: String, profile: String, state: HostRoutingState, deviceSession: UInt64? = nil, deviceIngress: UInt64? = nil, requiresEnabled: Bool = true, done: (() -> Void)? = nil) -> Bool {
        guard dispatchOpen, !requiresEnabled || hostExecutionEnabled else { status = "正在切换或输入已暂停，请稍后重试"; done?(); return false }
        if pendingHostActions.count >= 32 {
            clearPendingActions(); overflowDraining = true
            status = "输入过快，已丢弃积压；当前动作结束后恢复"
            done?(); return false
        }
        let payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(json.utf8))
        let selection = payload?.adapter == "t3" && payload?.command == "next-attention" ? t3Attention.selectNext() : nil
        pendingHostActions.append(PendingHostAction(json: json, binding: binding, profile: profile, state: state, deviceSession: deviceSession, deviceIngress: deviceIngress, admitted: .now, requiresEnabled: requiresEnabled, done: done, t3Selection: selection))
        guard hostExecutionTask == nil else { return true }
        let executionGeneration = hostExecutionGeneration
        hostExecutionTask = Task { @MainActor [self] in
            while !pendingHostActions.isEmpty {
                let entry = pendingHostActions.removeFirst()
                defer { entry.done?() }
                guard dispatchOpen, let current = routing,
                      current.document_id == entry.state.document_id, current.document_revision == entry.state.document_revision,
                      current.host_state_revision == entry.state.host_state_revision,
                      !entry.requiresEnabled || current.execution_enabled,
                      entry.deviceSession == nil || (entry.deviceSession == inputBoundary.session && (entry.deviceIngress ?? 0) > inputBoundary.cutoff),
                      entry.admitted.duration(to: .now) < .seconds(1) else { continue }
                if let payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(entry.json.utf8)),
                   payload.action_type == "agent_command", payload.command == "push-to-talk", let adapter = payload.adapter {
                    if entry.deviceSession != nil {
                        guard inputBoundary.held[entry.binding] == entry.deviceIngress else { continue }
                        startHeldDictation(adapter: adapter, binding: entry.binding, profile: entry.profile, ingress: entry.deviceIngress ?? 0)
                    } else {
                        pushToTalk.cancel(); await pushToTalk.waitUntilStopped()
                        guard admitAfterDictationCleanup(entry) else { continue }
                        var toggle = payload; toggle.command = "dictation"
                        if let data = try? JSONEncoder().encode(toggle) {
                            _ = await executeHostAction(actionJSON: String(decoding: data, as: UTF8.self), bindingId: entry.binding, profileId: entry.profile)
                        }
                    }
                } else {
                    pushToTalk.cancel(); await pushToTalk.waitUntilStopped()
                    guard admitAfterDictationCleanup(entry) else { continue }
                    _ = await executeHostAction(actionJSON: entry.json, bindingId: entry.binding, profileId: entry.profile, t3Selection: entry.t3Selection, stillAdmitted: { [weak self] in
                        self?.admitAfterDictationCleanup(entry, checkAge: false) == true
                    })
                }
            }
            if overflowDraining { readInputBoundary(); overflowDraining = false }
            if executionGeneration == hostExecutionGeneration { hostExecutionTask = nil }
        }
        return true
    }
    private func admitAfterDictationCleanup(_ entry: PendingHostAction, checkAge: Bool = true) -> Bool {
        dispatchOpen && pushToTalk.cleanupConfirmed && routing?.host_state_revision == entry.state.host_state_revision
            && routing?.document_revision == entry.state.document_revision && routing?.document_id == entry.state.document_id
            && (!entry.requiresEnabled || hostExecutionEnabled)
            && (entry.deviceSession == nil || (entry.deviceSession == inputBoundary.session && (entry.deviceIngress ?? 0) > inputBoundary.cutoff))
            && (!checkAge || entry.admitted.duration(to: .now) < .seconds(1))
    }
    func openNextT3Attention() {
        guard let state = routing, let profile = state.profile_id else { return }
        enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"t3","command":"next-attention"}"#,
                          binding: "menu-t3-attention", profile: profile, state: state, requiresEnabled: false)
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
        if inputBoundary.consume(event, gateOpen: dispatchOpen && !inputNeedsResume),
           let binding = event.bindingId, let (json, profile, state) = resolveHostAction(binding) {
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
