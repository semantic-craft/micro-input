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
        .init(name: "ZCode", bundleID: "dev.zcode.app", symbol: "chevron.left.forwardslash.chevron.right", capabilities: "打开应用 · 新任务 · 搜索 · 聚焦输入")
    ]
    static func named(_ name: String) -> DesktopAgent? { all.first { $0.name == name } }
}

// Restrict AX actions to one focused window in an allowlisted app. Never pick the
// first of multiple editors, send text, or invoke approval controls.
@MainActor enum DesktopAgentControls {
    static let bundles = ["codex": "com.openai.codex", "cindy": "com.xd.cindy", "qwenwork": "cn.qwenwork.desktop.mac", "zcode": "dev.zcode.app"]
    static func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success else { return nil }
        return value
    }
    static func perform(adapter: String, command: String) async -> (Bool, String) {
        guard let bundle = bundles[adapter], ["focus-input", "search"].contains(command),
              !(adapter == "qwenwork" && command == "search") else { return (false, "未接入的应用操作") }
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
        let deadline = Date().addingTimeInterval(1.5)
        while let element = pending.popLast(), visited < 1800, Date() < deadline {
            visited += 1
            let role = attribute(element, kAXRoleAttribute) as? String ?? ""
            let title = attribute(element, kAXTitleAttribute) as? String ?? ""
            let description = attribute(element, kAXDescriptionAttribute) as? String ?? ""
            let placeholder = attribute(element, kAXPlaceholderValueAttribute) as? String ?? ""
            let enabled = attribute(element, kAXEnabledAttribute) as? Bool ?? true
            if enabled && ((command == "focus-input" && (role == kAXTextAreaRole || (adapter == "codex" && role == kAXTextFieldRole && (title == "Work with Codex" || description == "Work with Codex"))))
                || (command == "search" && role == kAXButtonRole && (title == "搜索" || description == "搜索" || (adapter == "codex" && (title == "Search" || description == "Search")))))
                || (enabled && command == "search" && adapter == "cindy" && role == kAXTextFieldRole && [title, description, placeholder].contains("搜索标题或内容…")) {
                matches.append(element)
            }
            if let children = attribute(element, kAXChildrenAttribute) as? [AXUIElement] { pending.append(contentsOf: children) }
        }
        guard pending.isEmpty, Date() < deadline else { return (false, "界面检查超时，未操作") }
        guard matches.count == 1, let target = matches.first else {
            return (false, matches.isEmpty ? "当前界面没有此操作；请先打开任务" : "存在多个目标，请先聚焦唯一任务窗口")
        }
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else { return (false, "焦点已变化，未操作") }
        let focusesField = command == "focus-input" || (attribute(target, kAXRoleAttribute) as? String == kAXTextFieldRole)
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
        return (true, "搜索控件已接受操作；未发送消息")
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
    var body: some View {
        Image(systemName: DesktopAgent.named(name)?.symbol ?? "square.stack")
            .font(.system(size: 15, weight: .medium)).foregroundStyle(.tint)
            .frame(width: 32, height: 32)
            .background(.tint.opacity(0.09), in: RoundedRectangle(cornerRadius: 8))
    }
}
