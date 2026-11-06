import AppKit
import ApplicationServices
import SwiftUI

struct DesktopAgent: Identifiable {
    let name: String
    let bundleID: String
    let symbol: String
    let capabilities: String
    var id: String { name }
    var installed: Bool { NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) != nil }
    static let all: [DesktopAgent] = [
        .init(name: "Cindy", bundleID: "com.xd.cindy", symbol: "sparkles", capabilities: "打开应用、搜索任务、定位输入框、旋钮滚动对话"),
        .init(name: "Codex", bundleID: "com.openai.codex", symbol: "terminal", capabilities: "打开应用、搜索、定位输入框、旋钮滚动对话"),
        .init(name: "千问办公", bundleID: "cn.qwenwork.desktop.mac", symbol: "text.bubble", capabilities: "打开应用、新建任务、搜索、定位输入框、旋钮滚动对话"),
        .init(name: "ZCode", bundleID: "dev.zcode.app", symbol: "chevron.left.forwardslash.chevron.right", capabilities: "打开应用、新建任务、搜索、定位输入框、旋钮滚动对话"),
        .init(name: "Antigravity", bundleID: "com.google.antigravity", symbol: "sparkles", capabilities: "打开应用、新建对话、搜索、定位输入框、旋钮滚动对话"),
        .init(name: "DeepSeek Coder (DSH)", bundleID: "com.deepseek.dsh", symbol: "terminal", capabilities: "打开应用、新建对话、搜索、定位输入框、旋钮滚动对话"),
        .init(name: "Herdr", bundleID: "dev.herdr.app", symbol: "rectangle.split.2x2", capabilities: "工作区切换、标签页、分屏与面板导航")
    ]
    // Stored profile names remain stable so existing mappings and favorites survive.
    static func displayName(_ name: String) -> String {
        switch name {
        case "DeepSeek Coder (DSH)", "DSH", "DeepSeek Harness": return "Deepseek Harness"
        default: return name
        }
    }
    static func named(_ name: String) -> DesktopAgent? {
        all.first { $0.name == name || displayName($0.name) == displayName(name) }
    }
    static func adapterForName(_ name: String) -> String? {
        switch name.trimmingCharacters(in: .whitespacesAndNewlines) {
        case "Cindy": return "cindy"
        case "Codex": return "codex"
        case "千问办公", "QwenWork": return "qwenwork"
        case "ZCode": return "zcode"
        case "Antigravity": return "antigravity"
        case "DeepSeek Coder (DSH)", "DSH", "DeepSeek Harness", "Deepseek Harness": return "dsh-desktop"
        default: return nil
        }
    }
    @MainActor static func forAdapter(_ adapter: String) -> DesktopAgent? {
        guard let bundleID = DesktopAgentControls.bundles[adapter] else { return nil }
        return all.first { $0.bundleID == bundleID }
    }
}

// Restrict AX actions to one focused window in an allowlisted app. Never pick the
// first of multiple editors, send text, or invoke approval controls.
@MainActor enum DesktopAgentControls {
    static let bundles = ["codex": "com.openai.codex", "cindy": "com.xd.cindy", "qwenwork": "cn.qwenwork.desktop.mac", "zcode": "dev.zcode.app", "antigravity": "com.google.antigravity", "dsh-desktop": "com.deepseek.dsh", "dsh": "com.deepseek.dsh"]
    static func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success else { return nil }
        return value
    }
    static func perform(adapter: String, command: String, revealTools: Bool = true) async -> (Bool, String) {
        guard let bundle = bundles[adapter], ["focus-input", "search", "new-chat", "model", "effort", "dictation", "browser", "terminal", "reveal-tools", "voice-reply", "toggle-voice-reply"].contains(command),
              !(adapter == "qwenwork" && command == "search"),
              command != "new-chat" || ["antigravity", "dsh-desktop"].contains(adapter) else { return (false, "未接入的应用操作") }
        guard AXIsProcessTrusted() else { return (false, "需要辅助功能权限") }
        guard let app = NSWorkspace.shared.frontmostApplication, app.bundleIdentifier == bundle else {
            return (false, "目标应用不在前台，未执行")
        }
        let root = AXUIElementCreateApplication(app.processIdentifier)
        AXUIElementSetMessagingTimeout(root, 0.3)
        // Activation and AX focus arrive on different run-loop turns.
        var focusedWindow: AXUIElement?
        for _ in 0..<20 {
            if let value = attribute(root, kAXFocusedWindowAttribute), CFGetTypeID(value) == AXUIElementGetTypeID() {
                focusedWindow = unsafeBitCast(value, to: AXUIElement.self); break
            }
            if let windows = attribute(root, kAXWindowsAttribute) as? [AXUIElement], windows.count == 1 {
                focusedWindow = windows[0]; break
            }
            try? await Task.sleep(nanoseconds: 25_000_000)
        }
        guard let window = focusedWindow else { return (false, "目标应用没有唯一可访问的当前窗口") }
        var pending = [window], matches: [AXUIElement] = []
        var visited = 0
        var terminalAlreadyOpen = false
        let deadline = Date().addingTimeInterval(1.5)
        while let element = pending.popLast(), visited < 1800, Date() < deadline {
            visited += 1
            let role = attribute(element, kAXRoleAttribute) as? String ?? ""
            let title = attribute(element, kAXTitleAttribute) as? String ?? ""
            let description = attribute(element, kAXDescriptionAttribute) as? String ?? ""
            let placeholder = attribute(element, kAXPlaceholderValueAttribute) as? String ?? ""
            let enabled = attribute(element, kAXEnabledAttribute) as? Bool ?? true
            let labels = [title, description, placeholder]
            // Conversation bodies are content, not controls. Skipping their trees
            // keeps long chats from exhausting the bounded scan.
            if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
            if command == "terminal" && labels.contains("关闭终端面板") { terminalAlreadyOpen = true }
            let desktopSearch = command == "search" && ((adapter == "antigravity" && role == kAXButtonRole && labels.contains("Search Conversations"))
                || (adapter == "dsh-desktop" && role == kAXTextFieldRole && labels.contains("搜索会话名称")))
            let desktopNew = command == "new-chat" && role == kAXButtonRole &&
                ((adapter == "antigravity" && labels.contains("New Conversation")) || (adapter == "dsh-desktop" && labels.contains("新建会话")))
            let extra = extraControl(adapter: adapter, command: command, role: role, labels: labels)
            if enabled && (desktopSearch || desktopNew || extra) { matches.append(element) }
            else if enabled && ((command == "focus-input" && (role == kAXTextAreaRole || (adapter == "codex" && role == kAXTextFieldRole && (title == "Work with Codex" || description == "Work with Codex"))))
                || (command == "search" && role == kAXButtonRole && (title == "搜索" || description == "搜索" || (adapter == "codex" && (title == "Search" || description == "Search")))))
                || (enabled && command == "search" && adapter == "cindy" && role == kAXTextFieldRole && [title, description, placeholder].contains("搜索标题或内容…")) {
                matches.append(element)
            }
            if let children = attribute(element, kAXChildrenAttribute) as? [AXUIElement] { pending.append(contentsOf: children) }
        }
        guard pending.isEmpty, Date() < deadline else { return (false, "界面检查超时，未操作") }
        if terminalAlreadyOpen { return (true, "终端面板已打开") }
        if matches.isEmpty, revealTools, ["browser", "terminal"].contains(command) {
            let revealed = await perform(adapter: adapter, command: "reveal-tools", revealTools: false)
            if revealed.0 {
                try? await Task.sleep(nanoseconds: 150_000_000)
                return await perform(adapter: adapter, command: command, revealTools: false)
            }
        }
        guard matches.count == 1, let target = matches.first else {
            return (false, matches.isEmpty ? "当前应用界面未提供此操作（\(command)）；请打开任务或检查应用能力" : "存在多个目标，请先聚焦唯一任务窗口")
        }
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else { return (false, "焦点已变化，未操作") }
        let focusesField = command == "focus-input" || (attribute(target, kAXRoleAttribute) as? String == kAXTextFieldRole)
        let wasStoppingDictation = [kAXTitleAttribute, kAXDescriptionAttribute].compactMap { attribute(target, $0) as? String }.contains { $0.contains("停止") || $0.contains("结束") || $0.hasPrefix("Stop") }
        let error = focusesField
            ? AXUIElementSetAttributeValue(target, kAXFocusedAttribute as CFString, kCFBooleanTrue)
            : AXUIElementPerformAction(target, kAXPressAction as CFString)
        // AX can return cannotComplete while a focus change is still in flight.
        // Read the final state without repeating the action.
        if focusesField {
            if await confirmFocus({
                attribute(target, kAXFocusedAttribute) as? Bool == true &&
                NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier
            }) { return (true, "已确认输入框焦点；未输入或发送文本") }
            return (false, "输入框未确认获得焦点")
        }
        guard error == .success else { return (false, "目标应用拒绝辅助功能操作") }
        if command == "dictation" {
            let recordingDeadline = Date().addingTimeInterval(3)
            for _ in 0..<25 where Date() < recordingDeadline {
                try? await Task.sleep(nanoseconds: 100_000_000)
                var queue = [window], count = 0
                while let e = queue.popLast(), count < 1800, Date() < recordingDeadline {
                    count += 1
                    let labels = [kAXTitleAttribute, kAXDescriptionAttribute].compactMap { attribute(e, $0) as? String }
                    if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
                    if dictationState(labels: labels, recording: !wasStoppingDictation) { return (true, wasStoppingDictation ? "已确认听写停止；未发送消息" : "已确认听写启动；再次按键停止") }
                    if let children = attribute(e, kAXChildrenAttribute) as? [AXUIElement] { queue.append(contentsOf: children) }
                }
            }
            return (false, "已触发听写入口，但应用未确认录音状态；请检查应用语音设置")
        }
        return (true, "原生控件已接受操作（\(command)）；未发送消息")
    }
    static func extraControl(adapter: String, command: String, role: String, labels: [String]) -> Bool {
        let button = [kAXButtonRole, kAXPopUpButtonRole, kAXCheckBoxRole, "AXComboBox"].contains(role)
        guard button else { return false }
        func exact(_ names: [String]) -> Bool { labels.contains { names.contains($0) } }
        func prefix(_ value: String) -> Bool { labels.contains { $0.hasPrefix(value) } }
        if command == "model" || command == "effort" {
            // Several apps put model and reasoning effort in the same native menu.
            switch adapter {
            case "antigravity": return prefix("Select model, current:")
            case "dsh-desktop": return prefix("选择模型，当前")
            case "cindy": return prefix("选择模型。当前：")
            case "zcode": return labels.contains { $0.range(of: #"^(GLM|GPT|Claude|Gemini|DeepSeek|Qwen|Kimi|MiniMax)[- .]"#, options: .regularExpression) != nil }
            case "qwenwork": return command == "model" && (exact(["标准", "高级"]) || prefix("Qwen3.8-"))
            case "codex": return prefix("Select model") || prefix("Choose model") || prefix("选择模型") || prefix("Model selector")
            default: return false
            }
        }
        if command == "dictation" {
            switch adapter {
            case "antigravity": return exact(["Record voice memo", "Stop recording"])
            case "qwenwork": return prefix("语音输入（") || prefix("停止录音")
            case "cindy": return exact(["开始语音输入", "停止语音输入", "结束语音输入"])
            case "zcode": return exact(["开始语音输入", "停止语音输入", "Voice Input", "Stop Voice Input"])
            case "dsh-desktop": return exact(["开始听写", "停止听写", "Voice Dictation", "Stop Dictation"])
            case "codex": return exact(["Dictate", "Stop dictation", "听写", "停止听写"])
            default: return false
            }
        }
        if command == "voice-reply" || command == "toggle-voice-reply" {
            switch adapter {
            case "cindy": return exact(["语音对话", "结束语音对话", "Voice Conversation", "End Voice Conversation"])
            case "zcode": return exact(["语音回复", "关闭语音回复", "Voice Reply", "Disable Voice Reply"])
            case "dsh-desktop": return exact(["语音回复", "关闭语音回复", "Voice Reply", "Disable Voice Reply"])
            default: return false
            }
        }
        if command == "reveal-tools" { return exact(["打开右侧边栏", "打开侧边栏", "显示侧栏", "展开侧边面板"]) }
        if command == "browser" {
            if adapter == "dsh-desktop" && prefix("浏览器 浏览网页") { return true }
            return exact(["浏览器", "Browser", "Open browser", "打开浏览器"])
        }
        if command == "terminal" {
            if adapter == "dsh-desktop" && prefix("新建终端 在会话工作区") { return true }
            return exact(["切换终端", "打开终端面板", "Terminal", "Open terminal", "Toggle terminal", "终端"])
        }
        return false
    }

    static func dictationState(labels: [String], recording: Bool) -> Bool {
        let exact = recording
            ? ["停止语音输入", "结束语音输入", "Stop recording", "Stop dictation", "停止听写", "Stop Voice Input", "Stop Dictation"]
            : ["开始语音输入", "Record voice memo", "Dictate", "听写", "开始听写", "Voice Input", "Voice Dictation"]
        return labels.contains { exact.contains($0) || (recording ? $0.hasPrefix("停止录音") : $0.hasPrefix("语音输入（")) }
    }

    static func stopActiveDictationIfNeeded(adapter: String) async -> (Bool, String) {
        guard let bundle = bundles[adapter] else { return (false, "unknown_agent_adapter:\(adapter)") }
        guard ["antigravity", "qwenwork", "cindy", "codex", "zcode", "dsh-desktop"].contains(adapter) else {
            return (true, "dictation_not_applicable:\(adapter)")
        }
        guard let app = NSWorkspace.shared.frontmostApplication, app.bundleIdentifier == bundle else {
            return (true, "dictation_inactive:\(adapter)")
        }
        guard AXIsProcessTrusted() else { return (false, "需要辅助功能权限") }
        let root = AXUIElementCreateApplication(app.processIdentifier)
        AXUIElementSetMessagingTimeout(root, 0.3)
        var focusedWindow: AXUIElement?
        if let value = attribute(root, kAXFocusedWindowAttribute), CFGetTypeID(value) == AXUIElementGetTypeID() {
            focusedWindow = unsafeBitCast(value, to: AXUIElement.self)
        } else if let windows = attribute(root, kAXWindowsAttribute) as? [AXUIElement], windows.count == 1 {
            focusedWindow = windows[0]
        }
        guard let window = focusedWindow else { return (true, "dictation_inactive:\(adapter)") }
        var pending = [window]
        var recordingControls: [AXUIElement] = []
        var visited = 0
        while let element = pending.popLast(), visited < 1800 {
            visited += 1
            let role = attribute(element, kAXRoleAttribute) as? String ?? ""
            let title = attribute(element, kAXTitleAttribute) as? String ?? ""
            let description = attribute(element, kAXDescriptionAttribute) as? String ?? ""
            let placeholder = attribute(element, kAXPlaceholderValueAttribute) as? String ?? ""
            let labels = [title, description, placeholder]
            if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
            if extraControl(adapter: adapter, command: "dictation", role: role, labels: labels),
               dictationState(labels: labels, recording: true) {
                recordingControls.append(element)
            }
            if let children = attribute(element, kAXChildrenAttribute) as? [AXUIElement] {
                pending.append(contentsOf: children)
            }
        }
        if recordingControls.isEmpty {
            return (true, "dictation_inactive:\(adapter)")
        }
        let stopped = await perform(adapter: adapter, command: "dictation", revealTools: false)
        if stopped.0 && stopped.1.contains("停止") {
            return (true, "dictation_stopped:\(adapter)")
        }
        return (false, "dictation_stop_unconfirmed:\(adapter):\(stopped.1)")
    }

    static func confirmFocus(_ read: () -> Bool, interval: UInt64 = 25_000_000) async -> Bool {
        for _ in 0..<20 {
            guard !Task.isCancelled else { return false }
            if read() { return true }
            try? await Task.sleep(nanoseconds: interval)
        }
        return false
    }

    static var scrollTrustChecker: () -> Bool = { AXIsProcessTrusted() }
    static var scrollRunningProcessID: (String) -> pid_t? = { bundleID in
        NSRunningApplication.runningApplications(withBundleIdentifier: bundleID).first(where: { !$0.isTerminated })?.processIdentifier
    }
    static var scrollWindowFinder: (pid_t) -> DialScrollWindow? = { pid in
        let list = (CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]]) ?? []
        return DialScrollWindow.frontmost(in: list, processID: pid)
    }
    static var scrollMouseLocation: () -> CGPoint? = {
        CGEvent(source: nil)?.location
    }
    static var scrollMouseWarper: (CGPoint) -> Void = { point in
        CGWarpMouseCursorPosition(point)
    }
    static var scrollWheelPoster: (Int32, Int) -> Bool = { wheelDelta, windowNumber in
        guard let source = CGEventSource(stateID: .hidSystemState),
              let event = CGEvent(
                  scrollWheelEvent2Source: source,
                  units: .line,
                  wheelCount: 1,
                  wheel1: wheelDelta,
                  wheel2: 0,
                  wheel3: 0
              ) else { return false }
        event.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: Int64(windowNumber))
        event.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: Int64(windowNumber))
        event.post(tap: .cghidEventTap)
        return true
    }
    static var scrollSleeper: (UInt64) async -> Void = { nanoseconds in
        try? await Task.sleep(nanoseconds: nanoseconds)
    }
    static var scrollInputFocuser: (String, pid_t) -> Bool = { adapter, pid in
        focusInputWithoutStealingActivation(adapter: adapter, pid: pid)
    }

    static func resetScrollHooks() {
        scrollTrustChecker = { AXIsProcessTrusted() }
        scrollRunningProcessID = { bundleID in
            NSRunningApplication.runningApplications(withBundleIdentifier: bundleID).first(where: { !$0.isTerminated })?.processIdentifier
        }
        scrollWindowFinder = { pid in
            let list = (CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]]) ?? []
            return DialScrollWindow.frontmost(in: list, processID: pid)
        }
        scrollMouseLocation = { CGEvent(source: nil)?.location }
        scrollMouseWarper = { point in CGWarpMouseCursorPosition(point) }
        scrollWheelPoster = { wheelDelta, windowNumber in
            guard let source = CGEventSource(stateID: .hidSystemState),
                  let event = CGEvent(
                      scrollWheelEvent2Source: source,
                      units: .line,
                      wheelCount: 1,
                      wheel1: wheelDelta,
                      wheel2: 0,
                      wheel3: 0
                  ) else { return false }
            event.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: Int64(windowNumber))
            event.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: Int64(windowNumber))
            event.post(tap: .cghidEventTap)
            return true
        }
        scrollSleeper = { nanoseconds in try? await Task.sleep(nanoseconds: nanoseconds) }
        scrollInputFocuser = { adapter, pid in
            focusInputWithoutStealingActivation(adapter: adapter, pid: pid)
        }
    }

    static func performScroll(adapter: String, command: String) async -> (Bool, String) {
        guard let bundle = bundles[adapter],
              let request = DialScrollRequest.from(command: command) else {
            return (false, "未接入的滚动操作")
        }
        guard scrollTrustChecker() else {
            return (false, "需要辅助功能权限")
        }
        guard let pid = scrollRunningProcessID(bundle) else {
            return (false, "目标应用未运行，未滚动且不改变当前窗口焦点")
        }
        guard let window = scrollWindowFinder(pid) else {
            return (false, "目标应用没有可滚动的可见窗口，未滚动且不改变当前窗口焦点")
        }
        let targetPoint = window.scrollPoint(fromTop: request.fromTop)
        let savedLocation = scrollMouseLocation()
        scrollMouseWarper(targetPoint)
        defer {
            if let savedLocation {
                scrollMouseWarper(savedLocation)
            }
        }
        switch request.mode {
        case .lines(let lines):
            let delta = DialScrollRequest.wheelDelta(lines: lines)
            guard scrollWheelPoster(delta, window.windowNumber) else {
                return (false, "滚动事件合成失败")
            }
            return (true, "\(adapter)/\(request.canonicalCommand) (lines=\(lines) window=\(window.windowNumber))")
        case .bottom:
            let stepDelta = DialScrollRequest.wheelDelta(lines: DialScrollRequest.maximumLines)
            let steps = DialScrollRequest.edgeLines / DialScrollRequest.maximumLines
            for index in 0..<steps {
                guard scrollWheelPoster(stepDelta, window.windowNumber) else {
                    return (false, "跳到底部滚动事件合成失败")
                }
                if index + 1 < steps {
                    await scrollSleeper(DialScrollRequest.burstIntervalNanoseconds)
                }
            }
            let focused = scrollInputFocuser(adapter, pid)
            return (true, "\(adapter)/scroll-to-bottom (lines=\(DialScrollRequest.edgeLines) window=\(window.windowNumber) focused=\(focused))")
        }
    }

    private static func focusInputWithoutStealingActivation(adapter: String, pid: pid_t) -> Bool {
        // Only focus the AX input field if the target app is already frontmost, so dial press
        // never steals keyboard focus from a non-target window.
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == pid else {
            return false
        }
        let root = AXUIElementCreateApplication(pid)
        AXUIElementSetMessagingTimeout(root, 0.25)
        let window: AXUIElement?
        if let value = attribute(root, kAXFocusedWindowAttribute), CFGetTypeID(value) == AXUIElementGetTypeID() {
            window = unsafeBitCast(value, to: AXUIElement.self)
        } else if let windows = attribute(root, kAXWindowsAttribute) as? [AXUIElement], windows.count == 1 {
            window = windows[0]
        } else {
            window = nil
        }
        guard let window else { return false }
        var pending = [window]
        var matches: [AXUIElement] = []
        var visited = 0
        while let element = pending.popLast(), visited < 1200 {
            visited += 1
            let role = attribute(element, kAXRoleAttribute) as? String ?? ""
            let title = attribute(element, kAXTitleAttribute) as? String ?? ""
            let description = attribute(element, kAXDescriptionAttribute) as? String ?? ""
            let placeholder = attribute(element, kAXPlaceholderValueAttribute) as? String ?? ""
            let enabled = attribute(element, kAXEnabledAttribute) as? Bool ?? true
            let labels = [title, description, placeholder]
            if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
            if enabled && (role == kAXTextAreaRole || (adapter == "codex" && role == kAXTextFieldRole && (title == "Work with Codex" || description == "Work with Codex"))) {
                matches.append(element)
            }
            if let children = attribute(element, kAXChildrenAttribute) as? [AXUIElement] {
                pending.append(contentsOf: children)
            }
        }
        guard matches.count == 1, let target = matches.first else { return false }
        return AXUIElementSetAttributeValue(target, kAXFocusedAttribute as CFString, kCFBooleanTrue) == .success
    }

}

struct DialScrollRequest: Equatable, Sendable {
    static let standardLines = 3
    static let fastLines = 15
    static let maximumLines = 100
    static let edgeLines = 1_000
    static let burstIntervalNanoseconds: UInt64 = 16_000_000
    static let defaultFromTop = 0.4

    enum Mode: Equatable, Sendable {
        case lines(Int)
        case bottom
    }

    let mode: Mode
    let fromTop: Double

    init(mode: Mode, fromTop: Double = Self.defaultFromTop) {
        self.mode = mode
        self.fromTop = max(0.15, min(0.85, fromTop))
    }

    static func wheelDelta(lines: Int) -> Int32 {
        let clamped = max(-maximumLines, min(maximumLines, lines))
        return Int32(-clamped)
    }

    static func from(command: String, dialHeld: Bool = false) -> DialScrollRequest? {
        switch command {
        case "scroll-up":
            return DialScrollRequest(mode: .lines(dialHeld ? -fastLines : -standardLines))
        case "scroll-down":
            return DialScrollRequest(mode: .lines(dialHeld ? fastLines : standardLines))
        case "scroll-up-fast":
            return DialScrollRequest(mode: .lines(-fastLines))
        case "scroll-down-fast":
            return DialScrollRequest(mode: .lines(fastLines))
        case "scroll-to-bottom", "jump-to-bottom":
            return DialScrollRequest(mode: .bottom)
        default:
            guard command.hasPrefix("scroll-lines:"),
                  let parsed = Int(command.dropFirst("scroll-lines:".count)),
                  parsed != 0 else {
                return nil
            }
            let clamped = max(-maximumLines, min(maximumLines, parsed))
            return DialScrollRequest(mode: .lines(clamped))
        }
    }

    var canonicalCommand: String {
        switch mode {
        case .bottom:
            return "scroll-to-bottom"
        case .lines(let rawLines):
            let lines = max(-Self.maximumLines, min(Self.maximumLines, rawLines))
            switch lines {
            case -Self.standardLines: return "scroll-up"
            case Self.standardLines: return "scroll-down"
            case -Self.fastLines: return "scroll-up-fast"
            case Self.fastLines: return "scroll-down-fast"
            default: return "scroll-lines:\(lines)"
            }
        }
    }

    func coalesced(with incoming: DialScrollRequest) -> DialScrollRequest? {
        guard case .lines(let current) = mode,
              case .lines(let delta) = incoming.mode else {
            return nil
        }
        let sum = current + delta
        let resolved = sum == 0 ? (delta >= 0 ? Self.standardLines : -Self.standardLines) : max(-Self.maximumLines, min(Self.maximumLines, sum))
        return DialScrollRequest(mode: .lines(resolved), fromTop: incoming.fromTop)
    }
}

struct DialScrollWindow: Equatable, Sendable {
    let windowNumber: Int
    let bounds: CGRect

    func scrollPoint(fromTop: Double = DialScrollRequest.defaultFromTop) -> CGPoint {
        let ratio = max(0.15, min(0.85, fromTop))
        return CGPoint(x: bounds.midX, y: bounds.minY + bounds.height * ratio)
    }

    static func frontmost(in windowList: [[String: Any]], processID: pid_t) -> DialScrollWindow? {
        for info in windowList {
            guard let ownerPID = info[kCGWindowOwnerPID as String] as? pid_t,
                  ownerPID == processID else { continue }
            let layer = info[kCGWindowLayer as String] as? Int ?? 0
            guard layer == 0,
                  let windowNumber = info[kCGWindowNumber as String] as? Int,
                  let boundsDict = info[kCGWindowBounds as String] as? NSDictionary,
                  let bounds = CGRect(dictionaryRepresentation: boundsDict),
                  bounds.width >= 100,
                  bounds.height >= 100 else { continue }
            return DialScrollWindow(windowNumber: windowNumber, bounds: bounds)
        }
        return nil
    }
}

struct AgentBadge: View {
    let name: String
    var size: CGFloat = 28
    var body: some View {
        Group {
            if let agent = DesktopAgent.named(name), let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: agent.bundleID) {
                Image(nsImage: NSWorkspace.shared.icon(forFile: url.path)).resizable().interpolation(.high)
            } else {
                Image(systemName: DesktopAgent.named(name)?.symbol ?? "square.stack")
                    .resizable().scaledToFit().padding(5).foregroundStyle(.secondary)
            }
        }.frame(width: size, height: size).accessibilityHidden(true)
    }
}
