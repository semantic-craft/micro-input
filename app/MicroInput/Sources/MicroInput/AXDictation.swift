import AppKit
import ApplicationServices

@MainActor struct AXDictationDriver: DictationDriving {
    func start(adapter: String, isHeld: @escaping () -> Bool) async -> DictationStart {
        guard let bundle = DesktopAgentControls.bundles[adapter], AXIsProcessTrusted() else {
            return .blocked("听写目标不可用或辅助功能未授权")
        }
        if NSWorkspace.shared.frontmostApplication?.bundleIdentifier != bundle {
            guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundle) else { return .blocked("目标应用未安装") }
            let configuration = NSWorkspace.OpenConfiguration(); configuration.activates = true
            NSWorkspace.shared.openApplication(at: url, configuration: configuration) { _, _ in }
        }
        for _ in 0..<20 {
            if NSWorkspace.shared.frontmostApplication?.bundleIdentifier == bundle { break }
            try? await Task.sleep(nanoseconds: 25_000_000)
        }
        guard isHeld(), let app = NSWorkspace.shared.frontmostApplication, app.bundleIdentifier == bundle else {
            return .blocked("按键已松开或目标未能到前台，未启动听写")
        }
        let root = AXUIElementCreateApplication(app.processIdentifier)
        AXUIElementSetMessagingTimeout(root, 0.3)
        var window: AXUIElement?
        for _ in 0..<20 {
            if let raw = DesktopAgentControls.attribute(root, kAXFocusedWindowAttribute), CFGetTypeID(raw) == AXUIElementGetTypeID() {
                window = unsafeBitCast(raw, to: AXUIElement.self); break
            }
            try? await Task.sleep(nanoseconds: 25_000_000)
        }
        guard let window else { return .blocked("没有唯一的当前听写窗口") }
        var pending = [window], matches: [AXUIElement] = [], visited = 0
        let deadline = Date().addingTimeInterval(1.5)
        while let element = pending.popLast(), visited < 1800, Date() < deadline {
            visited += 1
            let labels = AXDictationRecording.labels(element)
            if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
            let role = DesktopAgentControls.attribute(element, kAXRoleAttribute) as? String ?? ""
            if DesktopAgentControls.extraControl(adapter: adapter, command: "dictation", role: role, labels: labels),
               DesktopAgentControls.attribute(element, kAXEnabledAttribute) as? Bool != false { matches.append(element) }
            if let children = DesktopAgentControls.attribute(element, kAXChildrenAttribute) as? [AXUIElement] { pending.append(contentsOf: children) }
        }
        guard pending.isEmpty, Date() < deadline, matches.count == 1, let control = matches.first else {
            return .blocked("当前窗口没有唯一可确认的听写控件")
        }
        guard DesktopAgentControls.dictationState(labels: AXDictationRecording.labels(control), recording: false) else {
            return .blocked("听写已在录音或状态未知；不接管已有录音")
        }
        let recording = AXDictationRecording(app: app, window: window, control: control, adapter: adapter)
        guard isHeld(), recording.targetIsOriginal,
              NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else {
            return .blocked("按键已松开或目标已变化，未启动听写")
        }
        let error = AXUIElementPerformAction(control, kAXPressAction as CFString)
        if await recording.confirm(recording: true) { recording.startConfirmed = true; return .started(recording) }
        return .uncertain(recording, error == .success ? "听写启动状态未确认，正在清理原目标" : "听写操作返回错误，正在核对原目标")
    }
}

@MainActor struct DictationTargetAccess {
    var terminated: () -> Bool
    var originalWindow: () -> Bool
    var originalControl: () -> Bool
    var labels: () -> [String]
    var uniqueIdleInWindow: () -> Bool
    var pressOriginal: () -> Void
}

@MainActor final class AXDictationRecording: DictationRecording {
    private let access: DictationTargetAccess
    private var stopped = false
    private var stopAttempted = false
    var startConfirmed = false
    init(access: DictationTargetAccess) { self.access = access }
    convenience init(app: NSRunningApplication, window: AXUIElement, control: AXUIElement, adapter: String) {
        let title = DesktopAgentControls.attribute(window, kAXTitleAttribute) as? String
        func originalWindow() -> Bool {
            guard !app.isTerminated,
                  DesktopAgentControls.attribute(window, kAXRoleAttribute) as? String == kAXWindowRole,
                  DesktopAgentControls.attribute(window, kAXTitleAttribute) as? String == title else { return false }
            var pid: pid_t = 0
            guard AXUIElementGetPid(window, &pid) == .success, pid == app.processIdentifier else { return false }
            let root = AXUIElementCreateApplication(app.processIdentifier)
            guard let windows = DesktopAgentControls.attribute(root, kAXWindowsAttribute) as? [AXUIElement] else { return false }
            return windows.contains { CFEqual($0, window) }
        }
        func originalControl() -> Bool {
            guard originalWindow() else { return false }
            var parent = control
            for _ in 0..<64 {
                if CFEqual(parent, window) { return true }
                guard let raw = DesktopAgentControls.attribute(parent, kAXParentAttribute), CFGetTypeID(raw) == AXUIElementGetTypeID() else { return false }
                parent = unsafeBitCast(raw, to: AXUIElement.self)
            }
            return false
        }
        self.init(access: DictationTargetAccess(
            terminated: { app.isTerminated }, originalWindow: originalWindow,
            originalControl: originalControl, labels: { Self.labels(control) },
            uniqueIdleInWindow: {
                var pending = [window], matches: [AXUIElement] = [], visited = 0
                let deadline = Date().addingTimeInterval(1.5)
                while let element = pending.popLast(), visited < 1800, Date() < deadline {
                    visited += 1
                    let labels = Self.labels(element)
                    let role = DesktopAgentControls.attribute(element, kAXRoleAttribute) as? String ?? ""
                    if DesktopAgentControls.extraControl(adapter: adapter, command: "dictation", role: role, labels: labels) {
                        matches.append(element)
                    }
                    var raw: CFTypeRef?
                    let result = AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &raw)
                    if result == .success {
                        guard let children = raw as? [AXUIElement] else { return false }
                        pending.append(contentsOf: children)
                    } else if result != .attributeUnsupported { return false }
                }
                guard pending.isEmpty, visited < 1800, Date() < deadline, matches.count == 1,
                      let match = matches.first, originalWindow() else { return false }
                return DesktopAgentControls.dictationState(labels: Self.labels(match), recording: false)
            }, pressOriginal: { _ = AXUIElementPerformAction(control, kAXPressAction as CFString) }))
    }
    static func labels(_ element: AXUIElement) -> [String] {
        [kAXTitleAttribute, kAXDescriptionAttribute].compactMap { DesktopAgentControls.attribute(element, $0) as? String }
    }
    var targetIsOriginal: Bool { access.originalControl() }
    private func stoppedState() -> Bool {
        guard access.originalWindow() else { return false }
        if targetIsOriginal && DesktopAgentControls.dictationState(labels: access.labels(), recording: false) { return true }
        return stopAttempted && access.uniqueIdleInWindow() && access.originalWindow()
    }
    func confirm(recording: Bool) async -> Bool {
        for _ in 0..<25 {
            if recording {
                guard targetIsOriginal else { return false }
                if DesktopAgentControls.dictationState(labels: access.labels(), recording: true) { return true }
            } else {
                guard access.originalWindow() else { return false }
                if stoppedState() { return true }
            }
            try? await Task.sleep(nanoseconds: 100_000_000)
        }
        return false
    }
    func isStopped() async -> Bool {
        if access.terminated() { return true }
        return stoppedState()
    }
    func stop() async -> (Bool, String) {
        guard !stopped else { return (false, "原听写结束操作已执行，不重放") }
        stopped = true
        guard targetIsOriginal else { return (false, "原听写控件或窗口已变化，停止未确认；请检查原应用") }
        let labels = access.labels()
        if DesktopAgentControls.dictationState(labels: labels, recording: false) {
            return startConfirmed ? (true, "原听写已停止；未发送消息") : (false, "启动结果未知，不能从当前空闲控件推定已停止；请检查原应用")
        }
        guard DesktopAgentControls.dictationState(labels: labels, recording: true) else {
            return (false, "原控件未确认处于录音，未执行切换；请检查原应用")
        }
        stopAttempted = true
        access.pressOriginal()
        return await confirm(recording: false)
            ? (true, "已确认原听写停止；未发送消息")
            : (false, "原听写停止未确认；未重试或发送消息")
    }
}
