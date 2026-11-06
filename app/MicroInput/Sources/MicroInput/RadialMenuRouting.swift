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
    private static let radialSlotConfigsPreferencesKey = "radialMenuSlotConfigs"

    func radialSlotConfigs() -> [RadialSlotConfig]? {
        guard let data = radialPreferences.data(forKey: Self.radialSlotConfigsPreferencesKey),
              let decoded = try? JSONDecoder().decode([RadialSlotConfig].self, from: data),
              decoded.count == 8 else { return nil }
        return decoded
    }
    func setRadialSlotConfigs(_ configs: [RadialSlotConfig]?) {
        if let configs, configs.count == 8, let data = try? JSONEncoder().encode(configs) {
            radialPreferences.set(data, forKey: Self.radialSlotConfigsPreferencesKey)
        } else {
            radialPreferences.removeObject(forKey: Self.radialSlotConfigsPreferencesKey)
        }
        cancelRadialMenu()
    }
    func setRadialSlotConfig(at index: Int, _ config: RadialSlotConfig) {
        guard (0..<8).contains(index) else { return }
        var current = radialSlotConfigs() ?? defaultRadialSlotConfigs()
        guard current.indices.contains(index) else { return }
        current[index] = config
        setRadialSlotConfigs(current)
    }
    func applyPaseoRadialPreset() {
        setRadialSlotConfigs(RadialSlotConfig.paseoEngineeringSlots)
    }
    func applyFourAppRadialPreset() {
        setRadialSlotConfigs(RadialSlotConfig.fourAppEngineeringSlots)
    }
    private func defaultRadialSlotConfigs() -> [RadialSlotConfig] {
        var configs: [RadialSlotConfig] = [
            .t3(),
            .cheatSheet()
        ]
        for adapter in ["cindy", "codex", "qwenwork", "zcode", "antigravity", "dsh-desktop"] {
            if let bundle = DesktopAgentControls.bundles[adapter],
               let app = DesktopAgent.all.first(where: { $0.bundleID == bundle }) {
                configs.append(.openApp(label: app.name, bundleID: bundle))
            }
        }
        return configs
    }
    private func resolveRadialSlot(_ config: RadialSlotConfig, hasProfile: Bool) -> RadialSlot {
        switch config.kind {
        case .t3:
            let selection = t3Attention.selectNext()
            let title = selection.flatMap { selected in t3Attention.items.first(where: { $0.event == selected.event })?.title }
            return RadialSlot(
                label: title.map { "需要我 · \($0)" } ?? config.label,
                intent: selection.map(RadialIntent.t3),
                unavailableReason: selection == nil ? "没有新鲜待处理目标" : nil
            )
        case .cheatSheet:
            return RadialSlot(
                label: config.label,
                intent: hasProfile ? .cheatSheet : nil,
                unavailableReason: hasProfile ? nil : "当前配置不可用"
            )
        case .openApp:
            let bundle = config.bundleID ?? ""
            let installed = !bundle.isEmpty && NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundle) != nil
            return RadialSlot(
                label: config.label,
                intent: installed ? .openApp(bundle) : nil,
                unavailableReason: installed ? nil : "应用未安装"
            )
        case .prompt:
            let sanitized = RecordingHostActionExecutor.stripTrailingNewlines(config.text ?? "")
            guard !sanitized.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                return RadialSlot(label: config.label, intent: nil, unavailableReason: "未设置指令文本")
            }
            let adapter = config.adapter ?? activeDesktopAgentAdapter()
            let available = adapter != nil || hasProfile
            return RadialSlot(
                label: config.label,
                intent: available ? .prompt(text: sanitized, adapter: adapter, submit: config.submit) : nil,
                unavailableReason: available ? nil : "没有可用的目标配置"
            )
        case .submit:
            let adapter = config.adapter ?? activeDesktopAgentAdapter()
            return RadialSlot(
                label: config.label,
                intent: adapter != nil ? .submit(adapter: adapter) : nil,
                unavailableReason: adapter != nil ? nil : "当前配置不是桌面 Agent"
            )
        case .stop:
            let adapter = config.adapter ?? activeDesktopAgentAdapter()
            return RadialSlot(
                label: config.label,
                intent: adapter != nil ? .stop(adapter: adapter) : nil,
                unavailableReason: adapter != nil ? nil : "当前配置不是桌面 Agent"
            )
        case .navigateBack:
            let canBack = navigationHistory.canGoBack(available: { [weak self] place in self?.isNavigationPlaceAvailable(place) == true })
            return RadialSlot(
                label: config.label,
                intent: canBack ? .navigateBack : nil,
                unavailableReason: canBack ? nil : "没有可后退的打开历史"
            )
        case .navigateForward:
            let canForward = navigationHistory.canGoForward(available: { [weak self] place in self?.isNavigationPlaceAvailable(place) == true })
            return RadialSlot(
                label: config.label,
                intent: canForward ? .navigateForward : nil,
                unavailableReason: canForward ? nil : "没有可前进的打开历史"
            )
        }
    }
    func radialSlots() -> [RadialSlot] {
        let hasProfile = routing?.profile_id.map { id in hostProfiles.contains(where: { $0.id.text == id }) } ?? false
        let configs = radialSlotConfigs() ?? defaultRadialSlotConfigs()
        return configs.map { resolveRadialSlot($0, hasProfile: hasProfile) }
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
            if let (confirmedSession, intent) = radialMenu.update(position, makeSession: { session.slots.count == 8 ? session : nil }) {
                radialOverlay.hide()
                enqueueRadialIntent(intent, session: confirmedSession, ingress: event.ingressSequence)
            } else {
                radialOverlay.render(radialMenu.session)
            }
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
        case .prompt(let text, let adapter, let submit):
            let resolvedAdapter = adapter ?? activeDesktopAgentAdapter(for: profile)
            let targetBundleID = resolvedAdapter.flatMap { DesktopAgentControls.bundles[$0] }
            let payload = ResolvedHostActionPayload(
                action_type: "paste_text",
                target_bundle_id: targetBundleID,
                text: text,
                adapter: resolvedAdapter,
                no_newline: true,
                submit: submit
            )
            guard let data = try? JSONEncoder().encode(payload) else { return }
            json = String(decoding: data, as: UTF8.self)
        case .submit(let adapter):
            let resolvedAdapter = adapter ?? activeDesktopAgentAdapter(for: profile) ?? "host"
            let payload = ResolvedHostActionPayload(
                action_type: "agent_command",
                target_bundle_id: DesktopAgentControls.bundles[resolvedAdapter],
                adapter: resolvedAdapter,
                command: "submit"
            )
            guard let data = try? JSONEncoder().encode(payload) else { return }
            json = String(decoding: data, as: UTF8.self)
        case .stop(let adapter):
            let resolvedAdapter = adapter ?? activeDesktopAgentAdapter(for: profile) ?? "host"
            let payload = ResolvedHostActionPayload(
                action_type: "agent_command",
                target_bundle_id: DesktopAgentControls.bundles[resolvedAdapter],
                adapter: resolvedAdapter,
                command: "stop"
            )
            guard let data = try? JSONEncoder().encode(payload) else { return }
            json = String(decoding: data, as: UTF8.self)
        case .navigateBack:
            json = #"{"action_type":"agent_command","adapter":"navigation","command":"back"}"#
        case .navigateForward:
            json = #"{"action_type":"agent_command","adapter":"navigation","command":"forward"}"#
        }
        enqueueHostAction(json: json, binding: "radial-menu", profile: profile, state: session.routing,
                          deviceSession: session.inputSession, deviceIngress: ingress, radialIntent: intent)
    }

    var radialConfirmMode: RadialMenuState.ConfirmMode {
        get {
            guard let raw = radialPreferences.string(forKey: "radialConfirmMode"),
                  let mode = RadialMenuState.ConfirmMode(rawValue: raw) else {
                return radialMenu.confirmMode
            }
            return mode
        }
        set {
            radialPreferences.set(newValue.rawValue, forKey: "radialConfirmMode")
            radialMenu.setConfirmMode(newValue)
        }
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
