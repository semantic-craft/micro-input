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
        .init(name: "Cindy", bundleID: "com.xd.cindy", symbol: "sparkles", capabilities: "打开应用 · 搜索任务 · 聚焦输入"),
        .init(name: "Codex", bundleID: "com.openai.codex", symbol: "terminal", capabilities: "打开应用 · 搜索 · 聚焦输入"),
        .init(name: "千问办公", bundleID: "cn.qwenwork.desktop.mac", symbol: "text.bubble", capabilities: "打开应用 · 新任务 · 搜索 · 聚焦输入"),
        .init(name: "ZCode", bundleID: "dev.zcode.app", symbol: "chevron.left.forwardslash.chevron.right", capabilities: "打开应用 · 新任务 · 搜索 · 聚焦输入"),
        .init(name: "Antigravity", bundleID: "com.google.antigravity", symbol: "sparkles", capabilities: "打开应用 · 新会话 · 搜索 · 聚焦输入"),
        .init(name: "DeepSeek Coder (DSH)", bundleID: "com.deepseek.dsh", symbol: "terminal", capabilities: "打开应用 · 新会话 · 搜索 · 聚焦输入"),
        .init(name: "Qoder CN", bundleID: "com.qodercn.app", symbol: "q.circle", capabilities: "打开应用 · 新任务 · 搜索 · 聚焦输入"),
        .init(name: "Herdr", bundleID: "dev.herdr.app", symbol: "rectangle.split.2x2", capabilities: "Space · Tab · 分屏 · 面板导航")
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
}

// Restrict AX actions to one focused window in an allowlisted app. Never pick the
// first of multiple editors, send text, or invoke approval controls.
@MainActor enum DesktopAgentControls {
    static let bundles = ["codex": "com.openai.codex", "cindy": "com.xd.cindy", "qwenwork": "cn.qwenwork.desktop.mac", "zcode": "dev.zcode.app", "antigravity": "com.google.antigravity", "dsh-desktop": "com.deepseek.dsh", "qoder-cn": "com.qodercn.app"]
    static func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success else { return nil }
        return value
    }
    static func perform(adapter: String, command: String, revealTools: Bool = true) async -> (Bool, String) {
        guard let bundle = bundles[adapter], ["focus-input", "search", "new-chat", "model", "effort", "dictation", "browser", "terminal", "reveal-tools"].contains(command),
              !(adapter == "qwenwork" && command == "search"),
              command != "new-chat" || ["antigravity", "dsh-desktop", "qoder-cn"].contains(adapter) else { return (false, "未接入的应用操作") }
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
            let desktopSearch = command == "search" && ((adapter == "qoder-cn" && role == kAXButtonRole && labels.contains("搜索 ⌘+G")) || (adapter == "antigravity" && role == kAXButtonRole && labels.contains("Search Conversations"))
                || (adapter == "dsh-desktop" && role == kAXTextFieldRole && labels.contains("搜索会话名称")))
            let desktopNew = command == "new-chat" && role == kAXButtonRole &&
                ((adapter == "antigravity" && labels.contains("New Conversation")) || (adapter == "dsh-desktop" && labels.contains("新建会话")))
            let qoderNew = command == "new-chat" && adapter == "qoder-cn" && role == "AXLink" && labels.contains("新的任务 ⌘+N")
            let extra = extraControl(adapter: adapter, command: command, role: role, labels: labels)
            if enabled && (desktopSearch || desktopNew || qoderNew || extra) { matches.append(element) }
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
            case "qoder-cn": return command == "model" && exact(["模型"])
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
            case "cindy", "qoder-cn": return exact(["开始语音输入", "停止语音输入", "结束语音输入"])
            case "codex": return exact(["Dictate", "Stop dictation", "听写", "停止听写"])
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
        let exact = recording ? ["停止语音输入", "结束语音输入", "Stop recording", "Stop dictation"] : ["开始语音输入", "Record voice memo", "Dictate"]
        return labels.contains { exact.contains($0) || (recording ? $0.hasPrefix("停止录音") : $0.hasPrefix("语音输入（")) }
    }

    static func confirmFocus(_ read: () -> Bool, interval: UInt64 = 25_000_000) async -> Bool {
        for _ in 0..<20 {
            guard !Task.isCancelled else { return false }
            if read() { return true }
            try? await Task.sleep(nanoseconds: interval)
        }
        return false
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
