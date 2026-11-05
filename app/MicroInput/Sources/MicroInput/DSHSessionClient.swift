import Foundation
import Darwin

struct DSHGrant: Codable, Equatable, Sendable {
    let version: Int
    let socketPath: String
    let token: String
    let hostId: String
    let generation: UInt64
    let sessionIds: [String]

    static func load(_ url: URL) throws -> DSHGrant {
        var parent = stat()
        guard lstat(url.deletingLastPathComponent().path, &parent) == 0,
              parent.st_mode & S_IFMT == S_IFDIR, parent.st_uid == getuid(), parent.st_mode & 0o077 == 0 else { throw DSHFailure.grant }
        let fd = Darwin.open(url.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
        guard fd >= 0 else { throw DSHFailure.grant }
        defer { Darwin.close(fd) }
        var info = stat()
        guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_uid == getuid(),
              info.st_mode & 0o077 == 0, info.st_size > 0, info.st_size <= 65536 else { throw DSHFailure.grant }
        var bytes = [UInt8](repeating: 0, count: Int(info.st_size))
        var offset = 0
        while offset < bytes.count {
            let remaining = bytes.count - offset
            let count = bytes.withUnsafeMutableBytes { Darwin.read(fd, $0.baseAddress!.advanced(by: offset), remaining) }
            guard count > 0 else { throw DSHFailure.grant }
            offset += count
        }
        let grant = try JSONDecoder().decode(DSHGrant.self, from: Data(bytes))
        guard grant.version == 1, grant.socketPath.hasPrefix("/"), grant.socketPath.utf8.count < 104,
              grant.token.utf8.count >= 32, grant.token.utf8.count <= 256,
              !grant.hostId.isEmpty, grant.hostId.utf8.count <= 256, grant.generation > 0,
              !grant.sessionIds.isEmpty, grant.sessionIds.count <= 4096, grant.sessionIds.allSatisfy(DSHSessionSnapshot.validID),
              Set(grant.sessionIds).count == grant.sessionIds.count else { throw DSHFailure.grant }
        return grant
    }
}

private struct DSHRequest: Encodable {
    let version = 1
    let subscriptionId: String
    let id: String
    let token: String
    let hostId: String
    let generation: UInt64
    let op: String
    let activate: Bool?
    let after: UInt64?
    let sessionId: String?
    let workspaceId: String?
    let requestId: String?
    let mode: String?
    let text: String?
    let expectedTurn: Int?
    let interactionId: String?
    let interactionType: String?
    let decision: String?
    let optionId: String?
    let provider: String?
    let model: String?
    let thought: String?

    init(
        subscriptionId: String,
        id: String,
        token: String,
        hostId: String,
        generation: UInt64,
        op: String,
        activate: Bool? = nil,
        after: UInt64? = nil,
        sessionId: String? = nil,
        workspaceId: String? = nil,
        requestId: String? = nil,
        mode: String? = nil,
        text: String? = nil,
        expectedTurn: Int? = nil,
        interactionId: String? = nil,
        interactionType: String? = nil,
        decision: String? = nil,
        optionId: String? = nil,
        provider: String? = nil,
        model: String? = nil,
        thought: String? = nil
    ) {
        self.subscriptionId = subscriptionId
        self.id = id
        self.token = token
        self.hostId = hostId
        self.generation = generation
        self.op = op
        self.activate = activate
        self.after = after
        self.sessionId = sessionId
        self.workspaceId = workspaceId
        self.requestId = requestId
        self.mode = mode
        self.text = text
        self.expectedTurn = expectedTurn
        self.interactionId = interactionId
        self.interactionType = interactionType
        self.decision = decision
        self.optionId = optionId
        self.provider = provider
        self.model = model
        self.thought = thought
    }
}

private struct DSHReply: Decodable {
    let version: Int
    let subscriptionId: String
    let id: String
    let hostId: String
    let generation: UInt64
    let status: String
    let cursor: UInt64?
    let rows: [DSHSessionRow]?
    let unavailableSessionIds: [String]?
    let changes: [DSHSessionChange]?
    let selected: String?
    let sessionId: String?
    let workspaceId: String?
    let requestId: String?
    let delivery: String?
    let expectedTurn: Int?
    let currentTurn: Int?
    let queuedCount: Int?
    let duplicate: Bool?
    let reasonCode: String?
    let modelConfig: DSHModelConfig?
}

actor DSHSessionClient: DSHSessionServing {
    private let grantURL: URL
    private var grant: DSHGrant?
    private var allowedSessionIds: [String] = []
    private var allowedWorkspaceIds: Set<String> = []
    private var epoch: UInt64 = 0
    private var cursor: UInt64?
    private var subscriptionId: String?
    private var readingEpoch: UInt64?
    init(grantURL: URL = FileManager.default.homeDirectoryForCurrentUser
         .appendingPathComponent("Library/Application Support/MicroInput/dsh-grant.json")) {
        self.grantURL = grantURL
    }
    func connect() async throws -> DSHSessionSnapshot {
        await close()
        let current = try DSHGrant.load(grantURL)
        grant = current
        allowedSessionIds = current.sessionIds
        subscriptionId = UUID().uuidString
        let expected = epoch
        let response = try await exchange(op: "subscribe", grant: current, expected: expected)
        let result = try snapshot(response, grant: current, after: nil)
        allowedWorkspaceIds = Set(result.rows.map(\.workspaceId))
        cursor = result.cursor
        return result
    }
    func close() async {
        let previous = grant, previousSubscription = subscriptionId
        epoch &+= 1; grant = nil; allowedSessionIds = []; allowedWorkspaceIds = []
        cursor = nil; subscriptionId = nil; readingEpoch = nil
        guard let previous, let previousSubscription else { return }
        let request = DSHRequest(subscriptionId: previousSubscription, id: UUID().uuidString, token: previous.token, hostId: previous.hostId,
                                 generation: previous.generation, op: "close")
        guard let data = try? JSONEncoder().encode(request) else { return }
        _ = try? await UnixNativeOwnershipTransport(path: previous.socketPath, requestLimit: 65536,
            responseLimit: 1_048_576, timeout: .seconds(2)).exchangeJSON(data)
    }
    func read() async throws -> DSHSessionSnapshot {
        let current = try currentGrant(), expected = epoch
        guard let after = cursor, readingEpoch == nil else { throw DSHFailure.stale }
        readingEpoch = expected
        defer { if readingEpoch == expected { readingEpoch = nil } }
        let response = try await exchange(op: "read", after: after, grant: current, expected: expected)
        let result = try snapshot(response, grant: current, after: after)
        allowedWorkspaceIds.formUnion(result.rows.map(\.workspaceId))
        cursor = result.cursor
        return result
    }
    func open(_ selection: DSHSelection) async throws -> DSHOpenReply {
        let current = try currentGrant(), expected = epoch
        guard selection.hostId == current.hostId, selection.generation == current.generation else { throw DSHFailure.stale }
        guard allowedSessionIds.contains(selection.sessionId), selection.workspaceId.utf8.count <= 4096 else { throw DSHFailure.denied }
        let response = try await exchange(op: "open", sessionId: selection.sessionId, workspaceId: selection.workspaceId,
                                          grant: current, expected: expected)
        guard let status = DSHOpenResult(rawValue: response.status), response.rows == nil,
              response.changes == nil, response.cursor == nil, response.unavailableSessionIds == nil,
              status == .confirmed || response.selected == nil else { throw DSHFailure.invalidReply }
        return DSHOpenReply(hostId: response.hostId, generation: response.generation, status: status, selected: response.selected)
    }
    func create(_ intent: DSHCreateIntent) async throws -> DSHCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.hostId == current.hostId, intent.generation == current.generation else { throw DSHFailure.stale }
        guard DSHSessionSnapshot.validID(intent.requestId), allowedWorkspaceIds.contains(intent.workspaceId) else { throw DSHFailure.denied }
        let response = try await exchange(op: "create", workspaceId: intent.workspaceId, requestId: intent.requestId,
                                          grant: current, expected: expected)
        let reply = try commandReply(response, expectedRequestId: intent.requestId)
        if reply.status == "confirmed", let createdId = reply.sessionId, DSHSessionSnapshot.validID(createdId),
           !allowedSessionIds.contains(createdId) {
            allowedSessionIds.append(createdId)
        }
        return reply
    }
    func prompt(_ intent: DSHPromptIntent) async throws -> DSHCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.hostId == current.hostId, intent.generation == current.generation else { throw DSHFailure.stale }
        guard DSHSessionSnapshot.validID(intent.requestId), allowedSessionIds.contains(intent.sessionId),
              allowedWorkspaceIds.contains(intent.workspaceId), !intent.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw DSHFailure.denied
        }
        let response = try await exchange(op: "prompt", sessionId: intent.sessionId, workspaceId: intent.workspaceId,
                                          requestId: intent.requestId, mode: intent.mode.rawValue, text: intent.text,
                                          grant: current, expected: expected)
        return try commandReply(response, expectedRequestId: intent.requestId)
    }
    func cancel(_ intent: DSHCancelIntent) async throws -> DSHCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.hostId == current.hostId, intent.generation == current.generation else { throw DSHFailure.stale }
        guard DSHSessionSnapshot.validID(intent.requestId), allowedSessionIds.contains(intent.sessionId),
              allowedWorkspaceIds.contains(intent.workspaceId), intent.expectedTurn >= 1 else {
            throw DSHFailure.denied
        }
        let response = try await exchange(op: "cancel", sessionId: intent.sessionId, workspaceId: intent.workspaceId,
                                          requestId: intent.requestId, expectedTurn: intent.expectedTurn,
                                          grant: current, expected: expected)
        return try commandReply(response, expectedRequestId: intent.requestId)
    }
    func query(_ intent: DSHQueryIntent) async throws -> DSHCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.hostId == current.hostId, intent.generation == current.generation else { throw DSHFailure.stale }
        guard DSHSessionSnapshot.validID(intent.requestId),
              intent.sessionId == nil || allowedSessionIds.contains(intent.sessionId!) else { throw DSHFailure.denied }
        let response = try await exchange(op: "query", sessionId: intent.sessionId, workspaceId: intent.workspaceId,
                                          requestId: intent.requestId, grant: current, expected: expected)
        let reply = try commandReply(response, expectedRequestId: intent.requestId)
        if reply.status == "confirmed", let createdId = reply.sessionId, DSHSessionSnapshot.validID(createdId),
           !allowedSessionIds.contains(createdId) {
            allowedSessionIds.append(createdId)
        }
        return reply
    }
    func resolve(_ intent: DSHResolveIntent) async throws -> DSHCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.hostId == current.hostId, intent.generation == current.generation else { throw DSHFailure.stale }
        guard DSHSessionSnapshot.validID(intent.requestId), allowedSessionIds.contains(intent.sessionId),
              allowedWorkspaceIds.contains(intent.workspaceId), !intent.interactionId.isEmpty else {
            throw DSHFailure.denied
        }
        let response = try await exchange(
            op: "resolve", sessionId: intent.sessionId, workspaceId: intent.workspaceId,
            requestId: intent.requestId, interactionId: intent.interactionId,
            interactionType: intent.interactionType.rawValue, decision: intent.decision.rawValue,
            optionId: intent.optionId, grant: current, expected: expected
        )
        return try commandReply(response, expectedRequestId: intent.requestId)
    }
    func switchModel(_ intent: DSHSwitchModelIntent) async throws -> DSHCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.hostId == current.hostId, intent.generation == current.generation else { throw DSHFailure.stale }
        guard DSHSessionSnapshot.validID(intent.requestId), allowedSessionIds.contains(intent.sessionId),
              allowedWorkspaceIds.contains(intent.workspaceId), !intent.provider.isEmpty, !intent.model.isEmpty else {
            throw DSHFailure.denied
        }
        let response = try await exchange(
            op: "selectModel", sessionId: intent.sessionId, workspaceId: intent.workspaceId,
            requestId: intent.requestId, provider: intent.provider, model: intent.model,
            thought: intent.thought, grant: current, expected: expected
        )
        return try commandReply(response, expectedRequestId: intent.requestId)
    }
    private func commandReply(_ response: DSHReply, expectedRequestId: String) throws -> DSHCommandReply {
        guard response.rows == nil, response.changes == nil, response.cursor == nil,
              response.unavailableSessionIds == nil else { throw DSHFailure.invalidReply }
        if let replyRequestId = response.requestId, replyRequestId != expectedRequestId {
            throw DSHFailure.invalidReply
        }
        let deliveryMode: DSHPromptMode?
        if let rawDelivery = response.delivery {
            guard let parsed = DSHPromptMode(rawValue: rawDelivery) else { throw DSHFailure.invalidReply }
            deliveryMode = parsed
        } else {
            deliveryMode = nil
        }
        return DSHCommandReply(
            hostId: response.hostId,
            generation: response.generation,
            status: response.status,
            sessionId: response.sessionId,
            workspaceId: response.workspaceId,
            selected: response.selected,
            requestId: response.requestId ?? expectedRequestId,
            delivery: deliveryMode,
            expectedTurn: response.expectedTurn,
            currentTurn: response.currentTurn,
            queuedCount: response.queuedCount,
            duplicate: response.duplicate ?? false,
            reasonCode: response.reasonCode,
            modelConfig: response.modelConfig
        )
    }
    private func failure(_ status: String) -> DSHFailure {
        switch status {
        case "denied": return .denied
        case "stale": return .stale
        case "unavailable": return .unavailable
        case "gap": return .gap
        default: return .invalidReply
        }
    }
    private func exchange(op: String, after: UInt64? = nil, sessionId: String? = nil, workspaceId: String? = nil,
                          requestId: String? = nil, mode: String? = nil, text: String? = nil, expectedTurn: Int? = nil,
                          interactionId: String? = nil, interactionType: String? = nil, decision: String? = nil,
                          optionId: String? = nil, provider: String? = nil, model: String? = nil,
                          thought: String? = nil, grant: DSHGrant, expected: UInt64) async throws -> DSHReply {
        guard expected == epoch, self.grant == grant, let subscription = subscriptionId, !Task.isCancelled else { throw DSHFailure.stale }
        let request = DSHRequest(subscriptionId: subscription, id: UUID().uuidString, token: grant.token, hostId: grant.hostId,
                                 generation: grant.generation, op: op, activate: op == "subscribe" ? false : nil,
                                 after: after, sessionId: sessionId, workspaceId: workspaceId,
                                 requestId: requestId, mode: mode, text: text, expectedTurn: expectedTurn,
                                 interactionId: interactionId, interactionType: interactionType, decision: decision,
                                 optionId: optionId, provider: provider, model: model, thought: thought)
        let data = try await UnixNativeOwnershipTransport(path: grant.socketPath, requestLimit: 65536,
            responseLimit: 1_048_576, timeout: .seconds(2)).exchangeJSON(JSONEncoder().encode(request))
        guard expected == epoch, self.grant == grant, subscriptionId == subscription, !Task.isCancelled,
              try currentGrant() == grant else { throw DSHFailure.stale }
        let allowedKeys: Set<String> = [
            "version", "subscriptionId", "id", "hostId", "generation", "status", "cursor", "rows",
            "unavailableSessionIds", "changes", "selected", "sessionId", "workspaceId", "requestId",
            "delivery", "expectedTurn", "currentTurn", "queuedCount", "duplicate", "reasonCode", "modelConfig"
        ]
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              Set(object.keys).isSubset(of: allowedKeys) else { throw DSHFailure.invalidReply }
        let allowedRowKeys: Set<String> = [
            "sessionId", "workspaceId", "title", "active", "running", "pendingInput",
            "currentTurn", "queuedCount", "pendingInteraction", "modelConfig", "supportedModels"
        ]
        if let rows = object["rows"] as? [[String: Any]], !rows.allSatisfy({ Set($0.keys).isSubset(of: allowedRowKeys) }) { throw DSHFailure.invalidReply }
        if let changes = object["changes"] as? [[String: Any]] {
            guard changes.allSatisfy({ change in
                guard Set(change.keys) == ["sequence", "row"], let row = change["row"] as? [String: Any] else { return false }
                return Set(row.keys).isSubset(of: allowedRowKeys)
            }) else { throw DSHFailure.invalidReply }
        }
        let response = try JSONDecoder().decode(DSHReply.self, from: data)
        guard response.version == 1, response.id == request.id, response.subscriptionId == subscription else { throw DSHFailure.invalidReply }
        guard response.hostId == grant.hostId, response.generation == grant.generation else { throw DSHFailure.stale }
        let validStatuses: Set<String> = [
            "ok", "confirmed", "queued", "steered", "cancelled", "cancelledQueueRemaining",
            "noop", "unknown", "unconfirmed", "denied", "stale", "unavailable", "gap",
            "resolved", "interactionTypeMismatch", "modelSwitched", "unsupportedThought"
        ]
        guard validStatuses.contains(response.status) else { throw DSHFailure.invalidReply }
        if response.status != "ok" {
            guard response.rows == nil && response.changes == nil && response.cursor == nil && response.unavailableSessionIds == nil else {
                throw DSHFailure.invalidReply
            }
        }
        return response
    }
    private func snapshot(_ response: DSHReply, grant: DSHGrant, after: UInt64?) throws -> DSHSessionSnapshot {
        if response.status != "ok" { throw failure(response.status) }
        guard let cursor = response.cursor, let rows = response.rows,
              let unavailableSessionIds = response.unavailableSessionIds,
              let changes = response.changes else { throw DSHFailure.invalidReply }
        let snapshot = DSHSessionSnapshot(hostId: grant.hostId, generation: grant.generation,
                                          sessionIds: allowedSessionIds, cursor: cursor, rows: rows,
                                          unavailableSessionIds: unavailableSessionIds, changes: changes, selected: response.selected)
        try snapshot.validate(after: after)
        return snapshot
    }
    private func currentGrant() throws -> DSHGrant {
        guard let current = grant else { throw DSHFailure.denied }
        let onDisk = try DSHGrant.load(grantURL)
        guard onDisk == current else { throw DSHFailure.stale }
        return current
    }
}
