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
        ["executed", "simulated", "delivered"].contains(outcome)
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
    let working_directory: String?
    let adapter: String?
    var command: String?
    let session: String?
    let events: [ResolvedMacroEventPayload]?
    let no_newline: Bool?
    let submit: Bool?

    init(
        action_type: String,
        key: String? = nil,
        key_code: UInt16? = nil,
        modifiers: [String]? = nil,
        target_bundle_id: String? = nil,
        text: String? = nil,
        bundle_id: String? = nil,
        url: String? = nil,
        working_directory: String? = nil,
        adapter: String? = nil,
        command: String? = nil,
        session: String? = nil,
        events: [ResolvedMacroEventPayload]? = nil,
        no_newline: Bool? = nil,
        submit: Bool? = nil
    ) {
        self.action_type = action_type
        self.key = key
        self.key_code = key_code
        self.modifiers = modifiers
        self.target_bundle_id = target_bundle_id
        self.text = text
        self.bundle_id = bundle_id
        self.url = url
        self.working_directory = working_directory
        self.adapter = adapter
        self.command = command
        self.session = session
        self.events = events
        self.no_newline = no_newline
        self.submit = submit
    }
}

@MainActor protocol PasteboardAccessing: AnyObject {
    var changeCount: Int { get }
    func snapshotItems(maxBytes: Int) -> [[NSPasteboard.PasteboardType: Data]]
    func writePlainText(_ text: String)
    func restoreItems(_ items: [[NSPasteboard.PasteboardType: Data]])
}

@MainActor final class SystemPasteboard: PasteboardAccessing {
    private let pb: NSPasteboard
    init(_ pb: NSPasteboard = .general) {
        self.pb = pb
    }
    var changeCount: Int { pb.changeCount }
    func snapshotItems(maxBytes: Int = 4 * 1024 * 1024) -> [[NSPasteboard.PasteboardType: Data]] {
        var snapshot: [[NSPasteboard.PasteboardType: Data]] = []
        var totalBytes = 0
        for item in pb.pasteboardItems ?? [] {
            var itemMap: [NSPasteboard.PasteboardType: Data] = [:]
            for type in item.types {
                if let data = item.data(forType: type), totalBytes + data.count <= maxBytes {
                    itemMap[type] = data
                    totalBytes += data.count
                }
            }
            if !itemMap.isEmpty { snapshot.append(itemMap) }
        }
        return snapshot
    }
    func writePlainText(_ text: String) {
        pb.clearContents()
        pb.setString(text, forType: .string)
    }
    func restoreItems(_ items: [[NSPasteboard.PasteboardType: Data]]) {
        pb.clearContents()
        let restoredItems = items.map { dict -> NSPasteboardItem in
            let item = NSPasteboardItem()
            for (type, data) in dict { item.setData(data, forType: type) }
            return item
        }
        if !restoredItems.isEmpty { pb.writeObjects(restoredItems) }
    }
}

struct ResolvedMacroEventPayload: Codable, Equatable {
    let event_type: String
    let key: String?
    let key_code: UInt16?
    let ms: UInt32?
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
            let target = (payload.target_bundle_id.flatMap { $0.isEmpty ? nil : $0 })
                ?? (payload.adapter.flatMap { DesktopAgentControls.bundles[$0] })
                ?? "frontmost-editor"
            let submit = payload.submit ?? false
            let detail = submit
                ? "no_newline=true auto_submit=true (bytes=\(byteLen))"
                : "no_newline=true (bytes=\(byteLen))"
            return (target, byteLen, detail)
        case "open_app":
            let bundleId = payload.bundle_id ?? ""
            return (bundleId, nil, bundleId)
        case "open_url":
            let url = payload.url ?? ""
            return (url, nil, url)
        case "shell_command":
            let workingDirectory = payload.working_directory ?? ""
            let commandBytes = (payload.command ?? "").utf8.count
            return (workingDirectory, commandBytes, "cwd_bytes=\(workingDirectory.utf8.count) command_bytes=\(commandBytes)")
        case "agent_command":
            let adapter = payload.adapter ?? ""
            let cmd = payload.command ?? ""
            if cmd.hasPrefix("prompt:") {
                let rawPrompt = String(cmd.dropFirst("prompt:".count))
                let sanitized = stripTrailingNewlines(rawPrompt)
                let byteLen = sanitized.utf8.count
                let target = DesktopAgentControls.bundles[adapter] ?? adapter
                return (target, byteLen, "\(adapter)/prompt no_newline=true auto_submit=false (bytes=\(byteLen))")
            }
            if cmd.hasPrefix("submit-prompt:") {
                let rawPrompt = String(cmd.dropFirst("submit-prompt:".count))
                let sanitized = stripTrailingNewlines(rawPrompt)
                let byteLen = sanitized.utf8.count
                let target = DesktopAgentControls.bundles[adapter] ?? adapter
                return (target, byteLen, "\(adapter)/submit-prompt no_newline=true auto_submit=true (bytes=\(byteLen))")
            }
            return (adapter, nil, "\(adapter)/\(cmd)")
        case "focus_session":
            let adapter = payload.adapter ?? ""
            let session = payload.session ?? ""
            return ("\(adapter):\(session)", nil, "\(adapter)/\(session)")
        case "macro_sequence":
            let events = payload.events ?? []
            let delays = events.filter { $0.event_type == "delay" }.count
            let keys = events.filter { $0.event_type == "press" || $0.event_type == "release" }.count
            return ("frontmost-editor", nil, "steps=\(events.count) keys=\(keys) delays=\(delays)")
        default:
            return ("unknown", nil, payload.action_type)
        }
    }
}

@MainActor final class LiveHostActionExecutor: HostActionExecuting {
    private var savedClipboardItems: [[NSPasteboard.PasteboardType: Data]]?
    private var clipboardRestoreTask: Task<Void, Never>?
    private let auditLogPath: String?
    var pasteboard: PasteboardAccessing = SystemPasteboard()
    var clipboardRestoreDelayNanoseconds: UInt64 = 150_000_000
    var submitSettleDelayNanoseconds: UInt64 = 120_000_000
    var macroAccessibilityChecker: () -> Bool = { AXIsProcessTrusted() }
    var appActivator: ((String) async -> Bool)?
    var agentInputFocuser: (String) async -> (Bool, String) = { adapter in
        await DesktopAgentControls.perform(adapter: adapter, command: "focus-input")
    }
    var activeDictationStopper: (String) async -> (Bool, String) = { adapter in
        await DesktopAgentControls.stopActiveDictationIfNeeded(adapter: adapter)
    }
    var pasteShortcutPoster: (() -> Bool)?
    var returnKeyPoster: (() -> Bool)?
    var macroKeyEventPoster: (CGKeyCode, Bool) -> Bool = { keyCode, down in
        guard let source = CGEventSource(stateID: .hidSystemState),
              let event = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: down) else {
            return false
        }
        event.post(tap: .cghidEventTap)
        return true
    }
    var macroSleeper: (UInt64) async throws -> Void = { nanoseconds in
        try await Task.sleep(nanoseconds: nanoseconds)
    }

    static var isAccessibilityTrusted: Bool {
        AXIsProcessTrusted()
    }

    static func openAccessibilitySettings() {
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        _ = AXIsProcessTrustedWithOptions(options)
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") {
            _ = NSWorkspace.shared.open(url)
        }
    }

    static func resetAndOpenAccessibilitySettings() {
        let lsreg = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
        if FileManager.default.isExecutableFile(atPath: lsreg),
           Bundle.main.bundlePath == "/Applications/Micro Input.app" {
            let clean = Process()
            clean.executableURL = URL(fileURLWithPath: lsreg)
            clean.arguments = ["-f", Bundle.main.bundlePath]
            try? clean.run()
            clean.waitUntilExit()
        }
        if let bundleId = Bundle.main.bundleIdentifier, !bundleId.isEmpty {
            let reset = Process()
            reset.executableURL = URL(fileURLWithPath: "/usr/bin/tccutil")
            reset.arguments = ["reset", "Accessibility", bundleId]
            try? reset.run()
            reset.waitUntilExit()
        }
        openAccessibilitySettings()
    }

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
            AuditLogWriter.append(receipt, pathOverride: auditLogPath)
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
            AuditLogWriter.append(receipt, pathOverride: auditLogPath)
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
        AuditLogWriter.append(receipt, pathOverride: auditLogPath)
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
            return await executePromptDispatch(
                rawText: rawText,
                adapter: payload.adapter,
                targetBundleId: payload.target_bundle_id,
                submit: payload.submit ?? false
            )

        case "open_app":
            guard let bundleId = payload.bundle_id else { return (false, "缺失应用标识") }
            let ok = await activateTargetApp(bundleId: bundleId)
            return (ok, ok ? bundleId : "目标应用 \(bundleId) 未安装或未能激活")

        case "open_url":
            guard let raw = payload.url, let url = URL(string: raw) else { return (false, "无效 URL") }
            let ok = NSWorkspace.shared.open(url)
            return (ok, raw)

        case "shell_command":
            guard let command = payload.command, !command.isEmpty,
                  let workingDirectory = payload.working_directory, !workingDirectory.isEmpty else {
                return (false, "缺失命令或绝对工作目录")
            }
            let (ok, _) = await AgentAdapters.runProcess(
                executable: "/bin/sh",
                arguments: ["-c", command],
                workingDirectory: workingDirectory,
                timeoutSeconds: 10.0
            )
            let commandBytes = command.utf8.count
            return (ok, ok ? "exit=0 cwd_bytes=\(workingDirectory.utf8.count) command_bytes=\(commandBytes)" : "命令执行失败；输出不进入审计")

        case "agent_command":
            let adapter = payload.adapter ?? ""
            let cmd = payload.command ?? ""
            if cmd == "submit" {
                return await executeExplicitSubmit(adapter: adapter)
            }
            if cmd.hasPrefix("prompt:") {
                let text = String(cmd.dropFirst("prompt:".count))
                return await executePromptDispatch(
                    rawText: text,
                    adapter: adapter,
                    targetBundleId: payload.target_bundle_id,
                    submit: false
                )
            }
            if cmd.hasPrefix("submit-prompt:") {
                let text = String(cmd.dropFirst("submit-prompt:".count))
                return await executePromptDispatch(
                    rawText: text,
                    adapter: adapter,
                    targetBundleId: payload.target_bundle_id,
                    submit: true
                )
            }
            if let bundle = DesktopAgentControls.bundles[adapter] {
                if DialScrollRequest.from(command: cmd) != nil {
                    return await DesktopAgentControls.performScroll(adapter: adapter, command: cmd)
                }
                guard await activateTargetApp(bundleId: bundle) else { return (false, "目标应用无法激活") }
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

        case "macro_sequence":
            return await performMacroSequence(payload.events ?? [])

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

    private func performMacroSequence(_ events: [ResolvedMacroEventPayload]) async -> (Bool, String) {
        guard !events.isEmpty else { return (false, "宏序列为空") }
        guard macroAccessibilityChecker() else {
            let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
            _ = AXIsProcessTrustedWithOptions(options)
            return (false, "辅助功能未授权")
        }

        var heldKeyCodes: Set<UInt16> = []
        do {
            for event in events {
                switch event.event_type {
                case "press":
                    guard let keyCode = event.key_code, !heldKeyCodes.contains(keyCode),
                    macroKeyEventPoster(keyCode, true) else {
                        releaseHeldKeys(heldKeyCodes)
                        return (false, "宏第 \(event.key_code == nil ? 0 : 1) 个键事件合成失败")
                    }
                    heldKeyCodes.insert(keyCode)
                case "release":
                    guard let keyCode = event.key_code, heldKeyCodes.contains(keyCode),
                    macroKeyEventPoster(keyCode, false) else {
                        releaseHeldKeys(heldKeyCodes)
                        return (false, "宏释放事件无效或合成失败")
                    }
                    heldKeyCodes.remove(keyCode)
                case "delay":
                    guard let milliseconds = event.ms, milliseconds > 0 else {
                        releaseHeldKeys(heldKeyCodes)
                        return (false, "宏延时无效")
                    }
                    try await macroSleeper(UInt64(milliseconds) * 1_000_000)
                default:
                    releaseHeldKeys(heldKeyCodes)
                    return (false, "宏包含不支持的事件")
                }
            }
            return (true, "宏序列完成")
        } catch is CancellationError {
            releaseHeldKeys(heldKeyCodes)
            return (false, "宏已取消，按下的键已释放")
        } catch {
            releaseHeldKeys(heldKeyCodes)
            return (false, "宏执行中断，按下的键已释放")
        }
    }

    private func releaseHeldKeys(_ keyCodes: Set<UInt16>) {
        for keyCode in keyCodes {
            _ = macroKeyEventPoster(keyCode, false)
        }
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

    private func activateTargetApp(bundleId: String) async -> Bool {
        if let appActivator {
            return await appActivator(bundleId)
        }
        return await activateAppAndWait(bundleId: bundleId)
    }

    private func resolveTargetDesktopAgent(adapter: String?, targetBundleId: String?) -> (adapter: String, bundleID: String)? {
        if let adapter, let bundleID = DesktopAgentControls.bundles[adapter] {
            return (adapter, bundleID)
        }
        if let targetBundleId,
           let match = DesktopAgentControls.bundles.first(where: { $0.value == targetBundleId }) {
            return (match.key, match.value)
        }
        return nil
    }

    private func executePromptDispatch(
        rawText: String,
        adapter: String?,
        targetBundleId: String?,
        submit: Bool
    ) async -> (Bool, String) {
        let sanitized = RecordingHostActionExecutor.stripTrailingNewlines(rawText)
        guard !sanitized.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return (false, "粘贴文本为空")
        }
        guard macroAccessibilityChecker() else {
            return (false, "辅助功能未授权")
        }
        let targetAgent = resolveTargetDesktopAgent(adapter: adapter, targetBundleId: targetBundleId)
        var dictationDetail: String?
        if let targetAgent {
            guard await activateTargetApp(bundleId: targetAgent.bundleID) else {
                return (false, "目标应用 \(targetAgent.bundleID) 未能激活到前台")
            }
            if submit {
                let dictationResult = await activeDictationStopper(targetAgent.adapter)
                guard dictationResult.0 else {
                    return (false, "活跃听写未能确认停止，已阻断提交（\(dictationResult.1)）")
                }
                dictationDetail = dictationResult.1
                if dictationResult.1.hasPrefix("dictation_stopped:") {
                    try? await macroSleeper(submitSettleDelayNanoseconds)
                }
            }
            let focusResult = await agentInputFocuser(targetAgent.adapter)
            guard focusResult.0 else {
                return (false, "目标输入区聚焦失败，未粘贴（\(focusResult.1)）")
            }
        }
        let pasted = await pasteTextPreservingClipboard(sanitized)
        guard pasted else {
            return (false, "辅助功能未授权或粘贴失败")
        }
        if !submit {
            return (true, "no_newline=true (bytes=\(sanitized.utf8.count)) auto_submit=false")
        }
        try? await macroSleeper(submitSettleDelayNanoseconds)
        let returned = returnKeyPoster?() ?? postShortcut(keyCode: 0x24, modifiers: [])
        guard returned else {
            return (false, "已粘贴文本，但回车提交事件合成失败")
        }
        let suffix = dictationDetail.map { " dictation=\($0)" } ?? ""
        return (true, "no_newline=true (bytes=\(sanitized.utf8.count)) auto_submit=true\(suffix)")
    }

    private func executeExplicitSubmit(adapter: String) async -> (Bool, String) {
        guard macroAccessibilityChecker() else {
            return (false, "辅助功能未授权")
        }
        guard let bundleID = DesktopAgentControls.bundles[adapter] else {
            return (false, "未知桌面 Agent 适配器 \(adapter)")
        }
        guard await activateTargetApp(bundleId: bundleID) else {
            return (false, "目标应用 \(bundleID) 未能激活到前台")
        }
        let dictationResult = await activeDictationStopper(adapter)
        guard dictationResult.0 else {
            return (false, "活跃听写未能确认停止，已阻断提交（\(dictationResult.1)）")
        }
        if dictationResult.1.hasPrefix("dictation_stopped:") {
            try? await macroSleeper(submitSettleDelayNanoseconds)
        }
        let focusResult = await agentInputFocuser(adapter)
        guard focusResult.0 else {
            return (false, "目标输入区聚焦失败，未提交（\(focusResult.1)）")
        }
        let returned = returnKeyPoster?() ?? postShortcut(keyCode: 0x24, modifiers: [])
        guard returned else {
            return (false, "回车提交事件合成失败")
        }
        return (true, "\(adapter)/submit (\(dictationResult.1))")
    }

    func flushClipboardRestoreForTesting() async {
        await clipboardRestoreTask?.value
    }

    private func pasteTextPreservingClipboard(_ sanitizedText: String) async -> Bool {
        guard macroAccessibilityChecker() else {
            let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
            _ = AXIsProcessTrustedWithOptions(options)
            return false
        }
        if savedClipboardItems == nil {
            savedClipboardItems = pasteboard.snapshotItems(maxBytes: 4 * 1024 * 1024)
        }

        pasteboard.writePlainText(sanitizedText)
        let expectedChangeCount = pasteboard.changeCount
        let posted = pasteShortcutPoster?() ?? postShortcut(keyCode: 0x09, modifiers: ["command"]) // Cmd+V
        if !posted {
            clipboardRestoreTask?.cancel()
            clipboardRestoreTask = nil
            if let saved = savedClipboardItems {
                savedClipboardItems = nil
                pasteboard.restoreItems(saved)
            }
            return false
        }

        let delay = clipboardRestoreDelayNanoseconds
        clipboardRestoreTask?.cancel()
        clipboardRestoreTask = Task { @MainActor [weak self] in
            if delay > 0 {
                try? await self?.macroSleeper(delay)
            }
            guard !Task.isCancelled, let self, let saved = self.savedClipboardItems else { return }
            self.savedClipboardItems = nil
            // Only restore if no external app/user modified the clipboard during the restore window.
            guard self.pasteboard.changeCount == expectedChangeCount else { return }
            self.pasteboard.restoreItems(saved)
        }
        return true
    }
}
