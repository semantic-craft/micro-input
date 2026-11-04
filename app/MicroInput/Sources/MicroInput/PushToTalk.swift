import Foundation

@MainActor protocol DictationRecording: AnyObject {
    func stop() async -> (Bool, String)
    func isStopped() async -> Bool
}

enum DictationStart {
    case started(any DictationRecording)
    case uncertain(any DictationRecording, String)
    case blocked(String)
}

@MainActor protocol DictationDriving {
    var simulated: Bool { get }
    func start(adapter: String, isHeld: @escaping () -> Bool) async -> DictationStart
}

extension DictationDriving { var simulated: Bool { false } }

@MainActor final class PushToTalkController {
    private final class Gesture {
        let binding: String
        let adapter: String
        let ingress: UInt64
        var released = false
        var reason = "松开按键"
        var release: CheckedContinuation<Void, Never>?
        init(binding: String, adapter: String, ingress: UInt64) { self.binding = binding; self.adapter = adapter; self.ingress = ingress }
        func end(_ reason: String = "松开按键") {
            if !released { self.reason = reason }
            released = true; release?.resume(); release = nil
        }
    }
    private let driver: any DictationDriving
    private let maximumHold: UInt64
    private let operationTimeout: UInt64
    private var pendingStart = false
    private var gesture: Gesture?
    private var task: Task<Void, Never>?
    private var unresolved: (any DictationRecording)?
    var cleanupConfirmed: Bool { unresolved == nil && !pendingStart }
    var isActive: Bool { task != nil }
    var simulated: Bool { driver.simulated }
    var targetBundle: String? { gesture.flatMap { DesktopAgentControls.bundles[$0.adapter] } }

    init(driver: any DictationDriving, maximumHold: UInt64 = 120_000_000_000, operationTimeout: UInt64 = 8_000_000_000) {
        self.driver = driver; self.maximumHold = maximumHold; self.operationTimeout = operationTimeout
    }
    @discardableResult
    func press(binding: String, adapter: String, ingress: UInt64 = 0, report: @escaping (String, Bool, String) -> Void, finished: @escaping () -> Void) -> Bool {
        guard task == nil, cleanupConfirmed else { return false }
        let held = Gesture(binding: binding, adapter: adapter, ingress: ingress)
        gesture = held
        task = Task { @MainActor [self] in
            let expiry = Task { @MainActor in
                do { try await Task.sleep(nanoseconds: maximumHold); held.end("达到最长按住时限") } catch { }
            }
            defer { expiry.cancel(); gesture = nil; task = nil; finished() }
            let recording: any DictationRecording
            let result = await bounded(timeout: operationTimeout, operation: {
                await self.driver.start(adapter: adapter, isHeld: { !held.released })
            }, timedOut: {
                self.pendingStart = true; held.end("听写启动超时")
            }, late: { result in
                switch result {
                case .started(let session), .uncertain(let session, _):
                    await self.stop(session, reason: "清理迟到的听写启动", report: report)
                case .blocked: break
                }
                self.pendingStart = false
                report("dictation_cleanup", self.cleanupConfirmed, self.cleanupConfirmed ? "迟到启动已清理；未发送消息" : "迟到启动的停止仍未确认")
            })
            guard let result else {
                report("dictation_start", false, "听写启动超时；原目标状态待确认，已阻止后续听写和设备写入")
                return
            }
            switch result {
            case .blocked(let detail):
                report("dictation_start", false, detail); return
            case .uncertain(let session, let detail):
                report("dictation_start", false, detail); recording = session; held.end()
            case .started(let session):
                recording = session; report("dictation_start", true, "已确认听写启动；松开停止，不自动发送")
            }
            if !held.released {
                await withCheckedContinuation { held.release = $0 }
            }
            await stop(recording, reason: held.reason, report: report)
        }
        return true
    }
    private func stop(_ recording: any DictationRecording, reason: String, report: @escaping (String, Bool, String) -> Void) async {
        let result = await bounded(timeout: operationTimeout, operation: { await recording.stop() }, timedOut: {}, late: { _ in })
        if result?.0 != true { unresolved = recording }
        report("dictation_stop", result?.0 == true, "\(reason) · \(result?.1 ?? "停止超时，原目标状态待确认")")
    }
    func release(binding: String, ingress: UInt64 = .max) {
        guard let gesture, gesture.binding == binding, ingress > gesture.ingress else { return }
        gesture.end()
    }
    func cancel() { gesture?.end("路由、输入或应用状态变化") }
    func verifyStopped() async -> Bool {
        guard !pendingStart, task == nil else { return false }
        guard let unresolved else { return true }
        let stopped = await bounded(timeout: operationTimeout, operation: { await unresolved.isStopped() }, timedOut: {}, late: { _ in })
        guard stopped == true, let current = self.unresolved, current === unresolved, !pendingStart, task == nil else { return false }
        self.unresolved = nil
        return true
    }
    func waitUntilStopped() async { await task?.value }
}


@MainActor private final class DictationDeadline<Value> {
    var continuation: CheckedContinuation<Value?, Never>?
    var resolved = false
    func finish(_ value: Value?) -> Bool {
        guard !resolved else { return false }
        resolved = true; continuation?.resume(returning: value); continuation = nil
        return true
    }
}

@MainActor private func bounded<Value>(timeout: UInt64, operation: @escaping () async -> Value,
    timedOut: @escaping () -> Void, late: @escaping (Value) async -> Void) async -> Value? {
    let deadline = DictationDeadline<Value>()
    return await withCheckedContinuation { continuation in
        deadline.continuation = continuation
        let timer = Task { @MainActor in
            do {
                try await Task.sleep(nanoseconds: timeout)
                if !deadline.resolved { timedOut(); _ = deadline.finish(nil) }
            } catch { }
        }
        Task { @MainActor in
            let value = await operation()
            timer.cancel()
            if !deadline.finish(value) { await late(value) }
        }
    }
}

#if MI_SYNTHETIC_BACKEND
@MainActor struct SyntheticDictationDriver: DictationDriving {
    var simulated: Bool { true }
    func start(adapter: String, isHeld: @escaping () -> Bool) async -> DictationStart {
        isHeld() ? .started(SyntheticRecording()) : .blocked("合成按键已松开")
    }
    private final class SyntheticRecording: DictationRecording {
        private var stopped = false
        func stop() async -> (Bool, String) { stopped = true; return (true, "合成听写停止；未访问麦克风或发送消息") }
        func isStopped() async -> Bool { stopped }
    }
}
#endif
