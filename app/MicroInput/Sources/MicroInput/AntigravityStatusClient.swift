import Foundation
import Darwin

struct AntigravityGrant: Codable, Equatable, Sendable {
    let version: Int
    let socketPath: String
    let token: String
    let instanceId: String
    let generation: UInt64
    let allowedWorkspaces: [String]
    let capabilities: AntigravityCapabilities

    static func load(_ url: URL) throws -> AntigravityGrant {
        var parent = stat()
        guard lstat(url.deletingLastPathComponent().path, &parent) == 0,
              parent.st_mode & S_IFMT == S_IFDIR,
              parent.st_uid == getuid(),
              parent.st_mode & 0o077 == 0 else {
            throw AntigravityFailure.grant
        }
        let fd = Darwin.open(url.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
        guard fd >= 0 else { throw AntigravityFailure.grant }
        defer { Darwin.close(fd) }
        var info = stat()
        guard fstat(fd, &info) == 0,
              info.st_mode & S_IFMT == S_IFREG,
              info.st_uid == getuid(),
              info.st_mode & 0o077 == 0,
              info.st_size > 0,
              info.st_size <= 65536 else {
            throw AntigravityFailure.grant
        }
        var bytes = [UInt8](repeating: 0, count: Int(info.st_size))
        var offset = 0
        while offset < bytes.count {
            let remaining = bytes.count - offset
            let count = bytes.withUnsafeMutableBytes {
                Darwin.read(fd, $0.baseAddress!.advanced(by: offset), remaining)
            }
            guard count > 0 else { throw AntigravityFailure.grant }
            offset += count
        }
        let grant = try JSONDecoder().decode(AntigravityGrant.self, from: Data(bytes))
        guard grant.version == 1,
              grant.socketPath.hasPrefix("/"),
              grant.socketPath.utf8.count < 104,
              grant.token.utf8.count >= 32,
              grant.token.utf8.count <= 256,
              AntigravityStatusSnapshot.validID(grant.instanceId, limit: 256),
              grant.generation > 0,
              !grant.allowedWorkspaces.isEmpty,
              grant.allowedWorkspaces.count <= 4096,
              grant.allowedWorkspaces.allSatisfy({ AntigravityStatusSnapshot.validID($0, limit: 4096) }),
              Set(grant.allowedWorkspaces).count == grant.allowedWorkspaces.count,
              grant.capabilities.valid else {
            throw AntigravityFailure.grant
        }
        return grant
    }
}

private enum AntigravityWireIntent: Encodable {
    case create(AntigravityCreateIntent)
    case send(AntigravitySendIntent)
    case query(AntigravityQueryIntent)

    func encode(to encoder: Encoder) throws {
        switch self {
        case .create(let value):
            try value.encode(to: encoder)
        case .send(let value):
            try value.encode(to: encoder)
        case .query(let value):
            try value.encode(to: encoder)
        }
    }
}

private struct AntigravityRequest: Encodable {
    let version = 1
    let subscriptionId: String
    let id: String
    let token: String
    let instanceId: String
    let generation: UInt64
    let op: String
    let activate: Bool?
    let after: UInt64?
    let schemaVersion: String?
    let target: AntigravitySessionKey?
    let intent: AntigravityWireIntent?
}

private struct AntigravityReply: Decodable {
    let version: Int
    let subscriptionId: String
    let id: String
    let instanceId: String
    let generation: UInt64
    let status: String
    let capabilities: AntigravityCapabilities?
    let cursor: UInt64?
    let rows: [AntigravitySessionRow]?
    let changes: [AntigravityEventChange]?
    let selected: AntigravitySessionKey?
    let transcriptReads: UInt64?
    let tokenReads: UInt64?
    let commandOp: String?
    let requestId: String?
    let conversationId: String?
    let workspacePath: String?
    let projectId: String?
    let delivery: AntigravityDeliveryOutcome?
    let inFlightSteering: Bool?
    let reasonCode: String?
    let duplicate: Bool?
    let reconciled: Bool?
}

actor AntigravityStatusClient: AntigravityStatusServing {
    private static let catalogNavSchemaVersion = "antigravity-catalog-nav-v1"
    private static let agentapiCommandSchemaVersion = "antigravity-agentapi-v1"
    private let grantURL: URL
    private let commandTimeout: Duration
    private var grant: AntigravityGrant?
    private var epoch: UInt64 = 0
    private var cursor: UInt64?
    private var subscriptionId: String?
    private var readingEpoch: UInt64?

    init(
        grantURL: URL = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/MicroInput/antigravity-grant.json"),
        commandTimeout: Duration = .seconds(2)
    ) {
        self.grantURL = grantURL
        self.commandTimeout = commandTimeout
    }

    func connect() async throws -> AntigravityStatusSnapshot {
        await close()
        let current = try AntigravityGrant.load(grantURL)
        grant = current
        subscriptionId = UUID().uuidString
        let expected = epoch
        let response = try await exchange(op: "subscribe", grant: current, expected: expected)
        let result = try snapshot(response, grant: current, after: nil)
        cursor = result.cursor
        return result
    }

    func close() async {
        let previous = grant
        let previousSubscription = subscriptionId
        epoch &+= 1
        grant = nil
        cursor = nil
        subscriptionId = nil
        readingEpoch = nil
        guard let previous, let previousSubscription else { return }
        let request = AntigravityRequest(
            subscriptionId: previousSubscription,
            id: UUID().uuidString,
            token: previous.token,
            instanceId: previous.instanceId,
            generation: previous.generation,
            op: "close",
            activate: nil,
            after: nil,
            schemaVersion: nil,
            target: nil,
            intent: nil
        )
        guard let data = try? JSONEncoder().encode(request) else { return }
        _ = try? await UnixNativeOwnershipTransport(
            path: previous.socketPath,
            requestLimit: 65536,
            responseLimit: 1_048_576,
            timeout: .seconds(2)
        ).exchangeJSON(data)
    }

    private func currentGrant() throws -> AntigravityGrant {
        guard let grant else { throw AntigravityFailure.denied }
        guard try AntigravityGrant.load(grantURL) == grant else { throw AntigravityFailure.stale }
        return grant
    }

    func read() async throws -> AntigravityStatusSnapshot {
        let current = try currentGrant()
        let expected = epoch
        guard let after = cursor, readingEpoch == nil else { throw AntigravityFailure.stale }
        readingEpoch = expected
        defer { if readingEpoch == expected { readingEpoch = nil } }
        let response = try await exchange(op: "read", after: after, grant: current, expected: expected)
        let result = try snapshot(response, grant: current, after: after)
        cursor = result.cursor
        return result
    }

    func open(_ target: AntigravitySessionKey) async throws -> AntigravityOpenReply {
        guard AntigravityStatusSnapshot.validID(target.conversationId, limit: 256),
              AntigravityStatusSnapshot.validID(target.workspacePath, limit: 4096) else {
            throw AntigravityFailure.denied
        }
        let current = try currentGrant()
        guard current.capabilities.supportsCatalogNavigation,
              current.allowedWorkspaces.contains(target.workspacePath) else {
            throw AntigravityFailure.denied
        }
        let expected = epoch
        let response = try await exchange(
            op: "open",
            schemaVersion: Self.catalogNavSchemaVersion,
            target: target,
            grant: current,
            expected: expected
        )
        let transcriptReads = response.transcriptReads ?? 0
        let tokenReads = response.tokenReads ?? 0
        guard transcriptReads == 0, tokenReads == 0 else {
            throw AntigravityFailure.invalidReply
        }
        if response.status == "ok" || response.status == "unconfirmed" {
            if let selected = response.selected {
                guard AntigravityStatusSnapshot.validID(selected.conversationId, limit: 256),
                      AntigravityStatusSnapshot.validID(selected.workspacePath, limit: 4096),
                      current.allowedWorkspaces.contains(selected.workspacePath) else {
                    throw AntigravityFailure.invalidReply
                }
            } else if response.status == "ok" {
                throw AntigravityFailure.invalidReply
            }
            return AntigravityOpenReply(
                instanceId: current.instanceId,
                generation: current.generation,
                status: response.status,
                selected: response.selected,
                transcriptReads: transcriptReads,
                tokenReads: tokenReads
            )
        }
        throw failure(response.status)
    }

    func create(_ intent: AntigravityCreateIntent) async throws -> AntigravityCommandReply {
        guard intent.valid else { throw AntigravityFailure.denied }
        let current = try currentGrant()
        guard current.capabilities.supportsCreateConversation,
              current.allowedWorkspaces.contains(intent.workspacePath) else {
            throw AntigravityFailure.denied
        }
        let expected = epoch
        let response = try await exchange(
            op: "create",
            schemaVersion: Self.agentapiCommandSchemaVersion,
            intent: .create(intent),
            grant: current,
            expected: expected
        )
        let reply = try commandReply(response, grant: current, expectedRequestId: intent.requestId)
        cursor = reply.cursor
        return reply
    }

    func send(_ intent: AntigravitySendIntent) async throws -> AntigravityCommandReply {
        guard intent.valid else { throw AntigravityFailure.denied }
        let current = try currentGrant()
        guard current.capabilities.supportsSendMessage,
              current.allowedWorkspaces.contains(intent.target.workspacePath) else {
            throw AntigravityFailure.denied
        }
        let expected = epoch
        let response = try await exchange(
            op: "send",
            schemaVersion: Self.agentapiCommandSchemaVersion,
            intent: .send(intent),
            grant: current,
            expected: expected
        )
        let reply = try commandReply(response, grant: current, expectedRequestId: intent.requestId)
        cursor = reply.cursor
        return reply
    }

    func query(_ intent: AntigravityQueryIntent) async throws -> AntigravityCommandReply {
        guard intent.valid else { throw AntigravityFailure.denied }
        let current = try currentGrant()
        guard current.capabilities.schemaVerified else {
            throw AntigravityFailure.denied
        }
        if let workspacePath = intent.workspacePath, !workspacePath.isEmpty {
            guard current.allowedWorkspaces.contains(workspacePath) else {
                throw AntigravityFailure.denied
            }
        }
        let expected = epoch
        let response = try await exchange(
            op: "query",
            schemaVersion: Self.agentapiCommandSchemaVersion,
            intent: .query(intent),
            grant: current,
            expected: expected
        )
        let reply = try commandReply(response, grant: current, expectedRequestId: intent.requestId)
        cursor = reply.cursor
        return reply
    }

    private func commandReply(
        _ response: AntigravityReply,
        grant: AntigravityGrant,
        expectedRequestId: String
    ) throws -> AntigravityCommandReply {
        let allowedCommandStatuses: Set<String> = [
            "created", "started", "queued", "steered", "unconfirmed", "unknown"
        ]
        guard allowedCommandStatuses.contains(response.status) else {
            throw failure(response.status)
        }
        let transcriptReads = response.transcriptReads ?? 0
        let tokenReads = response.tokenReads ?? 0
        guard transcriptReads == 0,
              tokenReads == 0,
              let commandOp = response.commandOp,
              ["create", "send", "query"].contains(commandOp),
              let requestId = response.requestId,
              requestId == expectedRequestId,
              let conversationId = response.conversationId,
              conversationId.utf8.count <= 256,
              let workspacePath = response.workspacePath,
              workspacePath.utf8.count <= 4096,
              workspacePath.isEmpty || grant.allowedWorkspaces.contains(workspacePath),
              let projectId = response.projectId,
              projectId.utf8.count <= 256,
              let delivery = response.delivery,
              let inFlightSteering = response.inFlightSteering,
              let reasonCode = response.reasonCode,
              AntigravityStatusSnapshot.validID(reasonCode, limit: 256),
              let duplicate = response.duplicate,
              let reconciled = response.reconciled,
              let replyCursor = response.cursor,
              let rows = response.rows else {
            throw AntigravityFailure.invalidReply
        }
        let workspaceScope = Set(grant.allowedWorkspaces)
        let rowKeys = Set(rows.map(\.key))
        guard rows.count == rowKeys.count,
              rows.allSatisfy({ $0.valid && workspaceScope.contains($0.workspacePath) }),
              response.selected == nil || rowKeys.contains(response.selected!) else {
            throw AntigravityFailure.invalidReply
        }
        if response.status == "created" {
            let createdKey = AntigravitySessionKey(conversationId: conversationId, workspacePath: workspacePath)
            guard !conversationId.isEmpty,
                  !workspacePath.isEmpty,
                  !projectId.isEmpty,
                  rowKeys.contains(createdKey),
                  response.selected == createdKey else {
                throw AntigravityFailure.invalidReply
            }
        }
        return AntigravityCommandReply(
            instanceId: grant.instanceId,
            generation: grant.generation,
            status: response.status,
            commandOp: commandOp,
            requestId: requestId,
            conversationId: conversationId,
            workspacePath: workspacePath,
            projectId: projectId,
            delivery: delivery,
            inFlightSteering: inFlightSteering,
            reasonCode: reasonCode,
            duplicate: duplicate,
            reconciled: reconciled,
            cursor: replyCursor,
            rows: rows,
            selected: response.selected,
            transcriptReads: transcriptReads,
            tokenReads: tokenReads
        )
    }

    private func snapshot(_ response: AntigravityReply, grant: AntigravityGrant, after: UInt64?) throws -> AntigravityStatusSnapshot {
        guard response.status == "ok",
              let capabilities = response.capabilities,
              let cursor = response.cursor,
              let rows = response.rows,
              let changes = response.changes,
              let transcriptReads = response.transcriptReads else {
            throw failure(response.status)
        }
        guard capabilities == grant.capabilities else {
            throw AntigravityFailure.invalidReply
        }
        let result = AntigravityStatusSnapshot(
            instanceId: grant.instanceId,
            generation: grant.generation,
            allowedWorkspaces: grant.allowedWorkspaces,
            capabilities: capabilities,
            cursor: cursor,
            rows: rows,
            changes: changes,
            selected: response.selected,
            transcriptReads: transcriptReads,
            tokenReads: response.tokenReads ?? 0
        )
        try result.validate(after: after)
        return result
    }

    private func failure(_ status: String) -> AntigravityFailure {
        switch status {
        case "denied": return .denied
        case "stale": return .stale
        case "unavailable": return .unavailable
        case "gap": return .gap
        default: return .invalidReply
        }
    }

    private static let allowedTopKeys: Set<String> = [
        "version", "subscriptionId", "id", "instanceId", "generation",
        "status", "capabilities", "cursor", "rows", "changes", "selected",
        "transcriptReads", "tokenReads", "commandOp", "requestId",
        "conversationId", "workspacePath", "projectId", "delivery",
        "inFlightSteering", "reasonCode", "duplicate", "reconciled"
    ]
    private static let requiredRowKeys: Set<String> = [
        "conversationId", "workspacePath", "modelName", "phase",
        "lastEventType", "invocationNum", "stepIdx", "executionNum",
        "fullyIdle", "transitionCount"
    ]
    private static let allowedRowKeys: Set<String> = requiredRowKeys.union([
        "title", "projectId", "lastToolName", "terminationReason"
    ])

    private static func validRowObject(_ row: [String: Any]) -> Bool {
        let keys = Set(row.keys)
        return requiredRowKeys.isSubset(of: keys) && keys.isSubset(of: allowedRowKeys)
    }

    private static func validSelectedObject(_ raw: Any) -> Bool {
        if raw is NSNull { return true }
        guard let dict = raw as? [String: Any] else { return false }
        return Set(dict.keys) == ["conversationId", "workspacePath"]
    }

    private func exchange(
        op: String,
        after: UInt64? = nil,
        schemaVersion: String? = nil,
        target: AntigravitySessionKey? = nil,
        intent: AntigravityWireIntent? = nil,
        grant: AntigravityGrant,
        expected: UInt64
    ) async throws -> AntigravityReply {
        guard expected == epoch, self.grant == grant, let subscription = subscriptionId, !Task.isCancelled else {
            throw AntigravityFailure.stale
        }
        let request = AntigravityRequest(
            subscriptionId: subscription,
            id: UUID().uuidString,
            token: grant.token,
            instanceId: grant.instanceId,
            generation: grant.generation,
            op: op,
            activate: op == "subscribe" ? false : nil,
            after: after,
            schemaVersion: schemaVersion,
            target: target,
            intent: intent
        )
        let isCommandOp = ["create", "send", "query"].contains(op)
        let timeout = isCommandOp ? commandTimeout : .seconds(2)
        let startedAt = ContinuousClock.now
        let data: Data
        do {
            data = try await UnixNativeOwnershipTransport(
                path: grant.socketPath,
                requestLimit: 65536,
                responseLimit: 1_048_576,
                timeout: timeout
            ).exchangeJSON(JSONEncoder().encode(request))
        } catch {
            if isCommandOp, ContinuousClock.now - startedAt >= timeout {
                throw AntigravityFailure.transportTimeout
            }
            throw error
        }
        guard expected == epoch, self.grant == grant, subscriptionId == subscription, !Task.isCancelled,
              try currentGrant() == grant else {
            throw AntigravityFailure.stale
        }
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              Set(object.keys).isSubset(of: Self.allowedTopKeys) else {
            throw AntigravityFailure.invalidReply
        }
        if let rawSelected = object["selected"], !Self.validSelectedObject(rawSelected) {
            throw AntigravityFailure.invalidReply
        }
        if let rows = object["rows"] as? [[String: Any]],
           !rows.allSatisfy(Self.validRowObject) {
            throw AntigravityFailure.invalidReply
        }
        if let changes = object["changes"] as? [[String: Any]] {
            guard changes.allSatisfy({ change in
                guard Set(change.keys) == ["sequence", "eventKey", "row"],
                      let row = change["row"] as? [String: Any] else { return false }
                return Self.validRowObject(row)
            }) else {
                throw AntigravityFailure.invalidReply
            }
        }
        let response = try JSONDecoder().decode(AntigravityReply.self, from: data)
        guard response.version == 1,
              response.id == request.id,
              response.subscriptionId == subscription else {
            throw AntigravityFailure.invalidReply
        }
        guard response.instanceId == grant.instanceId,
              response.generation == grant.generation else {
            throw AntigravityFailure.stale
        }
        if isCommandOp {
            let allowedCommandStatuses: Set<String> = [
                "created", "started", "queued", "steered", "unconfirmed",
                "unknown", "denied", "stale", "unavailable"
            ]
            guard allowedCommandStatuses.contains(response.status) else {
                throw AntigravityFailure.invalidReply
            }
            return response
        }
        let allowedStatuses = op == "open"
            ? ["ok", "unconfirmed", "denied", "stale", "unavailable"]
            : ["ok", "denied", "stale", "unavailable", "gap"]
        guard allowedStatuses.contains(response.status),
              response.status == "ok" || (response.rows == nil && response.changes == nil && response.cursor == nil) else {
            throw AntigravityFailure.invalidReply
        }
        return response
    }
}
