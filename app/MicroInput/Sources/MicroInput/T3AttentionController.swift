import AppKit
import Combine
import Foundation

enum T3ConnectionState: Equatable {
    case disabled, connecting, ready, unavailable(T3Failure)
    var label: String {
        switch self {
        case .disabled: return "T3 只读连接未开启"
        case .connecting: return "正在连接 T3"
        case .ready: return "T3 只读连接已就绪"
        case .unavailable(let failure): return failure.message
        }
    }
}
enum T3OpenResult: String {
    case noCandidate, stale, delivered, failed
    var label: String {
        switch self {
        case .noCandidate: return "没有待处理会话"
        case .stale: return "目标已变化或连接过期，未打开"
        case .delivered: return "已交付 T3 打开请求，目标尚未确认"
        case .failed: return "T3 打开请求失败或超时，交付未确认"
        }
    }
}
@MainActor
final class T3AttentionController: ObservableObject {
    @Published private(set) var enabled = false
    @Published private(set) var state: T3ConnectionState = .disabled
    @Published private(set) var items: [T3AttentionItem] = []
    @Published private(set) var lastResult: T3OpenResult?
    private let client: any T3Reading
    private let opener: (URL) async -> Bool
    private var loop: Task<Void, Never>?
    private var closing: Task<Void, Never>?
    private var generation: UInt64 = 0
    private var snapshot: T3ReadSnapshot?
    private var receivedAt: Date?
    private var observed: [T3AttentionEvent: Date] = [:]
    private var round = T3AttentionRound()
    private var opening = false
    private var requestSequence: UInt64 = 0
    private var acceptedRequest: UInt64 = 0
    private let pollNanoseconds: UInt64
    init(client: any T3Reading = T3AttentionClient(), pollNanoseconds: UInt64 = 2_000_000_000,
         opener: @escaping (URL) async -> Bool = T3AttentionController.openNative) {
        self.client = client; self.pollNanoseconds = pollNanoseconds; self.opener = opener
    }
    static func openNative(_ url: URL) async -> Bool {
        guard let app = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.t3tools.t3code") else { return false }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        return await withCheckedContinuation { continuation in
            let completion = T3OpenCompletion(continuation)
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) { completion.finish(false) }
            NSWorkspace.shared.open([url], withApplicationAt: app, configuration: configuration) { application, error in
                completion.finish(application != nil && error == nil)
            }
        }
    }
    func setEnabled(_ value: Bool) {
        enabled = value
        stop()
        if value { start() }
    }
    func suspend() { stop() }
    func resume() { if enabled { start() } }
    func shutdown() async { enabled = false; stop(); await closing?.value }
    private func stop() {
        generation &+= 1
        items = []; snapshot = nil; receivedAt = nil; observed = [:]; round = T3AttentionRound()
        state = .disabled; lastResult = nil
        let old = loop, previous = closing, epoch = generation
        loop = nil; old?.cancel()
        closing = Task { [weak self, client] in
            await previous?.value
            await old?.value
            let revoked = await client.close()
            if let self, epoch == self.generation, !revoked { self.state = .unavailable(.revocation) }
        }
    }
    private func start() {
        let epoch = generation, previous = closing
        state = .connecting
        loop = Task { [weak self, client] in
            await previous?.value
            guard let self, self.generation == epoch, self.enabled, !Task.isCancelled else { return }
            do {
                let firstID = self.nextRequestID()
                let first = try await client.connect()
                guard self.generation == epoch, !Task.isCancelled else { _ = await client.close(); return }
                _ = try self.accept(first, requestID: firstID)
                while !Task.isCancelled {
                    try await Task.sleep(nanoseconds: self.pollNanoseconds)
                    let requestID = self.nextRequestID()
                    let next: T3ReadSnapshot
                    do { next = try await client.read() }
                    catch T3Failure.expired {
                        self.state = .connecting; self.items = []; self.snapshot = nil; self.receivedAt = nil
                        guard await client.close() else { throw T3Failure.revocation }
                        guard self.generation == epoch, !Task.isCancelled else { return }
                        next = try await client.connect()
                    }
                    guard self.generation == epoch, !Task.isCancelled else { return }
                    _ = try self.accept(next, requestID: requestID)
                }
            } catch {
                guard self.generation == epoch, !Task.isCancelled else { return }
                self.items = []; self.snapshot = nil; self.receivedAt = nil
                self.state = .unavailable(error as? T3Failure ?? .transport)
                if !(await client.close()), self.generation == epoch { self.state = .unavailable(.revocation) }
            }
        }
    }
    private func nextRequestID() -> UInt64 {
        requestSequence &+= 1
        return requestSequence
    }
    private func accept(_ next: T3ReadSnapshot, requestID: UInt64) throws -> Bool {
        guard requestID >= acceptedRequest else { return false }
        if let snapshot {
            guard snapshot.environmentID == next.environmentID else { throw T3Failure.discovery }
            guard next.shell.snapshotSequence >= snapshot.shell.snapshotSequence else { throw T3Failure.incompatible }
        }
        acceptedRequest = requestID
        snapshot = next; receivedAt = Date()
        items = next.shell.attention(environmentID: next.environmentID, now: Date(), observed: &observed)
        state = .ready
        return true
    }
    func selectNext() -> T3Selection? {
        guard state == .ready, let receivedAt, Date().timeIntervalSince(receivedAt) <= 10,
              let event = round.next(items) else { return nil }
        return T3Selection(event: event, generation: generation)
    }
    func open(_ selection: T3Selection?, admitted: @escaping @MainActor () -> Bool) async -> T3OpenResult {
        guard let selection else { lastResult = .noCandidate; return .noCandidate }
        guard !opening, selection.generation == generation, state == .ready, admitted() else { return .stale }
        opening = true
        defer { opening = false }
        let epoch = generation
        lastResult = nil
        do {
            let requestID = nextRequestID()
            let fresh = try await client.read()
            guard epoch == generation, state == .ready, admitted() else { return .stale }
            guard try accept(fresh, requestID: requestID) else { return .stale }
            guard items.contains(where: { $0.event == selection.event }), let url = selection.event.identity.deepLink else {
                lastResult = .stale; return .stale
            }
            let delivered = await opener(url)
            guard epoch == generation else { return delivered ? .delivered : .stale }
            guard delivered else { lastResult = .failed; return .failed }
            round.record(selection.event, items: items)
            lastResult = .delivered
            return .delivered
        } catch {
            guard epoch == generation else { return .stale }
            let result: T3OpenResult = lastResult == .delivered ? .delivered : .failed
            items = []; snapshot = nil; receivedAt = nil
            state = .unavailable(error as? T3Failure ?? .transport)
            loop?.cancel()
            if !(await client.close()), epoch == generation { state = .unavailable(.revocation) }
            lastResult = result
            return result
        }
    }
}

final class T3OpenCompletion: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Bool, Never>?
    init(_ continuation: CheckedContinuation<Bool, Never>) { self.continuation = continuation }
    func finish(_ value: Bool) {
        lock.lock()
        let pending = continuation
        continuation = nil
        lock.unlock()
        pending?.resume(returning: value)
    }
}
