import Combine
import Foundation

@MainActor
final class ZCodeSessionController: ObservableObject {
    @Published private(set) var enabled = false
    @Published private(set) var state: ZCodeConnectionState = .disabled
    @Published private(set) var rows: [ZCodeTaskRow] = []
    @Published private(set) var selected: ZCodeTaskAddress?
    @Published private(set) var lastResult: ZCodeOpenResult?
    private let client: any ZCodeSessionServing
    private let pollNanoseconds: UInt64
    private var snapshot: ZCodeSessionSnapshot?
    private var epoch: UInt64 = 0
    private var sequence: UInt64 = 0
    private var opening = false
    private var suspended = false
    private var connecting: Task<Void, Never>?
    private var polling: Task<Void, Never>?
    private var closing: Task<Void, Never>?

    init(client: any ZCodeSessionServing = ZCodeSessionClient(), pollNanoseconds: UInt64 = 2_000_000_000) {
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
        connecting?.cancel(); connecting = nil
        polling?.cancel(); polling = nil
        snapshot = nil; rows = []; selected = nil; lastResult = nil
        state = .disabled
        let previous = closing
        closing = Task { [client] in await previous?.value; await client.close() }
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
    private func accept(_ value: ZCodeSessionSnapshot) throws {
        try value.validate()
        if let snapshot {
            guard snapshot.instanceId == value.instanceId, snapshot.generation == value.generation,
                  snapshot.scopes == value.scopes else { throw ZCodeFailure.stale }
        }
        snapshot = value; rows = value.rows; selected = value.selected; state = .ready
    }
    private func fail(_ error: Error, epoch expected: UInt64) async {
        guard current(expected) else { return }
        rows = []; selected = nil; snapshot = nil; lastResult = nil
        state = .unavailable(error as? ZCodeFailure ?? .unavailable)
        polling?.cancel(); polling = nil
        let previous = closing
        let close = Task { [client] in await previous?.value; await client.close() }
        closing = close
        await close.value
    }
    func refresh() async {
        let expected = epoch
        await connecting?.value
        guard current(expected), state == .ready, !opening else { return }
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
    func selection(for address: ZCodeTaskAddress) -> ZCodeSelection? {
        guard enabled, !suspended, state == .ready, let snapshot,
              rows.contains(where: { $0.address == address }) else { return nil }
        return ZCodeSelection(address: address, instanceId: snapshot.instanceId,
                              generation: snapshot.generation, localEpoch: epoch)
    }
    func open(_ selection: ZCodeSelection?, admitted: @escaping @MainActor () -> Bool) async -> ZCodeOpenResult {
        guard let selection, !opening, let snapshot, enabled, !suspended, state == .ready,
              selection.localEpoch == epoch, selection.instanceId == snapshot.instanceId,
              selection.generation == snapshot.generation,
              snapshot.scopes.contains(selection.address.scope),
              rows.contains(where: { $0.address == selection.address }), admitted() else { return .stale }
        opening = true
        sequence &+= 1
        let expected = epoch
        defer { opening = false }
        do {
            let response = try await client.open(selection)
            guard current(expected), admitted() else { return .stale }
            guard response.instanceId == selection.instanceId, response.generation == selection.generation else {
                await fail(ZCodeFailure.stale, epoch: expected)
                guard current(expected) else { return .stale }
                lastResult = .stale
                return .stale
            }
            let result: ZCodeOpenResult = response.status == .confirmed && response.selected != selection.address
                ? .unconfirmed : response.status
            if result == .confirmed { selected = selection.address }
            if [.stale, .denied, .unavailable].contains(result) {
                await fail(result == .denied ? ZCodeFailure.denied : result == .stale ? .stale : .unavailable, epoch: expected)
                guard current(expected) else { return result }
            }
            lastResult = result
            return result
        } catch {
            await fail(error, epoch: expected)
            guard current(expected) else { return .stale }
            let result: ZCodeOpenResult = error as? ZCodeFailure == .denied ? .denied : error as? ZCodeFailure == .stale ? .stale : .unavailable
            lastResult = result
            return result
        }
    }
}
