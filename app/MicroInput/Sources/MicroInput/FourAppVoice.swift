import Foundation
import SwiftUI

/// Mode of voice reply for an agent application.
enum FourAppVoiceReplyMode: Equatable, Sendable {
    case supported(enabled: Bool)
    case unsupported(app: String, reason: String)

    var isSupported: Bool {
        if case .supported = self { return true }
        return false
    }

    var isEnabled: Bool {
        if case .supported(let enabled) = self { return enabled }
        return false
    }
}

/// Phase of voice dictation for a specific target.
enum FourAppDictationState: Equatable, Sendable {
    case idle
    case recording(app: String, session: KeypadNavigationPlace, startedAt: Date)
    case stopped(app: String, session: KeypadNavigationPlace)

    var isRecording: Bool {
        if case .recording = self { return true }
        return false
    }

    var activeApp: String? {
        switch self {
        case .idle: return nil
        case .recording(let app, _, _), .stopped(let app, _): return app
        }
    }
}

extension KeypadNavigationPlace {
    var targetApp: String {
        switch self {
        case .antigravity: return "antigravity"
        case .cindy: return "cindy"
        case .zcode: return "zcode"
        case .dsh: return "dsh-desktop"
        case .app(let bundle): return bundle
        case .session(let adapter, _): return adapter
        case .t3Thread: return "t3"
        }
    }

    var displayLabel: String {
        switch self {
        case .antigravity(let conv, let ws): return "\(conv)@\(ws)"
        case .cindy(let ws, let sess): return "\(sess)@\(ws)"
        case .zcode(_, _, let ws, let tid): return "\(tid)@\(ws)"
        case .dsh(let sess, let ws, _): return "\(sess)@\(ws)"
        case .app(let bundle): return bundle
        case .session(let adapter, let sess): return "\(adapter):\(sess)"
        case .t3Thread(let env, let thread): return "\(env)#\(thread)"
        }
    }
}

/// Unified manager coordinating hold-to-dictate, release-to-stop, submit-before-stop,
/// and independent voice reply controls across Antigravity, Cindy, ZCode, and DSH.
@MainActor
final class FourAppVoiceManager: ObservableObject {
    static let supportedApps: Set<String> = ["cindy", "antigravity", "zcode", "dsh-desktop"]

    @Published private(set) var dictationState: FourAppDictationState = .idle
    @Published private(set) var voiceReplyModes: [String: FourAppVoiceReplyMode] = [:]

    init() {
        self.voiceReplyModes = [
            "cindy": .supported(enabled: false),
            "zcode": .supported(enabled: false),
            "dsh-desktop": .supported(enabled: false),
            "antigravity": .unsupported(
                app: "antigravity",
                reason: "Antigravity 2.29.0 language_server does not expose speech generation or voice reply mode"
            )
        ]
    }

    /// Start hold-to-dictate bound strictly to target app and session.
    func startDictation(
        app: String,
        session: KeypadNavigationPlace,
        binding: String,
        profile: String,
        store: Store
    ) async -> (Bool, String) {
        guard Self.supportedApps.contains(app) else {
            return (false, "未接入的应用听写目标: \(app)")
        }

        // Verify target binding matches session
        guard session.targetApp == app else {
            return (false, "听写目标不匹配: 期望 \(app)，实际 \(session.targetApp)")
        }

        // Cindy native voice reuse
        let isCindyNative: Bool = {
            if case .native = store.nativeOwnership.state { return true }
            return false
        }()
        if app == "cindy" && isCindyNative {
            dictationState = .recording(app: app, session: session, startedAt: Date())
            return (true, "Cindy 复用原生语音；按住听写已启动")
        }

        // Check if already recording
        if case .recording(let currentApp, let currentSession, _) = dictationState {
            return (false, "听写已在进行中: \(currentApp) [\(currentSession.displayLabel)]")
        }

        // Dispatch hold dictation via Micro Input PushToTalk
        store.startHeldDictation(adapter: app, binding: binding, profile: profile)
        dictationState = .recording(app: app, session: session, startedAt: Date())
        return (true, "已确认听写启动；松开停止，不自动发送")
    }

    /// Stop dictation on key release.
    /// Dictation transcription and sending remain strictly separated.
    func stopDictation(
        app: String,
        session: KeypadNavigationPlace,
        binding: String,
        store: Store
    ) async -> (Bool, String) {
        guard case .recording(let recApp, let recSession, _) = dictationState else {
            return (false, "没有正在录音的听写会话")
        }
        guard recApp == app, recSession == session else {
            return (false, "停止听写目标不匹配: 期望 \(recApp), 实际 \(app)")
        }

        // Cindy native voice release
        let isCindyNative: Bool = {
            if case .native = store.nativeOwnership.state { return true }
            return false
        }()
        if app == "cindy" && isCindyNative {
            dictationState = .stopped(app: app, session: session)
            return (true, "已确认 Cindy 原生听写停止；未发送消息")
        }

        // Release PushToTalk binding
        store.pushToTalk.release(binding: binding)
        await store.pushToTalk.waitUntilStopped()
        let stopped = await store.pushToTalk.verifyStopped()
        dictationState = .stopped(app: app, session: session)
        return (stopped, stopped ? "已确认原听写停止；未发送消息" : "原听写停止未确认；未发送消息")
    }

    /// Enforce stopping active dictation before any submit operation.
    func stopBeforeSubmit(app: String, store: Store) async -> (Bool, String) {
        if case .recording(let recApp, let session, _) = dictationState {
            if recApp == app {
                store.pushToTalk.cancel()
                await store.pushToTalk.waitUntilStopped()
                dictationState = .stopped(app: app, session: session)
                let stopped = await DesktopAgentControls.stopActiveDictationIfNeeded(adapter: app)
                return (stopped.0, "提交前已先停止活跃听写")
            }
        }
        let stopped = await DesktopAgentControls.stopActiveDictationIfNeeded(adapter: app)
        return stopped
    }

    /// Read current voice reply mode from authoritative source.
    func getVoiceReplyMode(app: String, store: Store) -> FourAppVoiceReplyMode {
        switch app {
        case "cindy":
            let mode = FourAppVoiceReplyMode.supported(enabled: store.cindySessions.voiceReplyEnabled)
            voiceReplyModes[app] = mode
            return mode
        case "zcode":
            let mode = FourAppVoiceReplyMode.supported(enabled: store.zcodeSessions.voiceReplyEnabled)
            voiceReplyModes[app] = mode
            return mode
        case "dsh-desktop":
            let mode = FourAppVoiceReplyMode.supported(enabled: store.dshSessions.voiceReplyEnabled)
            voiceReplyModes[app] = mode
            return mode
        case "antigravity":
            let mode = FourAppVoiceReplyMode.unsupported(
                app: "antigravity",
                reason: "Antigravity 2.29.0 language_server does not expose speech generation or voice reply mode"
            )
            voiceReplyModes[app] = mode
            return mode
        default:
            return .unsupported(app: app, reason: "未接入的应用: \(app)")
        }
    }

    /// Set voice reply mode and read back to verify output.
    /// Crucially: unsupported models/interfaces remain incomplete and are never masked
    /// as "disabled" or "success".
    func setVoiceReply(app: String, enabled: Bool, store: Store) async -> (Bool, FourAppVoiceReplyMode, String) {
        switch app {
        case "cindy":
            let (_, detail) = await store.cindySessions.setVoiceReply(enabled: enabled)
            let readback = store.cindySessions.voiceReplyEnabled
            let mode = FourAppVoiceReplyMode.supported(enabled: readback)
            voiceReplyModes[app] = mode
            let verified = (readback == enabled)
            return (verified, mode, verified ? detail : "Cindy 语音回复回读不一致")
        case "zcode":
            let (_, detail) = await store.zcodeSessions.setVoiceReply(enabled: enabled)
            let readback = store.zcodeSessions.voiceReplyEnabled
            let mode = FourAppVoiceReplyMode.supported(enabled: readback)
            voiceReplyModes[app] = mode
            let verified = (readback == enabled)
            return (verified, mode, verified ? detail : "ZCode 语音回复回读不一致")
        case "dsh-desktop":
            let (_, detail) = await store.dshSessions.setVoiceReply(enabled: enabled)
            let readback = store.dshSessions.voiceReplyEnabled
            let mode = FourAppVoiceReplyMode.supported(enabled: readback)
            voiceReplyModes[app] = mode
            let verified = (readback == enabled)
            return (verified, mode, verified ? detail : "DSH 语音回复回读不一致")
        case "antigravity":
            let (_, _) = await store.antigravityStatus.setVoiceReply(enabled: enabled)
            let mode = FourAppVoiceReplyMode.unsupported(
                app: "antigravity",
                reason: "Antigravity 2.29.0 language_server does not expose speech generation or voice reply mode"
            )
            voiceReplyModes[app] = mode
            // Return false to keep uncompleted without masking as disabled or success
            return (false, mode, "Antigravity 2.29.0 模型/接口不支持语音回复，保持未完成缺口")
        default:
            let mode = FourAppVoiceReplyMode.unsupported(app: app, reason: "未知应用: \(app)")
            return (false, mode, "未接入的应用操作")
        }
    }

    /// Toggle voice reply mode and read back to verify output.
    func toggleVoiceReply(app: String, store: Store) async -> (Bool, FourAppVoiceReplyMode, String) {
        let current = getVoiceReplyMode(app: app, store: store)
        switch current {
        case .supported(let enabled):
            return await setVoiceReply(app: app, enabled: !enabled, store: store)
        case .unsupported:
            return (false, current, "目标应用模型/接口不支持语音回复，保持未完成缺口")
        }
    }
}
