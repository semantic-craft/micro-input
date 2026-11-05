import Foundation

@MainActor
final class CindySessionController: ObservableObject {
    @Published private(set) var enabled: Bool
    @Published private(set) var state: CindyConnectionState = .disabled
    @Published private(set) var instanceId: String?
    @Published private(set) var generation: UInt64?
    @Published private(set) var ownerEpoch: String?
    @Published private(set) var scopes: [CindyScope] = []
    @Published private(set) var rows: [CindyTaskRow] = []
    @Published private(set) var delivered: CindyTaskAddress?
    @Published private(set) var selected: CindyTaskAddress?
    @Published private(set) var lastDeepLink: String?
    @Published private(set) var lastResult: CindyOpenResult?

    var onOpenConfirmed: ((CindyTaskRow) -> Void)?

    private let client: any CindySessionServing
    private let pollNanoseconds: UInt64
    private var pollTask: Task<Void, Never>?
    private var localEpoch: UInt64 = 0
    private var suspended = false

    init(
        client: any CindySessionServing = CindySessionClient(),
        enabled: Bool = false,
        pollNanoseconds: UInt64 = 1_500_000_000
    ) {
        self.client = client
        self.enabled = enabled
        self.pollNanoseconds = pollNanoseconds
        if enabled {
            startPolling()
        }
    }

    deinit {
        pollTask?.cancel()
    }

    func setEnabled(_ value: Bool) {
        guard enabled != value else {
            if value, case .unavailable = state, !suspended {
                startPolling()
            }
            return
        }
        enabled = value
        localEpoch &+= 1
        if value {
            if !suspended {
                startPolling()
            }
        } else {
            stopAndClear(nextState: .disabled)
        }
    }

    func suspend() {
        guard !suspended else { return }
        suspended = true
        localEpoch &+= 1
        pollTask?.cancel()
        pollTask = nil
        let client = self.client
        Task {
            await client.close()
        }
    }

    func resume() {
        guard suspended else { return }
        suspended = false
        guard enabled else { return }
        startPolling()
    }

    func shutdown() async {
        suspended = true
        enabled = false
        localEpoch &+= 1
        pollTask?.cancel()
        pollTask = nil
        instanceId = nil
        generation = nil
        ownerEpoch = nil
        scopes = []
        rows = []
        delivered = nil
        selected = nil
        lastDeepLink = nil
        lastResult = nil
        state = .disabled
        await client.close()
    }

    func refreshNow() async {
        guard enabled, !suspended else { return }
        let epoch = localEpoch
        let result: Result<CindySessionSnapshot, CindyFailure>
        if state == .ready {
            result = await client.read()
        } else {
            state = .connecting
            result = await client.connect()
        }
        guard enabled, !suspended, epoch == localEpoch else { return }
        apply(result)
    }

    func selection(for address: CindyTaskAddress) -> CindySelection? {
        guard enabled,
              !suspended,
              state == .ready,
              let instanceId,
              let generation,
              let ownerEpoch,
              rows.contains(where: { $0.address == address }) else {
            return nil
        }
        return CindySelection(
            address: address,
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            localEpoch: localEpoch
        )
    }

    func open(_ address: CindyTaskAddress) async -> CindyOpenResult {
        guard let selection = selection(for: address) else {
            lastResult = .stale
            return .stale
        }
        return await openSelection(selection)
    }

    func open(_ selection: CindySelection?, admitted: @escaping @MainActor () -> Bool) async -> CindyOpenResult {
        guard let selection, admitted() else {
            lastResult = .stale
            return .stale
        }
        return await openSelection(selection, admitted: admitted)
    }

    func openSelection(
        _ selection: CindySelection,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> CindyOpenResult {
        guard enabled,
              !suspended,
              state == .ready,
              selection.localEpoch == localEpoch,
              selection.instanceId == instanceId,
              selection.generation == generation,
              selection.ownerEpoch == ownerEpoch,
              scopes.contains(CindyScope(workspacePath: selection.address.workspacePath)),
              let row = rows.first(where: { $0.address == selection.address }),
              admitted?() ?? true else {
            if enabled, !suspended, state == .ready {
                invalidate(with: .stale)
            }
            lastResult = .stale
            return .stale
        }

        let epoch = localEpoch
        let response = await client.open(selection)
        guard enabled, !suspended, epoch == localEpoch, admitted?() ?? true else {
            lastResult = .stale
            return .stale
        }

        if response.instanceId != instanceId ||
            response.generation != generation ||
            response.ownerEpoch != ownerEpoch {
            invalidate(with: .stale)
            lastResult = .stale
            return .stale
        }

        switch response.status {
        case .confirmed:
            guard response.delivered == selection.address,
                  response.selected == selection.address,
                  response.deepLink == selection.address.deepLink else {
                delivered = response.delivered
                selected = nil
                lastDeepLink = response.deepLink
                lastResult = .unconfirmed
                return .unconfirmed
            }
            delivered = selection.address
            selected = selection.address
            lastDeepLink = response.deepLink
            lastResult = .confirmed
            onOpenConfirmed?(row)
            return .confirmed
        case .delivered:
            delivered = selection.address
            selected = nil
            lastDeepLink = response.deepLink
            lastResult = .delivered
            return .delivered
        case .unconfirmed:
            delivered = response.delivered
            selected = nil
            lastDeepLink = response.deepLink
            lastResult = .unconfirmed
            return .unconfirmed
        case .stale:
            invalidate(with: .stale)
            lastResult = .stale
            return .stale
        case .denied:
            invalidate(with: .denied)
            lastResult = .denied
            return .denied
        case .unavailable:
            invalidate(with: .unavailable)
            lastResult = .unavailable
            return .unavailable
        }
    }

    private func startPolling() {
        pollTask?.cancel()
        localEpoch &+= 1
        let epoch = localEpoch
        let interval = pollNanoseconds
        state = .connecting
        pollTask = Task { [weak self] in
            guard let self else { return }
            await self.pollStep(expectedEpoch: epoch, initial: true)
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: interval)
                if Task.isCancelled { break }
                await self.pollStep(expectedEpoch: epoch, initial: false)
            }
        }
    }

    private func pollStep(expectedEpoch: UInt64, initial: Bool) async {
        guard enabled, !suspended, expectedEpoch == localEpoch else { return }
        let result: Result<CindySessionSnapshot, CindyFailure>
        if initial || state != .ready {
            result = await client.connect()
        } else {
            result = await client.read()
        }
        guard enabled, !suspended, expectedEpoch == localEpoch else { return }
        apply(result)
    }

    private func apply(_ result: Result<CindySessionSnapshot, CindyFailure>) {
        switch result {
        case .success(let snapshot):
            let identityChanged =
                instanceId != nil &&
                (instanceId != snapshot.instanceId ||
                 generation != snapshot.generation ||
                 ownerEpoch != snapshot.ownerEpoch)
            if identityChanged {
                localEpoch &+= 1
            }
            instanceId = snapshot.instanceId
            generation = snapshot.generation
            ownerEpoch = snapshot.ownerEpoch
            scopes = snapshot.scopes
            rows = snapshot.rows
            if let snapshotSelected = snapshot.selected,
               snapshot.rows.contains(where: { $0.address == snapshotSelected }) {
                selected = snapshotSelected
            } else {
                selected = nil
            }
            if let currentDelivered = delivered,
               !snapshot.rows.contains(where: { $0.address == currentDelivered }) {
                delivered = nil
            }
            state = .ready
        case .failure(let failure):
            invalidate(with: failure)
        }
    }

    private func invalidate(with failure: CindyFailure) {
        localEpoch &+= 1
        instanceId = nil
        generation = nil
        ownerEpoch = nil
        scopes = []
        rows = []
        delivered = nil
        selected = nil
        lastDeepLink = nil
        state = .unavailable(failure)
    }

    private func stopAndClear(nextState: CindyConnectionState) {
        localEpoch &+= 1
        pollTask?.cancel()
        pollTask = nil
        instanceId = nil
        generation = nil
        ownerEpoch = nil
        scopes = []
        rows = []
        delivered = nil
        selected = nil
        lastDeepLink = nil
        lastResult = nil
        state = nextState
        let client = self.client
        Task {
            await client.close()
        }
    }
}
