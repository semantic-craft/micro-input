import Combine
import Foundation

@MainActor
final class AntigravityStatusController: ObservableObject {
    @Published private(set) var enabled = false
    @Published private(set) var state: AntigravityConnectionState = .disabled
    @Published private(set) var capabilities: AntigravityCapabilities?
    @Published private(set) var rows: [AntigravitySessionRow] = []
    @Published private(set) var lastChanges: [AntigravityEventChange] = []
    @Published private(set) var cursor: UInt64 = 0
    @Published private(set) var transcriptReads: UInt64 = 0

    private let client: any AntigravityStatusServing
    private let pollNanoseconds: UInt64
    private var snapshot: AntigravityStatusSnapshot?
    private var epoch: UInt64 = 0
    private var sequence: UInt64 = 0
    private var refreshingEpoch: UInt64?
    private var suspended = false
    private var connecting: Task<Void, Never>?
    private var polling: Task<Void, Never>?
    private var closing: Task<Void, Never>?

    init(client: any AntigravityStatusServing = AntigravityStatusClient(), pollNanoseconds: UInt64 = 2_000_000_000) {
        self.client = client
        self.pollNanoseconds = pollNanoseconds
    }

    func setEnabled(_ value: Bool) {
        enabled = value
        invalidate()
        if value && !suspended { start() }
    }

    func suspend() {
        suspended = true
        invalidate()
    }

    func resume() {
        guard suspended else { return }
        suspended = false
        if enabled { start() }
    }

    func shutdown() async {
        enabled = false
        invalidate()
        await closing?.value
    }

    func row(for key: AntigravitySessionKey) -> AntigravitySessionRow? {
        guard enabled, !suspended, state == .ready else { return nil }
        return rows.first(where: { $0.key == key })
    }

    private func invalidate() {
        epoch &+= 1
        sequence &+= 1
        refreshingEpoch = nil
        let pendingConnection = connecting
        pendingConnection?.cancel()
        connecting = nil
        polling?.cancel()
        polling = nil
        snapshot = nil
        capabilities = nil
        rows = []
        lastChanges = []
        cursor = 0
        transcriptReads = 0
        state = .disabled
        let previous = closing
        closing = Task { [client] in
            await previous?.value
            await pendingConnection?.value
            await client.close()
        }
    }

    private func start() {
        let expected = epoch
        let previous = closing
        state = .connecting
        connecting = Task { [weak self, client] in
            await previous?.value
            guard let self, self.current(expected) else { return }
            do {
                let result = try await client.connect()
                guard self.current(expected) else { return }
                try self.accept(result)
                self.polling = Task { [weak self] in
                    while !Task.isCancelled {
                        do {
                            try await Task.sleep(nanoseconds: self?.pollNanoseconds ?? 2_000_000_000)
                        } catch {
                            return
                        }
                        guard let self, self.current(expected) else { return }
                        await self.refresh()
                    }
                }
            } catch {
                await self.fail(error, epoch: expected)
            }
        }
    }

    private func current(_ expected: UInt64) -> Bool {
        epoch == expected && enabled && !suspended && !Task.isCancelled
    }

    private func accept(_ value: AntigravityStatusSnapshot) throws {
        try value.validate(after: snapshot?.cursor)
        if let snapshot {
            guard snapshot.instanceId == value.instanceId,
                  snapshot.generation == value.generation,
                  snapshot.allowedWorkspaces == value.allowedWorkspaces,
                  snapshot.capabilities == value.capabilities else {
                throw AntigravityFailure.stale
            }
        }
        snapshot = value
        capabilities = value.capabilities
        rows = value.rows
        lastChanges = value.changes
        cursor = value.cursor
        transcriptReads = value.transcriptReads
        state = .ready
    }

    private func fail(_ error: Error, epoch expected: UInt64) async {
        guard current(expected) else { return }
        snapshot = nil
        capabilities = nil
        rows = []
        lastChanges = []
        cursor = 0
        transcriptReads = 0
        state = .unavailable(error as? AntigravityFailure ?? .unavailable)
        polling?.cancel()
        polling = nil
        let previous = closing
        let close = Task { [client] in
            await previous?.value
            await client.close()
        }
        closing = close
        await close.value
    }

    func refresh() async {
        let expected = epoch
        await connecting?.value
        guard current(expected), state == .ready, refreshingEpoch == nil else { return }
        refreshingEpoch = expected
        defer { if refreshingEpoch == expected { refreshingEpoch = nil } }
        sequence &+= 1
        let request = sequence
        do {
            let value = try await client.read()
            guard current(expected), sequence == request else { return }
            try accept(value)
        } catch {
            guard sequence == request else { return }
            await fail(error, epoch: expected)
        }
    }
}
