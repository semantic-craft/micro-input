import Combine
import Foundation

@MainActor
final class AntigravityStatusController: ObservableObject {
    @Published private(set) var enabled = false
    @Published private(set) var state: AntigravityConnectionState = .disabled
    @Published private(set) var capabilities: AntigravityCapabilities?
    @Published private(set) var rows: [AntigravitySessionRow] = []
    @Published private(set) var selected: AntigravitySessionKey?
    @Published private(set) var lastChanges: [AntigravityEventChange] = []
    @Published private(set) var cursor: UInt64 = 0
    @Published private(set) var transcriptReads: UInt64 = 0
    @Published private(set) var tokenReads: UInt64 = 0
    @Published private(set) var lastResult: AntigravityOpenResult?

    private let client: any AntigravityStatusServing
    private let pollNanoseconds: UInt64
    private var snapshot: AntigravityStatusSnapshot?
    private var epoch: UInt64 = 0
    private var sequence: UInt64 = 0
    private var openingEpoch: UInt64?
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

    func selection(for key: AntigravitySessionKey) -> AntigravitySelection? {
        guard enabled, !suspended, state == .ready, let snapshot,
              rows.contains(where: { $0.key == key }) else { return nil }
        return AntigravitySelection(
            key: key,
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: epoch
        )
    }

    func open(_ selection: AntigravitySelection?, admitted: @escaping @MainActor () -> Bool) async -> AntigravityOpenResult {
        guard let selection, openingEpoch == nil, refreshingEpoch == nil, let snapshot,
              enabled, !suspended, state == .ready else { return .stale }
        let expected = epoch
        guard snapshot.capabilities.supportsCatalogNavigation else {
            await fail(AntigravityFailure.denied, epoch: expected)
            guard current(expected) else { return .denied }
            lastResult = .denied
            return .denied
        }
        guard selection.localEpoch == epoch,
              selection.instanceId == snapshot.instanceId,
              selection.generation == snapshot.generation,
              snapshot.allowedWorkspaces.contains(selection.key.workspacePath),
              rows.contains(where: { $0.key == selection.key }),
              admitted() else { return .stale }
        openingEpoch = expected
        sequence &+= 1
        defer { if openingEpoch == expected { openingEpoch = nil } }
        do {
            let response = try await client.open(selection.key)
            guard current(expected), admitted() else { return .stale }
            guard response.instanceId == selection.instanceId,
                  response.generation == selection.generation,
                  response.transcriptReads == 0,
                  response.tokenReads == 0 else {
                await fail(AntigravityFailure.stale, epoch: expected)
                guard current(expected) else { return .stale }
                lastResult = .stale
                return .stale
            }
            let isConfirmed = response.status == "ok"
                && response.selected == selection.key
                && rows.contains(where: { $0.key == selection.key })
            let result: AntigravityOpenResult = isConfirmed ? .confirmed : .unconfirmed
            if isConfirmed {
                selected = selection.key
            } else if let responseSelected = response.selected,
                      rows.contains(where: { $0.key == responseSelected }) {
                selected = responseSelected
            }
            lastResult = result
            return result
        } catch {
            await fail(error, epoch: expected)
            guard current(expected) else { return .stale }
            let result: AntigravityOpenResult = error as? AntigravityFailure == .denied
                ? .denied
                : error as? AntigravityFailure == .stale
                ? .stale
                : .unavailable
            lastResult = result
            return result
        }
    }

    private func invalidate() {
        epoch &+= 1
        sequence &+= 1
        openingEpoch = nil
        refreshingEpoch = nil
        let pendingConnection = connecting
        pendingConnection?.cancel()
        connecting = nil
        polling?.cancel()
        polling = nil
        snapshot = nil
        capabilities = nil
        rows = []
        selected = nil
        lastChanges = []
        cursor = 0
        transcriptReads = 0
        tokenReads = 0
        lastResult = nil
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
        selected = value.selected
        lastChanges = value.changes
        cursor = value.cursor
        transcriptReads = value.transcriptReads
        tokenReads = value.tokenReads
        state = .ready
    }

    private func fail(_ error: Error, epoch expected: UInt64) async {
        guard current(expected) else { return }
        snapshot = nil
        capabilities = nil
        rows = []
        selected = nil
        lastChanges = []
        cursor = 0
        transcriptReads = 0
        tokenReads = 0
        lastResult = nil
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
        guard current(expected), state == .ready, openingEpoch == nil, refreshingEpoch == nil else { return }
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
