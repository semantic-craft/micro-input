import AppKit
import MicroInputCore

extension Store {
    var radialReadiness: RadialInputReadiness {
        guard let session = inputBoundary.session else { return .disconnected }
        return backend.radialMenuInputReadiness(sessionId: session)
    }
    func setRadialMenuEnabled(_ enabled: Bool) {
        radialPreferences.set(enabled, forKey: "radialMenuEnabled")
        readInputBoundary()
        radialMenu.setEnabled(enabled)
        radialOverlay.hide()
    }
    func cancelRadialMenu() { radialMenu.cancel(); radialOverlay.hide() }
    func radialSlots() -> [RadialSlot] {
        let selection = t3Attention.selectNext()
        let title = selection.flatMap { selected in t3Attention.items.first(where: { $0.event == selected.event })?.title }
        let hasProfile = routing?.profile_id.map { id in hostProfiles.contains(where: { $0.id.text == id }) } ?? false
        var slots = [RadialSlot(label: title.map { "需要我 · \($0)" } ?? "T3 下一个需要我", intent: selection.map(RadialIntent.t3),
                                unavailableReason: selection == nil ? "没有新鲜待处理目标" : nil),
                     RadialSlot(label: "按键速查", intent: hasProfile ? .cheatSheet : nil,
                                unavailableReason: hasProfile ? nil : "当前配置不可用")]
        for adapter in ["cindy", "codex", "qwenwork", "zcode", "antigravity", "dsh-desktop"] {
            guard let bundle = DesktopAgentControls.bundles[adapter], let app = DesktopAgent.all.first(where: { $0.bundleID == bundle }) else { continue }
            let installed = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundle) != nil
            slots.append(RadialSlot(label: app.name, intent: installed ? .openApp(bundle) : nil, unavailableReason: installed ? nil : "应用未安装"))
        }
        return slots
    }
    func consumeRadialInput(_ event: InputEventInfo, admission: HostInputAdmission) -> Bool {
        guard radialMenu.enabled else { return false }
        let capturedConfirm = radialMenu.capturesConfirm && event.bindingId == "ENC_CLK"
        let position = event.radialPosition
        guard dispatchOpen, !inputNeedsResume, hostExecutionEnabled, !pushToTalk.isActive, pushToTalk.cleanupConfirmed,
              radialReadiness == .ready, let route = routing else {
            cancelRadialMenu()
            return position != nil || capturedConfirm
        }
        if let session = radialMenu.session,
           session.inputSession != event.sessionId || RadialRoute(session.routing) != RadialRoute(route) {
            cancelRadialMenu()
            if capturedConfirm { return true }
        }
        if let position {
            guard admission.radial else { cancelRadialMenu(); return true }
            let session = RadialMenuSession(inputSession: event.sessionId, routing: route, slots: radialMenu.session == nil ? radialSlots() : [], selected: nil)
            radialMenu.update(position, makeSession: { session.slots.count == 8 ? session : nil })
            radialOverlay.render(radialMenu.session)
            return true
        }
        if event.bindingId == "ENC_CLK", radialMenu.capturesConfirm {
            if admission.pressed, let (session, intent) = radialMenu.confirm() {
                radialOverlay.hide()
                enqueueRadialIntent(intent, session: session, ingress: event.ingressSequence)
            }
            return true
        }
        if event.kind == .pressed, radialMenu.session != nil { cancelRadialMenu() }
        return false
    }
    func radialEntryReady(_ entry: PendingHostAction) -> Bool {
        guard let intent = entry.radialIntent else { return true }
        guard let session = entry.deviceSession, backend.radialMenuInputReadiness(sessionId: session) == .ready,
              radialMenu.enabled else { return false }
        if case .openApp(let bundle) = intent { return NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundle) != nil }
        return true
    }
    func enqueueRadialIntent(_ intent: RadialIntent, session: RadialMenuSession, ingress: UInt64) {
        let profile = session.routing.profile_id ?? ""
        let json: String
        switch intent {
        case .openApp(let bundle):
            guard let data = try? JSONSerialization.data(withJSONObject: ["action_type": "open_app", "bundle_id": bundle]) else { return }
            json = String(decoding: data, as: UTF8.self)
        case .t3: json = #"{"action_type":"agent_command","adapter":"t3","command":"next-attention"}"#
        case .cheatSheet: json = "{}"
        }
        enqueueHostAction(json: json, binding: "radial-menu", profile: profile, state: session.routing,
                          deviceSession: session.inputSession, deviceIngress: ingress, radialIntent: intent)
    }
    func showRadialCheatSheet(_ entry: PendingHostAction) {
        guard let lease = try? backend.beginHostExecution() else { return }
        inFlightHostActions += 1
        defer { inFlightHostActions -= 1; backend.finishHostExecution(lease: lease) }
        overlay.showSheet()
        let receipt = HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: entry.profile,
            bindingId: entry.binding, actionType: "cheat_sheet", target: "micro-input", textByteLen: nil,
            outcome: "executed", detail: "已显示当前宿主配置的按键速查")
        AuditLogWriter.append(receipt); lastHostActionReceipt = receipt
    }
}
