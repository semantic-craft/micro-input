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
    @Published private(set) var lastCommandResult: AntigravityCommandResult?
    @Published private(set) var lastCommandReply: AntigravityCommandReply?
    @Published private(set) var pendingUnknownCommand: AntigravityQueuedCommand?
    var onSnapshotUpdated: (() -> Void)?

    func setVoiceReply(enabled: Bool) async -> (Bool, String) {
        return (false, "Antigravity 2.29.0 language_server does not expose speech generation or voice reply mode")
    }

    #if DEBUG
    func setSnapshotForTesting(rows: [AntigravitySessionRow], selected: AntigravitySessionKey?) {
        self.rows = rows
        self.selected = selected
    }
    #endif

    private let client: any AntigravityStatusServing
    private let pollNanoseconds: UInt64
    private var snapshot: AntigravityStatusSnapshot?
    private var epoch: UInt64 = 0
    private var sequence: UInt64 = 0
    private var nextRequestSequence: UInt64 = 0
    private var openingEpoch: UInt64?
    private var refreshingEpoch: UInt64?
    private var suspended = false
    private var connecting: Task<Void, Never>?
    private var polling: Task<Void, Never>?
    private var closing: Task<Void, Never>?
    private var liveBridgeProcess: Process?

    init(client: any AntigravityStatusServing = AntigravityStatusClient(), pollNanoseconds: UInt64 = 2_000_000_000) {
        self.client = client
        self.pollNanoseconds = pollNanoseconds
    }

    private static func isUnixSocketAcceptingConnections(at path: String) -> Bool {
        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { return false }
        defer { Darwin.close(fd) }
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let utf8 = Array(path.utf8CString)
        let maxLen = MemoryLayout.size(ofValue: addr.sun_path)
        guard utf8.count <= maxLen else { return false }
        withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
            ptr.withMemoryRebound(to: CChar.self, capacity: utf8.count) { dst in
                for (i, ch) in utf8.enumerated() {
                    dst[i] = ch
                }
            }
        }
        let len = socklen_t(MemoryLayout<sockaddr_un>.size)
        let rc = withUnsafePointer(to: &addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.connect(fd, sa, len)
            }
        }
        return rc == 0
    }

    func ensureLiveBridgeProcessIfAvailable() {
        let fm = FileManager.default
        let supportDir = fm.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/MicroInput", isDirectory: true)
        let grantFile = supportDir.appendingPathComponent("antigravity-grant.json")
        let socketFile = supportDir.appendingPathComponent("antigravity.sock")
        if fm.fileExists(atPath: grantFile.path),
           fm.fileExists(atPath: socketFile.path),
           Self.isUnixSocketAcceptingConnections(at: socketFile.path) {
            return
        }
        var candidateScripts: [String] = []
        if let bundled = Bundle.main.resourceURL?.appendingPathComponent("live-bridge.py").path {
            candidateScripts.append(bundled)
        }
        candidateScripts.append(
            URL(fileURLWithPath: fm.currentDirectoryPath).appendingPathComponent("integrations/antigravity/live-bridge.py").path
        )
        candidateScripts.append(
            fm.homeDirectoryForCurrentUser.appendingPathComponent("Projects/micro-input/integrations/antigravity/live-bridge.py").path
        )
        guard let scriptPath = candidateScripts.first(where: { fm.isExecutableFile(atPath: $0) || fm.fileExists(atPath: $0) }) else {
            return
        }
        try? fm.createDirectory(at: supportDir, withIntermediateDirectories: true)
        try? fm.removeItem(at: grantFile)
        try? fm.removeItem(at: socketFile)
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
        proc.arguments = [
            scriptPath,
            "serve",
            "--directory", supportDir.path,
            "--allow-all-local-workspaces",
            "--ttl", "86400"
        ]
        proc.standardOutput = FileHandle.nullDevice
        proc.standardError = FileHandle.nullDevice
        if (try? proc.run()) != nil {
            liveBridgeProcess = proc
            for _ in 0..<60 {
                if fm.fileExists(atPath: grantFile.path) && fm.fileExists(atPath: socketFile.path) {
                    break
                }
                usleep(50_000)
            }
        }
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

    private func makeRequestId(_ explicit: String?) -> String {
        if let explicit, !explicit.isEmpty { return explicit }
        nextRequestSequence &+= 1
        return "ag-req-\(epoch)-\(nextRequestSequence)"
    }

    func createIntent(
        workspacePath: String,
        projectId: String? = nil,
        prompt: String,
        title: String? = nil,
        modelName: String? = nil,
        conversationId: String? = nil,
        requestId: String? = nil
    ) -> AntigravityQueuedCommand? {
        guard enabled, !suspended, state == .ready, let snapshot else { return nil }
        if let pending = pendingUnknownCommand, requestId != pending.operation.requestId {
            lastCommandResult = .unknown
            return nil
        }
        let resolvedProjectId = (projectId?.isEmpty == false)
            ? projectId!
            : (rows.first(where: { $0.workspacePath == workspacePath && !$0.projectId.isEmpty })?.projectId ?? "")
        let intent = AntigravityCreateIntent(
            requestId: makeRequestId(requestId),
            workspacePath: workspacePath,
            projectId: resolvedProjectId,
            prompt: prompt,
            title: title,
            modelName: modelName,
            conversationId: conversationId
        )
        guard intent.valid else { return nil }
        return AntigravityQueuedCommand(
            operation: .create(intent),
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: epoch
        )
    }

    func sendIntent(
        for target: AntigravitySessionKey,
        projectId: String? = nil,
        message: String,
        deliveryMode: AntigravityDeliveryMode = .auto,
        requestId: String? = nil
    ) -> AntigravityQueuedCommand? {
        guard enabled, !suspended, state == .ready, let snapshot else { return nil }
        if let pending = pendingUnknownCommand, requestId != pending.operation.requestId {
            lastCommandResult = .unknown
            return nil
        }
        let resolvedProjectId = (projectId?.isEmpty == false)
            ? projectId
            : rows.first(where: { $0.key == target && !$0.projectId.isEmpty })?.projectId
        let intent = AntigravitySendIntent(
            requestId: makeRequestId(requestId),
            target: target,
            projectId: resolvedProjectId,
            message: message,
            deliveryMode: deliveryMode
        )
        guard intent.valid else { return nil }
        return AntigravityQueuedCommand(
            operation: .send(intent),
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: epoch
        )
    }

    func stopIntent(
        for target: AntigravitySessionKey,
        expectedInvocationNum: UInt64? = nil,
        expectedExecutionNum: UInt64? = nil,
        killBackgroundTasks: Bool = true,
        requestId: String? = nil
    ) -> AntigravityQueuedCommand? {
        guard enabled, !suspended, state == .ready, let snapshot else { return nil }
        if let pending = pendingUnknownCommand, requestId != pending.operation.requestId {
            lastCommandResult = .unknown
            return nil
        }
        let targetRow = rows.first(where: { $0.key == target })
        let resolvedInv = expectedInvocationNum ?? targetRow?.invocationNum ?? 0
        let resolvedExec = expectedExecutionNum ?? targetRow?.executionNum
        let intent = AntigravityStopIntent(
            requestId: makeRequestId(requestId),
            target: target,
            expectedInvocationNum: resolvedInv,
            expectedExecutionNum: resolvedExec,
            killBackgroundTasks: killBackgroundTasks
        )
        guard intent.valid else { return nil }
        return AntigravityQueuedCommand(
            operation: .stop(intent),
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: epoch
        )
    }

    func resolveIntent(
        for target: AntigravitySessionKey,
        interactionId: String,
        interactionType: AntigravityInteractionType,
        decision: AntigravityInteractionDecision,
        selectedOptionIds: [String] = [],
        requestId: String? = nil
    ) -> AntigravityQueuedCommand? {
        guard enabled, !suspended, state == .ready, let snapshot else { return nil }
        if let pending = pendingUnknownCommand, requestId != pending.operation.requestId {
            lastCommandResult = .unknown
            return nil
        }
        let intent = AntigravityResolveIntent(
            requestId: makeRequestId(requestId),
            target: target,
            interactionId: interactionId,
            interactionType: interactionType,
            decision: decision,
            selectedOptionIds: selectedOptionIds
        )
        guard intent.valid else { return nil }
        return AntigravityQueuedCommand(
            operation: .resolve(intent),
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: epoch
        )
    }

    func switchModelIntent(
        for target: AntigravitySessionKey,
        modelName: String,
        thinkingLevel: String,
        requestId: String? = nil
    ) -> AntigravityQueuedCommand? {
        guard enabled, !suspended, state == .ready, let snapshot else { return nil }
        if let pending = pendingUnknownCommand, requestId != pending.operation.requestId {
            lastCommandResult = .unknown
            return nil
        }
        let intent = AntigravitySwitchModelIntent(
            requestId: makeRequestId(requestId),
            target: target,
            modelName: modelName,
            thinkingLevel: thinkingLevel
        )
        guard intent.valid else { return nil }
        return AntigravityQueuedCommand(
            operation: .switchModel(intent),
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: epoch
        )
    }

    func queryIntent(
        requestId: String? = nil,
        expectedOp: String? = nil,
        target: AntigravitySessionKey? = nil,
        projectId: String? = nil
    ) -> AntigravityQueuedCommand? {
        guard enabled, !suspended, state == .ready, let snapshot else { return nil }
        let resolvedRequestId = (requestId?.isEmpty == false)
            ? requestId!
            : (pendingUnknownCommand?.operation.requestId ?? "")
        guard !resolvedRequestId.isEmpty else { return nil }
        let intent = AntigravityQueryIntent(
            requestId: resolvedRequestId,
            expectedOp: expectedOp ?? pendingUnknownCommand?.operation.opName,
            conversationId: target?.conversationId,
            workspacePath: target?.workspacePath,
            projectId: projectId
        )
        guard intent.valid else { return nil }
        return AntigravityQueuedCommand(
            operation: .query(intent),
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: epoch
        )
    }

    func execute(_ command: AntigravityQueuedCommand?, admitted: @escaping @MainActor () -> Bool) async -> AntigravityCommandResult {
        guard let command else {
            let fallback = lastCommandResult ?? .stale
            lastCommandResult = fallback
            return fallback
        }
        guard openingEpoch == nil, refreshingEpoch == nil, let currentSnapshot = snapshot,
              enabled, !suspended, state == .ready,
              command.localEpoch == epoch,
              command.instanceId == currentSnapshot.instanceId,
              command.generation == currentSnapshot.generation,
              admitted() else {
            lastCommandResult = .stale
            return .stale
        }
        let expected = epoch

        if let pending = pendingUnknownCommand, pending.operation.requestId != command.operation.requestId {
            lastCommandResult = .unknown
            return .unknown
        }

        let effectiveOperation: AntigravityCommandOperation
        if let pending = pendingUnknownCommand,
           pending.operation.requestId == command.operation.requestId,
           command.operation.opName != "query" {
            effectiveOperation = .query(AntigravityQueryIntent(
                requestId: pending.operation.requestId,
                expectedOp: pending.operation.opName
            ))
        } else {
            effectiveOperation = command.operation
        }

        switch effectiveOperation {
        case .create(let intent):
            guard currentSnapshot.capabilities.supportsCreateConversation,
                  currentSnapshot.allowedWorkspaces.contains(intent.workspacePath),
                  intent.valid else {
                await fail(AntigravityFailure.denied, epoch: expected)
                guard current(expected) else { return .denied }
                lastCommandResult = .denied
                return .denied
            }
            if let existingBoundProject = rows.first(where: { $0.workspacePath == intent.workspacePath && !$0.projectId.isEmpty })?.projectId,
               existingBoundProject != intent.projectId {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "create",
                    requestId: intent.requestId,
                    conversationId: "",
                    workspacePath: intent.workspacePath,
                    projectId: intent.projectId,
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.projectIdMismatch",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
        case .send(let intent):
            guard currentSnapshot.capabilities.supportsSendMessage,
                  currentSnapshot.allowedWorkspaces.contains(intent.target.workspacePath),
                  intent.valid else {
                await fail(AntigravityFailure.denied, epoch: expected)
                guard current(expected) else { return .denied }
                lastCommandResult = .denied
                return .denied
            }
            guard let targetRow = rows.first(where: { $0.key == intent.target }) else {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "send",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: intent.projectId ?? "",
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.targetNotFound",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
            guard selected == intent.target else {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "send",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: targetRow.projectId,
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.selectionMismatch",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
        case .stop(let intent):
            guard currentSnapshot.capabilities.supportsStopConversation,
                  currentSnapshot.allowedWorkspaces.contains(intent.target.workspacePath),
                  intent.valid else {
                await fail(AntigravityFailure.denied, epoch: expected)
                guard current(expected) else { return .denied }
                lastCommandResult = .denied
                return .denied
            }
            guard let targetRow = rows.first(where: { $0.key == intent.target }) else {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "stop",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: "",
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.targetNotFound",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
            guard selected == intent.target else {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "stop",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: targetRow.projectId,
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.selectionMismatch",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
        case .resolve(let intent):
            guard currentSnapshot.capabilities.supportsResolveInteraction,
                  currentSnapshot.allowedWorkspaces.contains(intent.target.workspacePath),
                  intent.valid else {
                await fail(AntigravityFailure.denied, epoch: expected)
                guard current(expected) else { return .denied }
                lastCommandResult = .denied
                return .denied
            }
            guard let targetRow = rows.first(where: { $0.key == intent.target }) else {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "resolve",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: "",
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.targetNotFound",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
            guard selected == intent.target else {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "resolve",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: targetRow.projectId,
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.selectionMismatch",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
            let isQuestionAsPermission = (intent.interactionType == .question && (intent.decision == .allow || intent.decision == .deny))
                || (targetRow.pendingInteraction?.interactionType == .question && (intent.interactionType != .question || intent.decision == .allow || intent.decision == .deny))
            if isQuestionAsPermission {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "resolve",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: targetRow.projectId,
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.questionCannotApproveAsPermission",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
        case .switchModel(let intent):
            guard currentSnapshot.capabilities.supportsSwitchModelConfig,
                  currentSnapshot.allowedWorkspaces.contains(intent.target.workspacePath),
                  intent.valid else {
                await fail(AntigravityFailure.denied, epoch: expected)
                guard current(expected) else { return .denied }
                lastCommandResult = .denied
                return .denied
            }
            guard let targetRow = rows.first(where: { $0.key == intent.target }) else {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "switch-model",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: "",
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.targetNotFound",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
            guard selected == intent.target else {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "switch-model",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: targetRow.projectId,
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.selectionMismatch",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
            guard targetRow.availableModels.contains(where: { $0.modelName == intent.modelName }) else {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "switch-model",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: targetRow.projectId,
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.unsupportedModel",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
            guard targetRow.supportsThinkingLevel(intent.thinkingLevel, for: intent.modelName) else {
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unconfirmed",
                    commandOp: "switch-model",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: targetRow.projectId,
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "guard.unsupportedThinkingLevel",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
        case .query(let intent):
            guard currentSnapshot.capabilities.schemaVerified, intent.valid else {
                await fail(AntigravityFailure.denied, epoch: expected)
                guard current(expected) else { return .denied }
                lastCommandResult = .denied
                return .denied
            }
        }

        openingEpoch = expected
        sequence &+= 1
        defer { if openingEpoch == expected { openingEpoch = nil } }

        do {
            let response: AntigravityCommandReply
            switch effectiveOperation {
            case .create(let intent):
                response = try await client.create(intent)
            case .send(let intent):
                response = try await client.send(intent)
            case .stop(let intent):
                response = try await client.stop(intent)
            case .resolve(let intent):
                response = try await client.resolve(intent)
            case .switchModel(let intent):
                response = try await client.switchModel(intent)
            case .query(let intent):
                response = try await client.query(intent)
            }
            guard current(expected), admitted() else {
                lastCommandResult = .stale
                return .stale
            }
            guard response.instanceId == command.instanceId,
                  response.generation == command.generation,
                  response.transcriptReads == 0,
                  response.tokenReads == 0 else {
                await fail(AntigravityFailure.stale, epoch: expected)
                guard current(expected) else { return .stale }
                lastCommandResult = .stale
                return .stale
            }

            let updatedSnapshot = AntigravityStatusSnapshot(
                instanceId: currentSnapshot.instanceId,
                generation: currentSnapshot.generation,
                allowedWorkspaces: currentSnapshot.allowedWorkspaces,
                capabilities: currentSnapshot.capabilities,
                cursor: response.cursor,
                rows: response.rows,
                changes: [],
                selected: response.selected,
                transcriptReads: response.transcriptReads,
                tokenReads: response.tokenReads
            )
            snapshot = updatedSnapshot
            rows = response.rows
            selected = response.selected
            cursor = response.cursor
            transcriptReads = response.transcriptReads
            tokenReads = response.tokenReads
            lastCommandReply = response

            let mapped = mapCommandReply(response, operation: command.operation)
            if mapped == .unknown {
                if pendingUnknownCommand == nil {
                    pendingUnknownCommand = command
                }
            } else if pendingUnknownCommand?.operation.requestId == response.requestId {
                pendingUnknownCommand = nil
            }
            lastCommandResult = mapped
            return mapped
        } catch {
            if let failure = error as? AntigravityFailure, failure == .transportTimeout {
                guard current(expected) else { return .stale }
                if pendingUnknownCommand == nil {
                    pendingUnknownCommand = command
                }
                lastCommandReply = AntigravityCommandReply(
                    instanceId: currentSnapshot.instanceId,
                    generation: currentSnapshot.generation,
                    status: "unknown",
                    commandOp: command.operation.opName,
                    requestId: command.operation.requestId,
                    conversationId: "",
                    workspacePath: "",
                    projectId: "",
                    delivery: .none,
                    inFlightSteering: false,
                    reasonCode: "transport.timeout",
                    duplicate: false,
                    reconciled: false,
                    cursor: cursor,
                    rows: rows,
                    selected: selected
                )
                lastCommandResult = .unknown
                return .unknown
            }
            await fail(error, epoch: expected)
            guard current(expected) else { return .stale }
            let result: AntigravityCommandResult = error as? AntigravityFailure == .denied
                ? .denied
                : error as? AntigravityFailure == .stale
                ? .stale
                : .unavailable
            lastCommandResult = result
            return result
        }
    }

    private func mapCommandReply(
        _ response: AntigravityCommandReply,
        operation: AntigravityCommandOperation
    ) -> AntigravityCommandResult {
        switch response.status {
        case "created":
            let createdKey = AntigravitySessionKey(
                conversationId: response.conversationId,
                workspacePath: response.workspacePath
            )
            guard !response.conversationId.isEmpty,
                  !response.workspacePath.isEmpty,
                  !response.projectId.isEmpty,
                  response.selected == createdKey,
                  response.rows.contains(where: { $0.key == createdKey && $0.projectId == response.projectId }) else {
                return .unconfirmed
            }
            return .created
        case "started":
            let targetKey = AntigravitySessionKey(
                conversationId: response.conversationId,
                workspacePath: response.workspacePath
            )
            guard response.delivery == .started,
                  !response.inFlightSteering,
                  response.selected == targetKey,
                  response.rows.contains(where: { $0.key == targetKey }) else {
                return .unconfirmed
            }
            return .started
        case "queued":
            let targetKey = AntigravitySessionKey(
                conversationId: response.conversationId,
                workspacePath: response.workspacePath
            )
            guard response.delivery == .nextInvocation,
                  !response.inFlightSteering,
                  response.selected == targetKey,
                  response.rows.contains(where: { $0.key == targetKey }) else {
                return .unconfirmed
            }
            return .queued
        case "steered":
            let targetKey = AntigravitySessionKey(
                conversationId: response.conversationId,
                workspacePath: response.workspacePath
            )
            guard response.delivery == .steered,
                  response.inFlightSteering,
                  response.selected == targetKey,
                  response.rows.contains(where: { $0.key == targetKey }) else {
                return .unconfirmed
            }
            return .steered
        case "stopped":
            let targetKey = AntigravitySessionKey(
                conversationId: response.conversationId,
                workspacePath: response.workspacePath
            )
            guard response.delivery == .canceled,
                  !response.inFlightSteering,
                  response.selected == targetKey,
                  response.rows.contains(where: { $0.key == targetKey && $0.phase == .canceled && $0.terminationReason == "USER_CANCELED" }) else {
                return .unconfirmed
            }
            return .stopped
        case "resolved":
            let targetKey = AntigravitySessionKey(
                conversationId: response.conversationId,
                workspacePath: response.workspacePath
            )
            guard [.allow, .deny, .answer, .cancel].contains(response.delivery),
                  !response.inFlightSteering,
                  response.selected == targetKey,
                  response.rows.contains(where: { $0.key == targetKey && $0.pendingInteraction == nil }) else {
                return .unconfirmed
            }
            return .resolved
        case "modelSwitched":
            let targetKey = AntigravitySessionKey(
                conversationId: response.conversationId,
                workspacePath: response.workspacePath
            )
            guard response.delivery == .modelSwitched,
                  !response.inFlightSteering,
                  response.selected == targetKey,
                  let targetRow = response.rows.first(where: { $0.key == targetKey }) else {
                return .unconfirmed
            }
            if case .switchModel(let intent) = operation {
                guard targetRow.modelName == intent.modelName,
                      targetRow.thinkingLevel == intent.thinkingLevel else {
                    return .unconfirmed
                }
            }
            return .modelSwitched
        case "noop":
            return .noop
        case "unknown":
            return .unknown
        default:
            return .unconfirmed
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
        lastCommandResult = nil
        lastCommandReply = nil
        pendingUnknownCommand = nil
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
        onSnapshotUpdated?()
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
        lastCommandResult = nil
        lastCommandReply = nil
        pendingUnknownCommand = nil
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
