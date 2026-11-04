import AppKit
import ApplicationServices
import Foundation

struct HostActionReceipt: Codable, Equatable {
    let timestampMs: UInt64
    let profileId: String
    let bindingId: String
    let actionType: String
    let target: String
    let textByteLen: Int?
    let outcome: String
    let detail: String

    var succeeded: Bool {
        outcome == "executed" || outcome == "simulated"
    }

    enum CodingKeys: String, CodingKey {
        case timestampMs = "timestamp_ms"
        case profileId = "profile_id"
        case bindingId = "binding_id"
        case actionType = "action_type"
        case target
        case textByteLen = "text_byte_len"
        case outcome
        case detail
    }
}

struct ResolvedHostActionPayload: Codable, Equatable {
    let action_type: String
    let key: String?
    let key_code: UInt16?
    let modifiers: [String]?
    let target_bundle_id: String?
    let text: String?
    let bundle_id: String?
    let url: String?
    let adapter: String?
    var command: String?
    let session: String?
}

enum PassThroughHealthView: Codable, Equatable {
    case ready
    case layerDriftWarning(activeProfile: UInt32, activeLayer: UInt32, boundAgCount: UInt32)
    case snapshotMissing
    case disconnected

    private enum CodingKeys: String, CodingKey {
        case status
        case active_profile
        case active_layer
        case bound_ag_count
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let status = try c.decode(String.self, forKey: .status)
        switch status {
        case "ready":
            self = .ready
        case "layer_drift_warning":
            let p = try c.decode(UInt32.self, forKey: .active_profile)
            let l = try c.decode(UInt32.self, forKey: .active_layer)
            let count = try c.decode(UInt32.self, forKey: .bound_ag_count)
            self = .layerDriftWarning(activeProfile: p, activeLayer: l, boundAgCount: count)
        case "snapshot_missing":
            self = .snapshotMissing
        default:
            self = .disconnected
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .ready:
            try c.encode("ready", forKey: .status)
        case .layerDriftWarning(let p, let l, let count):
            try c.encode("layer_drift_warning", forKey: .status)
            try c.encode(p, forKey: .active_profile)
            try c.encode(l, forKey: .active_layer)
            try c.encode(count, forKey: .bound_ag_count)
        case .snapshotMissing:
            try c.encode("snapshot_missing", forKey: .status)
        case .disconnected:
            try c.encode("disconnected", forKey: .status)
        }
    }

    var label: String {
        switch self {
        case .ready:
            return "透传层就绪 (13/13 AG)"
        case .layerDriftWarning(let p, let l, let count):
            return "硬件层偏移警告 (P\(p)/L\(l) 仅 \(count)/13 AG)"
        case .snapshotMissing:
            return "未读取配置快照 · 透传状态待核实"
        case .disconnected:
            return "设备未连接"
        }
    }
}

@MainActor protocol HostActionExecuting: AnyObject {
    func execute(actionJSON: String, bindingId: String, profileId: String) async -> HostActionReceipt
}

enum AuditLogWriter {
    static func append(_ receipt: HostActionReceipt, pathOverride: String? = nil) {
        let resolvedPath = pathOverride ?? ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"]
        guard let path = resolvedPath, !path.isEmpty else {
            return
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        guard let data = try? encoder.encode(receipt) else { return }
        var line = data
        line.append(0x0A)
        let url = URL(fileURLWithPath: path)
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        if !FileManager.default.fileExists(atPath: path) {
            FileManager.default.createFile(atPath: path, contents: line)
            return
        }
        if let handle = try? FileHandle(forWritingTo: url) {
            defer { try? handle.close() }
            _ = try? handle.seekToEnd()
            try? handle.write(contentsOf: line)
        }
    }
}

@MainActor final class RecordingHostActionExecutor: HostActionExecuting {
    private(set) var receipts: [HostActionReceipt] = []
    private let auditLogPath: String?

    init(auditLogPath: String? = nil) {
        self.auditLogPath = auditLogPath
    }

    func execute(actionJSON: String, bindingId: String, profileId: String) async -> HostActionReceipt {
        let now = UInt64(Date().timeIntervalSince1970 * 1000)
        guard let payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(actionJSON.utf8)) else {
            let receipt = HostActionReceipt(
                timestampMs: now,
                profileId: profileId,
                bindingId: bindingId,
                actionType: "invalid",
                target: "unknown",
                textByteLen: nil,
                outcome: "blocked",
                detail: "JSON 解析失败"
            )
            receipts.append(receipt)
            AuditLogWriter.append(receipt, pathOverride: auditLogPath)
            return receipt
        }
        let meta = Self.describeMetadata(payload)
        let receipt = HostActionReceipt(
            timestampMs: now,
            profileId: profileId,
            bindingId: bindingId,
            actionType: payload.action_type,
            target: meta.target,
            textByteLen: meta.textByteLen,
            outcome: "simulated",
            detail: meta.detail
        )
        receipts.append(receipt)
        AuditLogWriter.append(receipt, pathOverride: auditLogPath)
        return receipt
    }

    static func stripTrailingNewlines(_ text: String) -> String {
        text.replacingOccurrences(of: #"[\r\n]+$"#, with: "", options: .regularExpression)
    }

    static func describeMetadata(_ payload: ResolvedHostActionPayload) -> (target: String, textByteLen: Int?, detail: String) {
        switch payload.action_type {
        case "shortcut":
            let mods = (payload.modifiers ?? []).joined(separator: "+")
            let key = payload.key ?? ""
            let detail = mods.isEmpty ? key : "\(mods)+\(key)"
            let target = payload.target_bundle_id ?? "frontmost-editor"
            return (target, nil, detail)
        case "paste_text":
            let sanitized = stripTrailingNewlines(payload.text ?? "")
            let byteLen = sanitized.utf8.count
            return ("frontmost-editor", byteLen, "no_newline=true (bytes=\(byteLen))")
        case "open_app":
            let bundleId = payload.bundle_id ?? ""
            return (bundleId, nil, bundleId)
        case "open_url":
            let url = payload.url ?? ""
            return (url, nil, url)
        case "agent_command":
            let adapter = payload.adapter ?? ""
            let cmd = payload.command ?? ""
            return (adapter, nil, "\(adapter)/\(cmd)")
        case "focus_session":
            let adapter = payload.adapter ?? ""
            let session = payload.session ?? ""
            return ("\(adapter):\(session)", nil, "\(adapter)/\(session)")
        default:
            return ("unknown", nil, payload.action_type)
        }
    }
}

@MainActor final class LiveHostActionExecutor: HostActionExecuting {
    private var savedClipboardItems: [[NSPasteboard.PasteboardType: Data]]?
    private var clipboardRestoreTask: Task<Void, Never>?

    static var isAccessibilityTrusted: Bool {
        AXIsProcessTrusted()
    }

    static func openAccessibilitySettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") {
            _ = NSWorkspace.shared.open(url)
        }
    }

    func execute(actionJSON: String, bindingId: String, profileId: String) async -> HostActionReceipt {
        let now = UInt64(Date().timeIntervalSince1970 * 1000)
        guard let payload = try? JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(actionJSON.utf8)) else {
            let receipt = HostActionReceipt(
                timestampMs: now,
                profileId: profileId,
                bindingId: bindingId,
                actionType: "invalid",
                target: "unknown",
                textByteLen: nil,
                outcome: "blocked",
                detail: "JSON 解析失败"
            )
            AuditLogWriter.append(receipt)
            return receipt
        }
        let meta = RecordingHostActionExecutor.describeMetadata(payload)
        // In synthetic QA runs, do not hijack live desktop focus, clipboard, or synthesize real CGEvents.
        if ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] == "host-execution" {
            let receipt = HostActionReceipt(
                timestampMs: now,
                profileId: profileId,
                bindingId: bindingId,
                actionType: payload.action_type,
                target: meta.target,
                textByteLen: meta.textByteLen,
                outcome: "simulated",
                detail: meta.detail
            )
            AuditLogWriter.append(receipt)
            return receipt
        }

        let (ok, detail) = await performLive(payload, defaultDetail: meta.detail)
        let receipt = HostActionReceipt(
            timestampMs: now,
            profileId: profileId,
            bindingId: bindingId,
            actionType: payload.action_type,
            target: meta.target,
            textByteLen: meta.textByteLen,
            outcome: ok ? "executed" : "blocked",
            detail: detail
        )
        AuditLogWriter.append(receipt)
        return receipt
    }

    private func performLive(_ payload: ResolvedHostActionPayload, defaultDetail: String) async -> (Bool, String) {
        switch payload.action_type {
        case "shortcut":
            guard let keyCode = payload.key_code else { return (false, "缺失虚拟键码") }
            if let targetBundle = payload.target_bundle_id, !targetBundle.isEmpty {
                let activated = await activateAppAndWait(bundleId: targetBundle)
                if !activated { return (false, "目标应用 \(targetBundle) 未能激活到前台") }
            }
            let ok = postShortcut(keyCode: CGKeyCode(keyCode), modifiers: payload.modifiers ?? [])
            return (ok, ok ? defaultDetail : "辅助功能未授权或事件合成失败")

        case "paste_text":
            guard let rawText = payload.text else { return (false, "缺失文本内容") }
            let sanitized = RecordingHostActionExecutor.stripTrailingNewlines(rawText)
            let ok = await pasteTextPreservingClipboard(sanitized)
            return (ok, ok ? "no_newline=true (bytes=\(sanitized.utf8.count))" : "辅助功能未授权或粘贴失败")

        case "open_app":
            guard let bundleId = payload.bundle_id else { return (false, "缺失应用标识") }
            let ok = await activateAppAndWait(bundleId: bundleId)
            return (ok, ok ? bundleId : "目标应用 \(bundleId) 未安装或未能激活")

        case "open_url":
            guard let raw = payload.url, let url = URL(string: raw) else { return (false, "无效 URL") }
            let ok = NSWorkspace.shared.open(url)
            return (ok, raw)

        case "agent_command":
            let adapter = payload.adapter ?? ""
            let cmd = payload.command ?? ""
            if let bundle = DesktopAgentControls.bundles[adapter] {
                guard await activateAppAndWait(bundleId: bundle) else { return (false, "目标应用无法激活") }
                return await DesktopAgentControls.perform(adapter: adapter, command: cmd)
            }
            if adapter == "herdr" {
                return await HerdrAdapter.shared.executeCommand(cmd)
            } else if adapter == "dsh" {
                return (false, "旧 DSH CLI 操作未接入；请使用桌面 Deepseek Harness 配置")
            }
            return (false, "未知适配器 \(adapter)")

        case "focus_session":
            let adapter = payload.adapter ?? ""
            let session = payload.session ?? ""
            if adapter == "herdr" {
                return await HerdrAdapter.shared.focusSession(session)
            } else if adapter == "dsh" {
                return (false, "旧 DSH 会话占位符不可执行；请使用桌面 Deepseek Harness 配置")
            }
            return (false, "未知适配器 \(adapter)")

        default:
            return (false, "不支持的动作类型 \(payload.action_type)")
        }
    }

    private func activateAppAndWait(bundleId: String) async -> Bool {
        if NSWorkspace.shared.frontmostApplication?.bundleIdentifier == bundleId {
            return true
        }
        guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleId) else {
            return false
        }
        let config = NSWorkspace.OpenConfiguration()
        config.activates = true
        _ = try? await NSWorkspace.shared.openApplication(at: url, configuration: config)
        for _ in 0..<20 {
            if NSWorkspace.shared.frontmostApplication?.bundleIdentifier == bundleId {
                return true
            }
            try? await Task.sleep(nanoseconds: 15_000_000)
        }
        return NSWorkspace.shared.frontmostApplication?.bundleIdentifier == bundleId
    }

    private func postShortcut(keyCode: CGKeyCode, modifiers: [String]) -> Bool {
        guard AXIsProcessTrusted() else {
            let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
            _ = AXIsProcessTrustedWithOptions(options)
            return false
        }
        guard let source = CGEventSource(stateID: .hidSystemState),
              let down = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: false) else {
            return false
        }
        var flags: CGEventFlags = []
        for mod in modifiers {
            switch mod.lowercased() {
            case "command", "cmd": flags.insert(.maskCommand)
            case "shift": flags.insert(.maskShift)
            case "option", "opt": flags.insert(.maskAlternate)
            case "control", "ctrl": flags.insert(.maskControl)
            default: break
            }
        }
        down.flags = flags
        up.flags = flags
        down.post(tap: .cghidEventTap)
        up.post(tap: .cghidEventTap)
        return true
    }

    private func pasteTextPreservingClipboard(_ sanitizedText: String) async -> Bool {
        guard AXIsProcessTrusted() else {
            let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
            _ = AXIsProcessTrustedWithOptions(options)
            return false
        }
        let pb = NSPasteboard.general
        if savedClipboardItems == nil {
            var snapshot: [[NSPasteboard.PasteboardType: Data]] = []
            var totalBytes = 0
            for item in pb.pasteboardItems ?? [] {
                var itemMap: [NSPasteboard.PasteboardType: Data] = [:]
                for type in item.types {
                    if let data = item.data(forType: type), totalBytes + data.count <= 4 * 1024 * 1024 {
                        itemMap[type] = data
                        totalBytes += data.count
                    }
                }
                if !itemMap.isEmpty { snapshot.append(itemMap) }
            }
            savedClipboardItems = snapshot
        }

        pb.clearContents()
        pb.setString(sanitizedText, forType: .string)
        let expectedChangeCount = pb.changeCount
        let posted = postShortcut(keyCode: 0x09, modifiers: ["command"]) // Cmd+V

        clipboardRestoreTask?.cancel()
        clipboardRestoreTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 150_000_000)
            guard !Task.isCancelled, let self, let saved = self.savedClipboardItems else { return }
            self.savedClipboardItems = nil
            // Only restore if no external app/user modified the clipboard during the 150ms window.
            guard pb.changeCount == expectedChangeCount else { return }
            pb.clearContents()
            let restoredItems = saved.map { dict -> NSPasteboardItem in
                let item = NSPasteboardItem()
                for (type, data) in dict { item.setData(data, forType: type) }
                return item
            }
            if !restoredItems.isEmpty { pb.writeObjects(restoredItems) }
        }
        return posted
    }
}
