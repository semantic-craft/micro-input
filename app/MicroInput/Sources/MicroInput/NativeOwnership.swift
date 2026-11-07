import Foundation

struct NativeOwnershipRequest: Codable, Equatable {
    var version = 1
    let requestId: UUID
    let clientInstance: UUID
    let generation: UInt64
    let desiredOwner: String
    let microDrained: Bool
}

struct NativeOwnershipReply: Codable {
    let version: Int
    let requestId: UUID
    let clientInstance: UUID
    let generation: UInt64
    let nativeInstance: UUID
    let nativeGeneration: UInt64
    let state: String
    let reason: String
    let inputDrained: Bool
    let pttDrained: Bool
    let lightingDrained: Bool
    let capabilities: [String]?
}

protocol NativeOwnershipTransport: Sendable {
    func exchange(_ request: NativeOwnershipRequest) async throws -> NativeOwnershipReply
}

@MainActor final class NativeOwnershipController {
    enum State: Equatable {
        case unknown(String)
        case native(UUID, UInt64)
        case released(UUID, UInt64)
    }
    private(set) var state: State = .unknown("尚未取得 Cindy 原生所有权回执")
    private(set) var generation: UInt64
    private(set) var clientInstance: UUID
    private var pending: NativeOwnershipRequest?
    private var requestStarted: ContinuousClock.Instant?
    private var expires: ContinuousClock.Instant?
    private var nativeIdentity: (UUID, UInt64)?
    private var task: Task<Void, Never>?
    private var expiryTask: Task<Void, Never>?
    private let transport: (any NativeOwnershipTransport)?
    private let isolated: Bool
    private let allowUnbridgedDesktop: Bool
    private var bridgeObserved = false
    var changed: (() -> Void)?
    var drain: (() async -> Bool)?
    private(set) var desiredOwner = "native"

    init(transport: (any NativeOwnershipTransport)? = UnixNativeOwnershipTransport(), allowUnbridgedDesktop: Bool = false) {
        self.transport = transport
        self.isolated = false
        self.allowUnbridgedDesktop = allowUnbridgedDesktop
        if let unix = transport as? UnixNativeOwnershipTransport, let fence = unix.readPersistedFence() {
            self.clientInstance = fence.clientInstance
            self.generation = fence.generation &+ 1
        } else {
            self.clientInstance = UUID()
            self.generation = 0
        }
    }
    // Explicit dependency for isolated tests; never selected by production environment flags.
    init(isolated: Bool) {
        precondition(isolated)
        self.transport = nil
        self.isolated = true
        self.allowUnbridgedDesktop = false
        self.clientInstance = UUID()
        self.generation = 0
    }
    private func syncPersistedFenceIfNeeded() {
        guard let unix = transport as? UnixNativeOwnershipTransport,
              let fence = unix.readPersistedFence() else { return }
        clientInstance = fence.clientInstance
        if generation <= fence.generation {
            generation = fence.generation &+ 1
        }
    }
    private var isUnbridgedDesktopActive: Bool {
        guard allowUnbridgedDesktop,
              let unix = transport as? UnixNativeOwnershipTransport else { return false }
        return !FileManager.default.fileExists(atPath: unix.path)
    }
    var allowsExecution: Bool {
        if isolated || isUnbridgedDesktopActive { return true }
        guard case .released = state, let expires else { return false }
        return ContinuousClock.now < expires
    }
    var summary: String {
        if isolated { return "隔离合成所有权" }
        if isUnbridgedDesktopActive { return "本地宿主控制就绪（Cindy 原生桥未启用）" }
        switch state {
        case .unknown(let reason): return "原生所有权未知，按键、听写与灯光暂停 · \(reason)"
        case .native: return "Cindy 原生持有，Micro Input 按键、听写与灯光暂停"
        case .released: return allowsExecution ? "Cindy 已确认释放" : "原生回执已过期，输入暂停"
        }
    }
    func setDesiredOwner(_ value: String) {
        guard value != desiredOwner else { return }
        desiredOwner = value
        invalidate("配置已切换，等待当前代回执")
    }
    func invalidate(_ reason: String) {
        guard !isolated, !isUnbridgedDesktopActive else { return }
        expiryTask?.cancel(); expiryTask = nil
        generation &+= 1
        pending = nil; requestStarted = nil; expires = nil
        state = .unknown(reason)
        changed?()
    }
    func beginRequest(microDrained: Bool) -> NativeOwnershipRequest {
        let request = NativeOwnershipRequest(requestId: UUID(), clientInstance: clientInstance,
            generation: generation, desiredOwner: desiredOwner, microDrained: microDrained)
        pending = request; requestStarted = .now
        return request
    }
    func receive(_ reply: NativeOwnershipReply, for request: NativeOwnershipRequest) {
        guard pending == request, generation == request.generation else { return }
        pending = nil
        let maxLatency: Duration = allowUnbridgedDesktop ? .seconds(2) : .seconds(1)
        guard let started = requestStarted, started.duration(to: .now) < maxLatency,
              reply.version == 1, reply.requestId == request.requestId,
              reply.clientInstance == clientInstance, reply.generation == generation else {
            invalidate("握手身份或代次未确认"); return
        }
        if let previous = nativeIdentity,
           previous.0 == reply.nativeInstance, reply.nativeGeneration < previous.1 {
            invalidate("原生旧代回执已拒绝"); return
        }
        if let previous = nativeIdentity, previous.0 != reply.nativeInstance {
            nativeIdentity = (reply.nativeInstance, reply.nativeGeneration)
            invalidate("Cindy 实例已更换，等待重新确认"); return
        }
        nativeIdentity = (reply.nativeInstance, reply.nativeGeneration)
        if reply.state == "unknown",
           ["stale_generation", "returned_generation", "different_client"].contains(reply.reason) {
            syncPersistedFenceIfNeeded()
            invalidate("握手代次已同步，等待重新确认"); return
        }
        let next: State
        switch reply.state {
        case "native": next = .native(reply.nativeInstance, reply.nativeGeneration)
        case "released":
            guard request.desiredOwner == "micro-input",
                  reply.capabilities?.contains("fenced-release-v1") == true,
                  reply.inputDrained, reply.pttDrained, reply.lightingDrained else {
                invalidate("原生释放未完整确认"); return
            }
            next = .released(reply.nativeInstance, reply.nativeGeneration)
        case "unknown" where allowUnbridgedDesktop
            && request.desiredOwner == "micro-input"
            && reply.capabilities?.contains("fenced-release-v1") == true
            && reply.lightingDrained
            && reply.reason == "native_drain_unconfirmed":
            next = .released(reply.nativeInstance, reply.nativeGeneration)
        default: next = .unknown("Cindy 未确认释放")
        }
        expires = allowUnbridgedDesktop ? ContinuousClock.now.advanced(by: .seconds(2)) : started.advanced(by: .seconds(1))
        expiryTask?.cancel()
        if case .released = next, let deadline = expires {
            let epoch = generation
            expiryTask = Task { @MainActor [weak self] in
                do { try await Task.sleep(until: deadline, tolerance: .zero, clock: .continuous) } catch { return }
                guard let self, self.generation == epoch, self.expires == deadline else { return }
                self.invalidate("原生回执已过期")
            }
        }
        let isSameInstanceRenewal: Bool = {
            if case .released(let prevInstance, _) = state, case .released(let nextInstance, _) = next {
                return prevInstance == nextInstance
            }
            return false
        }()
        if next != state {
            state = next
            if !isSameInstanceRenewal { changed?() }
        }
    }
    func failed(_ request: NativeOwnershipRequest) {
        guard pending == request, generation == request.generation else { return }
        invalidate("Cindy 握手失联或不可用")
    }
    func start() {
        guard task == nil, let transport else { return }
        task = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                if self.isUnbridgedDesktopActive {
                    if self.bridgeObserved || self.nativeIdentity != nil {
                        self.bridgeObserved = false
                        self.nativeIdentity = nil
                        self.expiryTask?.cancel(); self.expiryTask = nil
                        self.pending = nil; self.requestStarted = nil; self.expires = nil
                        self.state = .unknown("Cindy 原生桥已退出，恢复本地宿主控制")
                        self.changed?()
                    }
                    do { try await Task.sleep(for: .milliseconds(500)) } catch { return }
                    continue
                }
                if self.allowUnbridgedDesktop, !self.bridgeObserved {
                    self.bridgeObserved = true
                    self.syncPersistedFenceIfNeeded()
                    self.invalidate("检测到 Cindy 原生所有权桥，等待确认")
                }
                let round = self.generation
                let drained = self.allowsExecution ? false : (await self.drain?() ?? false)
                guard !Task.isCancelled else { return }
                if round != self.generation { continue }
                let request = self.beginRequest(microDrained: drained)
                do {
                    let reply = try await transport.exchange(request)
                    guard !Task.isCancelled else { return }
                    self.receive(reply, for: request)
                } catch { self.failed(request) }
                do { try await Task.sleep(for: .milliseconds(250)) } catch { return }
            }
        }
    }
    func returnToNative() async {
        guard let transport, !isUnbridgedDesktopActive else { stop(); return }
        stop()
        setDesiredOwner("native")
        guard await drain?() == true else { return }
        setDesiredOwner("native")
        let request = beginRequest(microDrained: true)
        do { receive(try await transport.exchange(request), for: request) }
        catch { failed(request) }
    }
    func stop() {
        task?.cancel(); task = nil
        invalidate("握手已停止")
    }
}
