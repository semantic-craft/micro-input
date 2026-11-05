import Combine
import Foundation

@MainActor
final class DSHSessionController: ObservableObject {
    @Published private(set) var enabled = false
    @Published private(set) var state: DSHConnectionState = .disabled
    @Published private(set) var rows: [DSHSessionRow] = []
    @Published private(set) var selected: String?
    @Published private(set) var unavailableSessionIds: [String] = []
    @Published private(set) var lastChanges: [DSHSessionChange] = []
    @Published private(set) var cursor: UInt64 = 0
    @Published private(set) var lastResult: DSHOpenResult?
    private let client: any DSHSessionServing
    private let pollNanoseconds: UInt64
    private var snapshot: DSHSessionSnapshot?
    private var epoch: UInt64 = 0
    private var sequence: UInt64 = 0
    private var openingEpoch: UInt64?
    private var refreshingEpoch: UInt64?
    private var suspended = false
    private var connecting: Task<Void, Never>?
    private var polling: Task<Void, Never>?
    private var closing: Task<Void, Never>?

    init(client: any DSHSessionServing = DSHSessionClient(), pollNanoseconds: UInt64 = 2_000_000_000) {
        self.client = client
        self.pollNanoseconds = pollNanoseconds
    }
    func setEnabled(_ value: Bool) {
        enabled = value
        invalidate()
        if value && !suspended { start() }
    }
    func suspend() { suspended = true; invalidate() }
    func resume() { suspended = false; if enabled { start() } }
    func shutdown() async { enabled = false; invalidate(); await closing?.value }
    private func invalidate() {
        epoch &+= 1
        sequence &+= 1
        openingEpoch = nil; refreshingEpoch = nil
        let pendingConnection = connecting
        pendingConnection?.cancel(); connecting = nil
        polling?.cancel(); polling = nil
        snapshot = nil; rows = []; selected = nil; lastResult = nil
        unavailableSessionIds = []; lastChanges = []; cursor = 0
        state = .disabled
        let previous = closing
        closing = Task { [client] in
            await previous?.value
            await pendingConnection?.value
            await client.close()
        }
    }
    private func start() {
        let expected = epoch, previous = closing
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
                        do { try await Task.sleep(nanoseconds: self?.pollNanoseconds ?? 2_000_000_000) }
                        catch { return }
                        guard let self, self.current(expected) else { return }
                        await self.refresh()
                    }
                }
            } catch { await self.fail(error, epoch: expected) }
        }
    }
    private func current(_ expected: UInt64) -> Bool {
        epoch == expected && enabled && !suspended && !Task.isCancelled
    }
    private func accept(_ value: DSHSessionSnapshot) throws {
        try value.validate(after: snapshot?.cursor)
        if let snapshot {
            guard snapshot.hostId == value.hostId, snapshot.generation == value.generation,
                  snapshot.sessionIds == value.sessionIds else { throw DSHFailure.stale }
        }
        snapshot = value; rows = value.rows; selected = value.selected; state = .ready
        unavailableSessionIds = value.unavailableSessionIds; lastChanges = value.changes; cursor = value.cursor
    }
    private func fail(_ error: Error, epoch expected: UInt64) async {
        guard current(expected) else { return }
        rows = []; selected = nil; snapshot = nil; lastResult = nil
        unavailableSessionIds = []; lastChanges = []; cursor = 0
        state = .unavailable(error as? DSHFailure ?? .unavailable)
        polling?.cancel(); polling = nil
        let previous = closing
        let close = Task { [client] in await previous?.value; await client.close() }
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
    func selection(for sessionId: String) -> DSHSelection? {
        guard enabled, !suspended, state == .ready, let snapshot,
              let row = rows.first(where: { $0.sessionId == sessionId }) else { return nil }
        return DSHSelection(sessionId: sessionId, workspaceId: row.workspaceId, hostId: snapshot.hostId,
                              generation: snapshot.generation, localEpoch: epoch)
    }
    func open(_ selection: DSHSelection?, admitted: @escaping @MainActor () -> Bool) async -> DSHOpenResult {
        guard let selection, openingEpoch == nil, refreshingEpoch == nil, let snapshot, enabled, !suspended, state == .ready,
              selection.localEpoch == epoch, selection.hostId == snapshot.hostId,
              selection.generation == snapshot.generation,
              snapshot.sessionIds.contains(selection.sessionId),
              rows.contains(where: { $0.sessionId == selection.sessionId && $0.workspaceId == selection.workspaceId }), admitted() else { return .stale }
        let expected = epoch
        openingEpoch = expected
        sequence &+= 1
        defer { if openingEpoch == expected { openingEpoch = nil } }
        do {
            let response = try await client.open(selection)
            guard current(expected), admitted() else { return .stale }
            guard response.hostId == selection.hostId, response.generation == selection.generation else {
                await fail(DSHFailure.stale, epoch: expected)
                guard current(expected) else { return .stale }
                lastResult = .stale
                return .stale
            }
            let result: DSHOpenResult = response.status == .confirmed && response.selected != selection.sessionId
                ? .unconfirmed : response.status
            if result == .confirmed { selected = selection.sessionId }
            if [.stale, .denied, .unavailable].contains(result) {
                await fail(result == .denied ? DSHFailure.denied : result == .stale ? .stale : .unavailable, epoch: expected)
                guard current(expected) else { return result }
            }
            lastResult = result
            return result
        } catch {
            await fail(error, epoch: expected)
            guard current(expected) else { return .stale }
            let result: DSHOpenResult = error as? DSHFailure == .denied ? .denied : error as? DSHFailure == .stale ? .stale : .unavailable
            lastResult = result
            return result
        }
    }
}
