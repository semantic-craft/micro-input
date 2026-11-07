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
        .init(name: "Antigravity", bundleID: "com.google.antigravity", symbol: "sparkles", capabilities: "打开应用、左旋钮会话导航、点按切换听写（Ctrl+M）、Live 实时语音、选模型/项目、浏览器/终端/分栏"),
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
    static var shortcutPoster: (CGKeyCode, CGEventFlags) -> Bool = { keyCode, flags in
        guard let source = CGEventSource(stateID: .hidSystemState),
              let down = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: false) else {
            return false
        }
        down.flags = flags
        up.flags = flags
        down.post(tap: .cghidEventTap)
        up.post(tap: .cghidEventTap)
        return true
    }

    static var lastAntigravityConversationTitle: String?
    static var lastAntigravityProjectTitle: String?

    static func isSidebarConversationLink(role: String, labels: [String], x: CGFloat?) -> Bool {
        guard role == "AXLink" else { return false }
        if let x, x >= 360 { return false }
        let trimmed = labels.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        guard let primary = trimmed.first else { return false }
        let excludedExact: Set<String> = [
            "New Conversation", "Customizations", "New Conversation in Project",
            "Projects", "Conversations"
        ]
        if excludedExact.contains(primary) { return false }
        if trimmed.contains(where: { $0.contains("Opens external link") || $0.hasPrefix("http://") || $0.hasPrefix("https://") }) {
            return false
        }
        return true
    }

    static func isSidebarProjectButton(role: String, title: String, x: CGFloat?, width: CGFloat?) -> Bool {
        guard role == kAXButtonRole else { return false }
        if let x, x >= 280 { return false }
        if let width, width < 140 { return false }
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return false }
        let excluded: Set<String> = [
            "Projects", "Conversations", "Automations", "Settings",
            "New Conversation", "Customizations", "Search Conversations",
            "Collapse Sidebar", "Expand Sidebar", "Archive conversation",
            "Pin conversation", "Stop execution", "New Conversation in Project"
        ]
        return !excluded.contains(trimmed)
    }

    private static func elementPosition(_ element: AXUIElement) -> CGPoint? {
        guard let raw = attribute(element, kAXPositionAttribute),
              CFGetTypeID(raw) == AXValueGetTypeID() else { return nil }
        let axVal = unsafeBitCast(raw, to: AXValue.self)
        var point = CGPoint.zero
        guard AXValueGetValue(axVal, .cgPoint, &point) else { return nil }
        return point
    }

    private static func elementSize(_ element: AXUIElement) -> CGSize? {
        guard let raw = attribute(element, kAXSizeAttribute),
              CFGetTypeID(raw) == AXValueGetTypeID() else { return nil }
        let axVal = unsafeBitCast(raw, to: AXValue.self)
        var size = CGSize.zero
        guard AXValueGetValue(axVal, .cgSize, &size) else { return nil }
        return size
    }

    static func selectAntigravityConversation(title: String, workspacePath: String) async -> (Bool, String) {
        guard let bundle = bundles["antigravity"], AXIsProcessTrusted() else {
            return (false, "需要辅助功能权限")
        }
        if NSWorkspace.shared.frontmostApplication?.bundleIdentifier != bundle {
            if let app = NSRunningApplication.runningApplications(withBundleIdentifier: bundle).first(where: { !$0.isTerminated }) {
                _ = app.activate(options: [.activateIgnoringOtherApps])
            } else if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundle) {
                let config = NSWorkspace.OpenConfiguration()
                config.activates = true
                _ = try? await NSWorkspace.shared.openApplication(at: url, configuration: config)
            }
            for _ in 0..<20 {
                if NSWorkspace.shared.frontmostApplication?.bundleIdentifier == bundle { break }
                try? await Task.sleep(nanoseconds: 25_000_000)
            }
        }
        guard let app = NSRunningApplication.runningApplications(withBundleIdentifier: bundle).first(where: { !$0.isTerminated }) else {
            return (false, "Antigravity 未运行")
        }
        let root = AXUIElementCreateApplication(app.processIdentifier)
        AXUIElementSetMessagingTimeout(root, 0.35)
        var focusedWindow: AXUIElement?
        for _ in 0..<16 {
            if let value = attribute(root, kAXFocusedWindowAttribute), CFGetTypeID(value) == AXUIElementGetTypeID() {
                focusedWindow = unsafeBitCast(value, to: AXUIElement.self); break
            }
            if let windows = attribute(root, kAXWindowsAttribute) as? [AXUIElement], !windows.isEmpty {
                focusedWindow = windows[0]; break
            }
            try? await Task.sleep(nanoseconds: 25_000_000)
        }
        guard let window = focusedWindow else {
            return (false, "未找到 Antigravity 窗口")
        }
        let cleanTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        let projectName = URL(fileURLWithPath: workspacePath).lastPathComponent.trimmingCharacters(in: .whitespacesAndNewlines)

        func scanSidebar() -> (links: [(element: AXUIElement, title: String, y: CGFloat)], projects: [(element: AXUIElement, title: String, y: CGFloat)]) {
            var pending = [window]
            var links: [(element: AXUIElement, title: String, y: CGFloat)] = []
            var projects: [(element: AXUIElement, title: String, y: CGFloat)] = []
            var visited = 0
            let deadline = Date().addingTimeInterval(0.8)
            while let element = pending.popLast(), visited < 1800, Date() < deadline {
                visited += 1
                let role = attribute(element, kAXRoleAttribute) as? String ?? ""
                let t = attribute(element, kAXTitleAttribute) as? String ?? ""
                let desc = attribute(element, kAXDescriptionAttribute) as? String ?? ""
                let labels = [t, desc]
                if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
                if role == "AXLink" {
                    let pos = elementPosition(element)
                    if isSidebarConversationLink(role: role, labels: labels, x: pos?.x),
                       let primary = labels.map({ $0.trimmingCharacters(in: .whitespacesAndNewlines) }).first(where: { !$0.isEmpty }) {
                        links.append((element, primary, pos?.y ?? CGFloat(visited)))
                    }
                } else if role == kAXButtonRole {
                    let pos = elementPosition(element)
                    let sz = elementSize(element)
                    if isSidebarProjectButton(role: role, title: t, x: pos?.x, width: sz?.width) {
                        projects.append((element, t.trimmingCharacters(in: .whitespacesAndNewlines), pos?.y ?? CGFloat(visited)))
                    }
                }
                if let children = attribute(element, kAXChildrenAttribute) as? [AXUIElement] {
                    pending.append(contentsOf: children)
                }
            }
            return (links.sorted { $0.y < $1.y }, projects.sorted { $0.y < $1.y })
        }

        func findMatchingLink(in links: [(element: AXUIElement, title: String, y: CGFloat)]) -> (element: AXUIElement, title: String, y: CGFloat)? {
            guard !cleanTitle.isEmpty else { return nil }
            if let exact = links.first(where: { $0.title.caseInsensitiveCompare(cleanTitle) == .orderedSame }) {
                return exact
            }
            if cleanTitle.count >= 3,
               let partial = links.first(where: { $0.title.localizedCaseInsensitiveContains(cleanTitle) || cleanTitle.localizedCaseInsensitiveContains($0.title) }) {
                return partial
            }
            return nil
        }

        let initial = scanSidebar()
        if let matched = findMatchingLink(in: initial.links) {
            if AXUIElementPerformAction(matched.element, kAXPressAction as CFString) == .success {
                lastAntigravityConversationTitle = matched.title
                if !projectName.isEmpty { lastAntigravityProjectTitle = projectName }
                return (true, "已跳转到会话：\(matched.title)")
            }
        }

        // If the conversation's project is collapsed in the sidebar, expand the project first and then click the conversation link.
        if !projectName.isEmpty,
           let proj = initial.projects.first(where: { $0.title.caseInsensitiveCompare(projectName) == .orderedSame }) {
            _ = AXUIElementPerformAction(proj.element, kAXPressAction as CFString)
            lastAntigravityProjectTitle = proj.title
            try? await Task.sleep(nanoseconds: 140_000_000)
            let afterExpand = scanSidebar()
            if let matched = findMatchingLink(in: afterExpand.links) {
                if AXUIElementPerformAction(matched.element, kAXPressAction as CFString) == .success {
                    lastAntigravityConversationTitle = matched.title
                    return (true, "已展开项目 \(proj.title) 并跳转到会话：\(matched.title)")
                }
            }
            // Even if the specific conversation title wasn't matched, if any link appeared under this project, open the first one under it.
            if let firstUnderProj = afterExpand.links.first(where: { $0.y > proj.y && $0.y < proj.y + 220 }) {
                if AXUIElementPerformAction(firstUnderProj.element, kAXPressAction as CFString) == .success {
                    lastAntigravityConversationTitle = firstUnderProj.title
                    return (true, "已展开项目 \(proj.title) 并打开会话：\(firstUnderProj.title)")
                }
            }
            return (true, "已定位到项目：\(proj.title)")
        }

        return (true, "已唤起 Antigravity 窗口")
    }

    static func performAntigravityProjectStep(window: AXUIElement, app: NSRunningApplication, next: Bool) -> (Bool, String) {
        struct ProjectEntry {
            let element: AXUIElement
            let title: String
            let y: CGFloat
            let order: Int
        }
        var pending = [window]
        var projects: [ProjectEntry] = []
        var linkYs: [CGFloat] = []
        var visited = 0
        let deadline = Date().addingTimeInterval(0.9)
        while let element = pending.popLast(), visited < 1800, Date() < deadline {
            visited += 1
            let role = attribute(element, kAXRoleAttribute) as? String ?? ""
            let title = attribute(element, kAXTitleAttribute) as? String ?? ""
            let desc = attribute(element, kAXDescriptionAttribute) as? String ?? ""
            let labels = [title, desc]
            if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
            if role == kAXButtonRole {
                let pos = elementPosition(element)
                let sz = elementSize(element)
                if isSidebarProjectButton(role: role, title: title, x: pos?.x, width: sz?.width) {
                    projects.append(ProjectEntry(element: element, title: title.trimmingCharacters(in: .whitespacesAndNewlines), y: pos?.y ?? CGFloat(visited), order: visited))
                }
            } else if role == "AXLink" {
                let pos = elementPosition(element)
                if isSidebarConversationLink(role: role, labels: labels, x: pos?.x), let y = pos?.y {
                    linkYs.append(y)
                }
            }
            if let children = attribute(element, kAXChildrenAttribute) as? [AXUIElement] {
                pending.append(contentsOf: children)
            }
        }
        let sorted = projects.sorted {
            if abs($0.y - $1.y) > 2 { return $0.y < $1.y }
            return $0.order < $1.order
        }
        var deduped: [ProjectEntry] = []
        for item in sorted {
            if let last = deduped.last, last.title == item.title && abs(last.y - item.y) <= 4 { continue }
            deduped.append(item)
        }
        guard !deduped.isEmpty else {
            return (false, "未在左侧栏找到项目列表")
        }
        // Determine current project index from lastAntigravityProjectTitle or whichever project currently has expanded conversation links below it
        let expandedIndex: Int? = (0..<deduped.count).first { idx in
            let minY = deduped[idx].y
            let maxY = (idx + 1 < deduped.count) ? deduped[idx + 1].y : (minY + 260)
            return linkYs.contains { $0 > minY && $0 < maxY }
        }
        let currentIndex = lastAntigravityProjectTitle.flatMap { last in deduped.firstIndex(where: { $0.title == last }) }
            ?? expandedIndex
        let targetIndex: Int
        if let currentIndex {
            targetIndex = next ? min(deduped.count - 1, currentIndex + 1) : max(0, currentIndex - 1)
        } else {
            targetIndex = 0
        }
        let target = deduped[targetIndex]
        if AXUIElementPerformAction(target.element, kAXPressAction as CFString) == .success {
            lastAntigravityProjectTitle = target.title
            return (true, "已切换到侧栏\(next ? "下一" : "上一")项目（\(target.title)）")
        }
        return (false, "侧栏项目按钮未能响应操作")
    }

    private static func performAntigravityConversationStep(window: AXUIElement, app: NSRunningApplication, next: Bool) -> (Bool, String) {
        struct LinkEntry {
            let element: AXUIElement
            let title: String
            let x: CGFloat
            let y: CGFloat
            let order: Int
        }
        var pending = [window]
        var entries: [LinkEntry] = []
        var visited = 0
        let deadline = Date().addingTimeInterval(1.0)
        while let element = pending.popLast(), visited < 1800, Date() < deadline {
            visited += 1
            let role = attribute(element, kAXRoleAttribute) as? String ?? ""
            let title = attribute(element, kAXTitleAttribute) as? String ?? ""
            let description = attribute(element, kAXDescriptionAttribute) as? String ?? ""
            let placeholder = attribute(element, kAXPlaceholderValueAttribute) as? String ?? ""
            let help = attribute(element, kAXHelpAttribute) as? String ?? ""
            let enabled = attribute(element, kAXEnabledAttribute) as? Bool ?? true
            let labels = [title, description, placeholder, help]
            if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
            let pos = role == "AXLink" ? elementPosition(element) : nil
            if enabled, isSidebarConversationLink(role: role, labels: labels, x: pos?.x),
               let primary = labels.map({ $0.trimmingCharacters(in: .whitespacesAndNewlines) }).first(where: { !$0.isEmpty }) {
                entries.append(LinkEntry(element: element, title: primary, x: pos?.x ?? 0, y: pos?.y ?? CGFloat(visited), order: visited))
            }
            if let children = attribute(element, kAXChildrenAttribute) as? [AXUIElement] {
                pending.append(contentsOf: children)
            }
        }
        let sorted = entries.sorted {
            if abs($0.y - $1.y) > 2 { return $0.y < $1.y }
            if abs($0.x - $1.x) > 2 { return $0.x < $1.x }
            return $0.order < $1.order
        }
        var deduped: [LinkEntry] = []
        for item in sorted {
            if let last = deduped.last, last.title == item.title && abs(last.y - item.y) <= 4 { continue }
            deduped.append(item)
        }
        if !deduped.isEmpty, NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier {
            let windowTitle = attribute(window, kAXTitleAttribute) as? String ?? ""
            let activeWindowPrefix: String? = {
                if let range = windowTitle.range(of: " - Antigravity") {
                    let prefix = String(windowTitle[..<range.lowerBound]).trimmingCharacters(in: .whitespacesAndNewlines)
                    return prefix.isEmpty || prefix == "New Conversation" ? nil : prefix
                }
                return nil
            }()
            let currentIndex = activeWindowPrefix.flatMap { active in deduped.firstIndex(where: { $0.title == active }) }
                ?? lastAntigravityConversationTitle.flatMap { last in deduped.firstIndex(where: { $0.title == last }) }
            let targetIndex: Int
            if let currentIndex {
                targetIndex = next ? min(deduped.count - 1, currentIndex + 1) : max(0, currentIndex - 1)
            } else {
                targetIndex = 0
            }
            let target = deduped[targetIndex]
            if AXUIElementPerformAction(target.element, kAXPressAction as CFString) == .success {
                lastAntigravityConversationTitle = target.title
                return (true, "已切换到侧栏\(next ? "下一" : "上一")会话（\(target.title)）；未发送消息")
            }
        }
        if next {
            guard shortcutPoster(0x7D, .maskAlternate) else { return (false, "下一会话快捷键事件合成失败") }
            return (true, "已切换到侧栏下一会话（Alt+Down）；未发送消息")
        } else {
            guard shortcutPoster(0x7E, .maskAlternate) else { return (false, "上一会话快捷键事件合成失败") }
            return (true, "已切换到侧栏上一会话（Alt+Up）；未发送消息")
        }
    }

    static func stepAntigravitySidebarFilteredRow(filter: AntigravitySessionFilter, next: Bool) -> (Bool, String) {
        guard AXIsProcessTrusted(),
              let app = NSRunningApplication.runningApplications(withBundleIdentifier: "com.google.antigravity").first else {
            return (false, "Antigravity 未运行或缺少辅助功能权限")
        }
        let root = AXUIElementCreateApplication(app.processIdentifier)
        AXUIElementSetMessagingTimeout(root, 0.3)
        guard let window = (attribute(root, kAXFocusedWindowAttribute) as! AXUIElement?)
            ?? (attribute(root, kAXWindowsAttribute) as? [AXUIElement])?.first else {
            return (false, "未找到 Antigravity 窗口")
        }
        struct RowCandidate {
            let link: AXUIElement
            let title: String
            let y: CGFloat
            let isWorking: Bool
            let isUnreadCompleted: Bool
        }
        var pending = [window]
        var rows: [RowCandidate] = []
        var visited = 0
        let deadline = Date().addingTimeInterval(0.9)
        while let element = pending.popLast(), visited < 1800, Date() < deadline {
            visited += 1
            let role = attribute(element, kAXRoleAttribute) as? String ?? ""
            let domClasses = attribute(element, "AXDOMClassList") as? [String] ?? []
            if role == kAXGroupRole && domClasses.contains("agy-sidebar-row") {
                var rowLink: AXUIElement?
                var rowTitle = ""
                var hasStopButton = false
                var hasArchiveButton = false
                var hasTimeText = false
                var subQueue = (attribute(element, kAXChildrenAttribute) as? [AXUIElement]) ?? []
                while let sub = subQueue.popLast() {
                    let sRole = attribute(sub, kAXRoleAttribute) as? String ?? ""
                    let sTitle = attribute(sub, kAXTitleAttribute) as? String ?? ""
                    let sDesc = attribute(sub, kAXDescriptionAttribute) as? String ?? ""
                    let sVal = attribute(sub, kAXValueAttribute) as? String ?? ""
                    if sRole == "AXLink" {
                        rowLink = sub
                        rowTitle = (sDesc.isEmpty ? sTitle : sDesc).trimmingCharacters(in: .whitespacesAndNewlines)
                    } else if sRole == kAXButtonRole {
                        if sDesc == "Stop execution" || sTitle == "Stop execution" {
                            hasStopButton = true
                        } else if sDesc == "Archive conversation" || sTitle == "Archive conversation" {
                            hasArchiveButton = true
                        }
                    } else if sRole == kAXStaticTextRole {
                        let txt = (sVal.isEmpty ? (sTitle.isEmpty ? sDesc : sTitle) : sVal).trimmingCharacters(in: .whitespacesAndNewlines)
                        if !txt.isEmpty && txt != rowTitle {
                            hasTimeText = true
                        }
                    }
                    if let sChildren = attribute(sub, kAXChildrenAttribute) as? [AXUIElement] {
                        subQueue.append(contentsOf: sChildren)
                    }
                }
                if let link = rowLink, !rowTitle.isEmpty, rowTitle != "See all" {
                    let y = elementPosition(link)?.y ?? CGFloat(visited)
                    let isUnread = !hasStopButton && hasArchiveButton && !hasTimeText
                    rows.append(RowCandidate(link: link, title: rowTitle, y: y, isWorking: hasStopButton, isUnreadCompleted: isUnread))
                }
                continue
            }
            if let children = attribute(element, kAXChildrenAttribute) as? [AXUIElement] {
                pending.append(contentsOf: children)
            }
        }
        let filtered = rows.filter { item in
            switch filter {
            case .working: return item.isWorking
            case .completed: return item.isUnreadCompleted
            case .attention: return item.isWorking || item.isUnreadCompleted
            }
        }.sorted { $0.y < $1.y }
        guard !filtered.isEmpty else {
            return (false, "侧栏未找到\(filter.label)")
        }
        let currentIndex = lastAntigravityConversationTitle.flatMap { last in filtered.firstIndex(where: { $0.title == last }) }
        let targetIndex: Int
        if let currentIndex {
            targetIndex = next ? ((currentIndex + 1) % filtered.count) : ((currentIndex - 1 + filtered.count) % filtered.count)
        } else {
            targetIndex = next ? 0 : (filtered.count - 1)
        }
        let target = filtered[targetIndex]
        if AXUIElementPerformAction(target.link, kAXPressAction as CFString) == .success {
            lastAntigravityConversationTitle = target.title
            return (true, "已切换到\(filter.label)（\(target.title)）")
        }
        return (false, "无法点击侧栏会话（\(target.title)）")
    }

    static func perform(adapter: String, command: String, revealTools: Bool = true) async -> (Bool, String) {
        if adapter == "antigravity", let (filter, next) = AntigravitySessionFilter.from(command: command) {
            return stepAntigravitySidebarFilteredRow(filter: filter, next: next)
        }
        let antigravityOnly = ["live-voice", "prev-conversation", "next-conversation", "prev-project", "next-project", "project", "split-right", "submit"]
        guard let bundle = bundles[adapter],
              ["focus-input", "search", "new-chat", "model", "effort", "dictation", "browser", "terminal", "reveal-tools", "voice-reply", "toggle-voice-reply", "live-voice", "prev-conversation", "next-conversation", "prev-project", "next-project", "project", "split-right", "submit"].contains(command),
              !(adapter == "qwenwork" && command == "search"),
              (!antigravityOnly.contains(command) || adapter == "antigravity"),
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
        if adapter == "antigravity" {
            switch command {
            case "prev-conversation":
                return performAntigravityConversationStep(window: window, app: app, next: false)
            case "next-conversation":
                return performAntigravityConversationStep(window: window, app: app, next: true)
            case "prev-project":
                return performAntigravityProjectStep(window: window, app: app, next: false)
            case "next-project":
                return performAntigravityProjectStep(window: window, app: app, next: true)
            case "split-right":
                guard shortcutPoster(0x02, .maskCommand) else { return (false, "向右分栏快捷键事件合成失败") }
                return (true, "已触发向右分栏（Cmd+D）；未发送消息")
            default:
                break
            }
        }
        var pending = [window], matches: [AXUIElement] = [], fallbackNewChatLinks: [AXUIElement] = []
        var visited = 0
        var terminalAlreadyOpen = false
        let deadline = Date().addingTimeInterval(1.5)
        while let element = pending.popLast(), visited < 1800, Date() < deadline {
            visited += 1
            let role = attribute(element, kAXRoleAttribute) as? String ?? ""
            let title = attribute(element, kAXTitleAttribute) as? String ?? ""
            let description = attribute(element, kAXDescriptionAttribute) as? String ?? ""
            let placeholder = attribute(element, kAXPlaceholderValueAttribute) as? String ?? ""
            let help = attribute(element, kAXHelpAttribute) as? String ?? ""
            let enabled = attribute(element, kAXEnabledAttribute) as? Bool ?? true
            let labels = [title, description, placeholder, help]
            // Conversation bodies are content, not controls. Skipping their trees
            // keeps long chats from exhausting the bounded scan.
            if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
            if command == "terminal" && labels.contains("关闭终端面板") { terminalAlreadyOpen = true }
            let desktopSearch = command == "search" && ((adapter == "antigravity" && role == kAXButtonRole && labels.contains("Search Conversations"))
                || (adapter == "dsh-desktop" && role == kAXTextFieldRole && labels.contains("搜索会话名称")))
            let desktopNew = command == "new-chat" && role == kAXButtonRole &&
                ((adapter == "antigravity" && labels.contains("New Conversation")) || (adapter == "dsh-desktop" && labels.contains("新建会话")))
            if enabled && command == "new-chat" && adapter == "antigravity" && role == "AXLink" && labels.contains("New Conversation") {
                fallbackNewChatLinks.append(element)
            }
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
        if matches.isEmpty, command == "new-chat", adapter == "antigravity", fallbackNewChatLinks.count == 1 {
            matches = fallbackNewChatLinks
        }
        if adapter == "antigravity", command == "project", matches.count > 1 {
            let selectorMatches = matches.filter { candidate in
                let candidateLabels = [kAXTitleAttribute, kAXDescriptionAttribute, kAXPlaceholderValueAttribute, kAXHelpAttribute]
                    .compactMap { attribute(candidate, $0) as? String }
                return candidateLabels.contains {
                    $0.hasPrefix("Select project") || ["Select project", "Switch project", "Toggle Project Selector"].contains($0)
                }
            }
            if selectorMatches.count == 1 {
                matches = selectorMatches
            }
        }
        if matches.isEmpty, revealTools, ["browser", "terminal"].contains(command) {
            let revealed = await perform(adapter: adapter, command: "reveal-tools", revealTools: false)
            if revealed.0 {
                try? await Task.sleep(nanoseconds: 150_000_000)
                return await perform(adapter: adapter, command: command, revealTools: false)
            }
        }
        if adapter == "antigravity", matches.count != 1 {
            if let fallback = await performAntigravityShortcutFallback(command: command, app: app, root: root, textAreas: matches) {
                return fallback
            }
        }
        guard matches.count == 1, let target = matches.first else {
            return (false, matches.isEmpty ? "当前应用界面未提供此操作（\(command)）；请打开任务或检查应用能力" : "存在多个目标，请先聚焦唯一任务窗口")
        }
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else { return (false, "焦点已变化，未操作") }
        let focusesField = command == "focus-input" || (attribute(target, kAXRoleAttribute) as? String == kAXTextFieldRole)
        let targetLabels = [kAXTitleAttribute, kAXDescriptionAttribute, kAXHelpAttribute].compactMap { attribute(target, $0) as? String }
        let wasStoppingDictation = targetLabels.contains { $0.contains("停止") || $0.contains("结束") || $0.hasPrefix("Stop") || $0 == "Recording" }
        let wasActiveLiveVoice = liveVoiceState(labels: targetLabels, active: true)
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
        if error != .success {
            if adapter == "antigravity", command == "dictation", shortcutPoster(0x2E, .maskControl) {
                // Fall through to confirm dictation state after Ctrl+M fallback
            } else {
                return (false, "目标应用拒绝辅助功能操作")
            }
        }
        if command == "dictation" {
            let recordingDeadline = Date().addingTimeInterval(3)
            var sentCtrlMFallback = (error != .success)
            for step in 0..<25 where Date() < recordingDeadline {
                try? await Task.sleep(nanoseconds: 100_000_000)
                let directLabels = [kAXTitleAttribute, kAXDescriptionAttribute, kAXHelpAttribute].compactMap { attribute(target, $0) as? String }
                if dictationState(labels: directLabels, recording: !wasStoppingDictation) {
                    return (true, wasStoppingDictation ? "已确认听写停止；未发送消息" : "已确认听写启动；再次按键停止")
                }
                var queue = [window], count = 0
                while let e = queue.popLast(), count < 1800, Date() < recordingDeadline {
                    count += 1
                    let labels = [kAXTitleAttribute, kAXDescriptionAttribute, kAXHelpAttribute].compactMap { attribute(e, $0) as? String }
                    if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
                    if dictationState(labels: labels, recording: !wasStoppingDictation) { return (true, wasStoppingDictation ? "已确认听写停止；未发送消息" : "已确认听写启动；再次按键停止") }
                    if let children = attribute(e, kAXChildrenAttribute) as? [AXUIElement] { queue.append(contentsOf: children) }
                }
                if adapter == "antigravity", step == 3, !sentCtrlMFallback {
                    sentCtrlMFallback = true
                    _ = shortcutPoster(0x2E, .maskControl)
                }
            }
            return (false, "已触发听写入口，但应用未确认录音状态；请检查应用语音设置")
        }
        if command == "live-voice" {
            let liveDeadline = Date().addingTimeInterval(3)
            for _ in 0..<25 where Date() < liveDeadline {
                try? await Task.sleep(nanoseconds: 100_000_000)
                var queue = [window], count = 0
                while let e = queue.popLast(), count < 1800, Date() < liveDeadline {
                    count += 1
                    let labels = [kAXTitleAttribute, kAXDescriptionAttribute, kAXHelpAttribute].compactMap { attribute(e, $0) as? String }
                    if labels.contains(where: { ["Conversation turns", "Agent response", "User message"].contains($0) }) { continue }
                    if liveVoiceState(labels: labels, active: !wasActiveLiveVoice) {
                        return (true, wasActiveLiveVoice ? "已确认 Gemini Live 实时语音结束；未发送消息" : "已确认 Gemini Live 实时语音启动；再次按键结束")
                    }
                    if let children = attribute(e, kAXChildrenAttribute) as? [AXUIElement] { queue.append(contentsOf: children) }
                }
            }
            return (false, "已触发 Gemini Live 语音入口，但未确认状态切换；请检查麦克风或语音设置")
        }
        return (true, "原生控件已接受操作（\(command)）；未发送消息")
    }

    private static func performAntigravityShortcutFallback(
        command: String,
        app: NSRunningApplication,
        root: AXUIElement,
        textAreas: [AXUIElement]
    ) async -> (Bool, String)? {
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else {
            return (false, "焦点已变化，未操作")
        }
        switch command {
        case "focus-input":
            guard shortcutPoster(0x25, .maskCommand) else { return (false, "聚焦输入框快捷键（Cmd+L）合成失败") }
            if !textAreas.isEmpty {
                _ = await confirmFocus({
                    textAreas.contains { attribute($0, kAXFocusedAttribute) as? Bool == true }
                })
            }
            return (true, "已通过快捷键（Cmd+L）聚焦输入框；未输入或发送文本")
        case "search":
            guard shortcutPoster(0x28, .maskCommand) else { return (false, "搜索会话快捷键（Cmd+K）合成失败") }
            return (true, "已通过快捷键（Cmd+K）打开会话搜索；未发送消息")
        case "new-chat":
            guard shortcutPoster(0x2D, .maskCommand) else { return (false, "新建会话快捷键（Cmd+N）合成失败") }
            return (true, "已通过快捷键（Cmd+N）新建会话；未发送消息")
        case "model", "effort":
            guard shortcutPoster(0x2C, .maskCommand) else { return (false, "选择模型快捷键（Cmd+/）合成失败") }
            return (true, "已通过快捷键（Cmd+/）打开模型选择器；未发送消息")
        case "project":
            guard shortcutPoster(0x29, .maskCommand) else { return (false, "项目选择器快捷键（Cmd+;）合成失败") }
            return (true, "已通过快捷键（Cmd+;）打开项目选择器；未发送消息")
        case "browser":
            guard shortcutPoster(0x0B, [.maskCommand, .maskShift]) else { return (false, "内置浏览器快捷键（Cmd+Shift+B）合成失败") }
            return (true, "已通过快捷键（Cmd+Shift+B）切换内置浏览器面板；未发送消息")
        case "terminal":
            guard shortcutPoster(0x26, .maskCommand) else { return (false, "终端面板快捷键（Cmd+J）合成失败") }
            return (true, "已通过快捷键（Cmd+J）切换终端面板；未发送消息")
        case "dictation":
            guard shortcutPoster(0x2E, .maskControl) else { return (false, "语音输入快捷键（Ctrl+M）合成失败") }
            return (true, "已通过快捷键（Ctrl+M）切换语音输入；再次按键停止")
        case "submit":
            let focused = await perform(adapter: "antigravity", command: "focus-input", revealTools: false)
            guard focused.0 else { return (false, "提交前未能聚焦输入框（\(focused.1)）") }
            guard shortcutPoster(0x24, []) else { return (false, "回车发送快捷键（Return）合成失败") }
            return (true, "已通过回车（Return）发送消息")
        default:
            return nil
        }
    }

    static func extraControl(adapter: String, command: String, role: String, labels: [String]) -> Bool {
        let button = [kAXButtonRole, kAXPopUpButtonRole, kAXCheckBoxRole, "AXComboBox"].contains(role)
        guard button else { return false }
        func exact(_ names: [String]) -> Bool { labels.contains { names.contains($0) } }
        func prefix(_ value: String) -> Bool { labels.contains { $0.hasPrefix(value) } }
        if command == "model" || command == "effort" {
            // Several apps put model and reasoning effort in the same native menu.
            switch adapter {
            case "antigravity": return prefix("Select model, current:") || exact(["Select model", "Model selector"])
            case "dsh-desktop": return prefix("选择模型，当前")
            case "cindy": return prefix("选择模型。当前：")
            case "zcode": return labels.contains { $0.range(of: #"^(GLM|GPT|Claude|Gemini|DeepSeek|Qwen|Kimi|MiniMax)[- .]"#, options: .regularExpression) != nil }
            case "qwenwork": return command == "model" && (exact(["标准", "高级"]) || prefix("Qwen3.8-"))
            case "codex": return prefix("Select model") || prefix("Choose model") || prefix("选择模型") || prefix("Model selector")
            default: return false
            }
        }
        if command == "project" && adapter == "antigravity" {
            return prefix("Select project") || exact(["Select project", "Switch project", "Toggle Project Selector", "Create New Project"])
        }
        if command == "submit" && adapter == "antigravity" {
            return exact(["Send", "Send message", "Submit", "发送"])
        }
        if command == "dictation" {
            switch adapter {
            case "antigravity": return exact(["Record Audio", "Stop Recording", "Recording", "Record voice memo", "Stop recording"])
            case "qwenwork": return prefix("语音输入（") || prefix("停止录音")
            case "cindy": return exact(["开始语音输入", "停止语音输入", "结束语音输入"])
            case "zcode": return exact(["开始语音输入", "停止语音输入", "Voice Input", "Stop Voice Input"])
            case "dsh-desktop": return exact(["开始听写", "停止听写", "Voice Dictation", "Stop Dictation"])
            case "codex": return exact(["Dictate", "Stop dictation", "听写", "停止听写"])
            default: return false
            }
        }
        if command == "live-voice" && adapter == "antigravity" {
            return exact(["Start Live Chat", "Start Voice mode", "Start Live chat", "End Live chat", "End Live Chat", "End Voice mode"])
        }
        if command == "voice-reply" || command == "toggle-voice-reply" {
            switch adapter {
            case "cindy": return exact(["语音对话", "结束语音对话", "Voice Conversation", "End Voice Conversation"])
            case "zcode": return exact(["语音回复", "关闭语音回复", "Voice Reply", "Disable Voice Reply"])
            case "dsh-desktop": return exact(["语音回复", "关闭语音回复", "Voice Reply", "Disable Voice Reply"])
            default: return false
            }
        }
        if command == "reveal-tools" { return exact(["打开右侧边栏", "打开侧边栏", "显示侧栏", "展开侧边面板", "Toggle Side Panel"]) }
        if command == "browser" {
            if adapter == "dsh-desktop" && prefix("浏览器 浏览网页") { return true }
            return exact(["浏览器", "Browser", "Open browser", "打开浏览器", "Toggle Side Panel"])
        }
        if command == "terminal" {
            if adapter == "dsh-desktop" && prefix("新建终端 在会话工作区") { return true }
            return exact(["切换终端", "打开终端面板", "Terminal", "Open terminal", "Toggle terminal", "终端"])
        }
        return false
    }

    static func dictationState(labels: [String], recording: Bool) -> Bool {
        let exact = recording
            ? ["停止语音输入", "结束语音输入", "Stop Recording", "Recording", "Stop recording", "Stop dictation", "停止听写", "Stop Voice Input", "Stop Dictation"]
            : ["开始语音输入", "Record Audio", "Record voice memo", "Dictate", "听写", "开始听写", "Voice Input", "Voice Dictation"]
        return labels.contains { exact.contains($0) || (recording ? $0.hasPrefix("停止录音") : $0.hasPrefix("语音输入（")) }
    }

    static func liveVoiceState(labels: [String], active: Bool) -> Bool {
        let exact = active
            ? ["End Live chat", "End Live Chat", "End Voice mode"]
            : ["Start Live Chat", "Start Voice mode", "Start Live chat"]
        return labels.contains {
            exact.contains($0) || (active && ($0.hasPrefix("Live chat, ") || $0.hasPrefix("Voice mode, ")))
        }
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
            let help = attribute(element, kAXHelpAttribute) as? String ?? ""
            let labels = [title, description, placeholder, help]
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
        let targetPoint = window.scrollPoint(fromTop: request.fromTop, region: request.region)
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
            if request.region == .sidebar,
               adapter == "antigravity",
               ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] == nil,
               let app = NSRunningApplication(processIdentifier: pid) {
                let root = AXUIElementCreateApplication(pid)
                AXUIElementSetMessagingTimeout(root, 0.25)
                if let val = attribute(root, kAXFocusedWindowAttribute), CFGetTypeID(val) == AXUIElementGetTypeID() {
                    let axWin = unsafeBitCast(val, to: AXUIElement.self)
                    _ = performAntigravityProjectStep(window: axWin, app: app, next: lines > 0)
                } else if let wins = attribute(root, kAXWindowsAttribute) as? [AXUIElement], let axWin = wins.first {
                    _ = performAntigravityProjectStep(window: axWin, app: app, next: lines > 0)
                }
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

    enum Region: Equatable, Sendable {
        case conversation
        case sidebar
    }

    let mode: Mode
    let fromTop: Double
    let region: Region

    init(mode: Mode, fromTop: Double = Self.defaultFromTop, region: Region = .conversation) {
        self.mode = mode
        self.fromTop = max(0.15, min(0.85, fromTop))
        self.region = region
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
        case "sidebar-scroll-up":
            return DialScrollRequest(mode: .lines(dialHeld ? -fastLines : -standardLines), fromTop: 0.5, region: .sidebar)
        case "sidebar-scroll-down":
            return DialScrollRequest(mode: .lines(dialHeld ? fastLines : standardLines), fromTop: 0.5, region: .sidebar)
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
            if region == .sidebar {
                return lines < 0 ? "sidebar-scroll-up" : "sidebar-scroll-down"
            }
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
        guard region == incoming.region,
              case .lines(let current) = mode,
              case .lines(let delta) = incoming.mode else {
            return nil
        }
        let sum = current + delta
        let resolved = sum == 0 ? (delta >= 0 ? Self.standardLines : -Self.standardLines) : max(-Self.maximumLines, min(Self.maximumLines, sum))
        return DialScrollRequest(mode: .lines(resolved), fromTop: incoming.fromTop, region: region)
    }
}

struct DialScrollWindow: Equatable, Sendable {
    let windowNumber: Int
    let bounds: CGRect

    func scrollPoint(fromTop: Double = DialScrollRequest.defaultFromTop, region: DialScrollRequest.Region = .conversation) -> CGPoint {
        let ratio = max(0.15, min(0.85, fromTop))
        switch region {
        case .conversation:
            return CGPoint(x: bounds.midX, y: bounds.minY + bounds.height * ratio)
        case .sidebar:
            let sidebarX = bounds.minX + min(140, max(80, bounds.width * 0.12))
            return CGPoint(x: sidebarX, y: bounds.minY + bounds.height * ratio)
        }
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
