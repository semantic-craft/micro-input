#if MI_SYNTHETIC_BACKEND
import AppKit
import MicroInputCore

extension Store {
    func prepareRadialMenuScenario() async {
        guard let core = backend as? MicroInputCore.MicroInput else { return }
        await installCodingAgentPresets(); await setHostExecutionEnabled(true)
        do {
            try await core.setSyntheticDiscovery(state: .reconnectedDevice)
            try await core.setSyntheticConfig(scenario: 1)
            await refresh(); selectDevice("synthetic-reconnected-interface-0")
            await connect("synthetic-reconnected-interface-0"); await readIdentity(); await readDeviceConfig(); showLocalDraft()
            setRadialMenuEnabled(true)
            try await Task.sleep(for: .milliseconds(100))
            for window in NSApp.windows where !(window is NSPanel) { window.orderOut(nil) }
            let foreground = NSWorkspace.shared.frontmostApplication?.processIdentifier
            try await core.emitSyntheticInputCase(inputCase: .radialCenter)
            try await core.emitSyntheticInputCase(inputCase: .radialEast)
            try await Task.sleep(for: .milliseconds(250))
            recordRadialPreview(phase: "eight-slots", passed: radialMenu.session?.slots.count == 8 && radialMenu.session?.selected == 0 && NSWorkspace.shared.frontmostApplication?.processIdentifier == foreground)
            try await core.emitSyntheticInputCase(inputCase: .encoderClick)
            try await Task.sleep(for: .milliseconds(100))
            recordRadialPreview(phase: "disabled-slot-zero-action", passed: radialMenu.session?.intent == nil && lastHostActionReceipt == nil)
            try await Task.sleep(for: .seconds(12))
            try await core.emitSyntheticInputCase(inputCase: .radialSouthEast)
            try await Task.sleep(for: .seconds(8))
            try await core.emitSyntheticInputCase(inputCase: .encoderClick)
            try await Task.sleep(for: .milliseconds(300))
            try await core.emitSyntheticInputCase(inputCase: .encoderClick)
            try await core.emitSyntheticInputCase(inputCase: .radialCenter)
            try await core.emitSyntheticInputCase(inputCase: .radialWest)
            try await Task.sleep(for: .milliseconds(100))
            try await core.emitSyntheticInputCase(inputCase: .radialCenter)
            try await Task.sleep(for: .milliseconds(100))
            recordRadialPreview(phase: "center-cancel", passed: radialMenu.session == nil && NSWorkspace.shared.frontmostApplication?.processIdentifier == foreground)
            overlay.hide()
            status = "径向菜单合成输入验收结束 · 实体手感未验收"
        } catch { cancelRadialMenu(); status = readableError(error) }
    }
    private func recordRadialPreview(phase: String, passed: Bool) {
        AuditLogWriter.append(HostActionReceipt(timestampMs: UInt64(Date().timeIntervalSince1970 * 1000), profileId: hostActiveProfileId ?? "",
            bindingId: "radial-menu", actionType: "radial_menu_preview", target: "synthetic", textByteLen: nil,
            outcome: passed ? "simulated" : "blocked", detail: phase))
    }
}
#endif
