import Foundation
import Darwin

struct ZCodeGrant: Codable, Equatable, Sendable {
    let version: Int
    let socketPath: String
    let token: String
    let instanceId: String
    let generation: UInt64
    let scopes: [ZCodeScope]

    static func load(_ url: URL) throws -> ZCodeGrant {
        var parent = stat()
        guard lstat(url.deletingLastPathComponent().path, &parent) == 0,
              parent.st_mode & S_IFMT == S_IFDIR, parent.st_uid == getuid(), parent.st_mode & 0o077 == 0 else { throw ZCodeFailure.grant }
        let fd = Darwin.open(url.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
        guard fd >= 0 else { throw ZCodeFailure.grant }
        defer { Darwin.close(fd) }
        var info = stat()
        guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_uid == getuid(),
              info.st_mode & 0o077 == 0, info.st_size > 0, info.st_size <= 65536 else { throw ZCodeFailure.grant }
        var bytes = [UInt8](repeating: 0, count: Int(info.st_size))
        var offset = 0
        while offset < bytes.count {
            let remaining = bytes.count - offset
            let count = bytes.withUnsafeMutableBytes { Darwin.read(fd, $0.baseAddress!.advanced(by: offset), remaining) }
            guard count > 0 else { throw ZCodeFailure.grant }
            offset += count
        }
        let grant = try JSONDecoder().decode(ZCodeGrant.self, from: Data(bytes))
        guard grant.version == 1, grant.socketPath.hasPrefix("/"), grant.socketPath.utf8.count < 104,
              grant.token.utf8.count >= 32, grant.token.utf8.count <= 256,
              !grant.instanceId.isEmpty, grant.instanceId.utf8.count <= 256, grant.generation > 0,
              !grant.scopes.isEmpty, grant.scopes.count <= 64, grant.scopes.allSatisfy(\.valid),
              Set(grant.scopes).count == grant.scopes.count else { throw ZCodeFailure.grant }
        return grant
    }
}

private struct ZCodeRequest: Encodable {
    let version = 1
    let id: String
    let token: String
    let instanceId: String
    let generation: UInt64
    let op: String
    let scope: ZCodeScope
    var target: ZCodeTaskAddress? = nil
    var commandId: String? = nil
    var clientId: String? = nil
    var sessionId: String? = nil
    var text: String? = nil
    var requestedDelivery: String? = nil
    var executionId: String? = nil
    var interactionId: String? = nil
    var interactionType: String? = nil
    var decision: String? = nil
    var optionId: String? = nil
    var provider: String? = nil
    var model: String? = nil
    var thought: String? = nil
}

private struct ZCodeReply: Decodable {
    let version: Int
    let id: String
    let instanceId: String
    let generation: UInt64
    let status: String
    let rows: [ZCodeTaskRow]?
    let selected: ZCodeTaskAddress?
    let commandId: String?
    let duplicate: Bool?
    let delivery: ZCodeDelivery?
    let executionId: String?
    let interactionId: String?
    let interactionType: ZCodeInteractionType?
    let modelConfig: ZCodeModelConfig?
    let reasonCode: String?
}

actor ZCodeSessionClient: ZCodeSessionServing {
    private let grantURL: URL
    private let timeout: Duration
    private var grant: ZCodeGrant?
    private var epoch: UInt64 = 0
    init(grantURL: URL = FileManager.default.homeDirectoryForCurrentUser
         .appendingPathComponent("Library/Application Support/MicroInput/zcode-grant.json"),
         timeout: Duration = .seconds(2)) {
        self.grantURL = grantURL
        self.timeout = timeout
    }
    func connect() async throws -> ZCodeSessionSnapshot {
        epoch &+= 1
        grant = nil
        grant = try ZCodeGrant.load(grantURL)
        return try await read()
    }
    func close() async { epoch &+= 1; grant = nil }
    private func currentGrant() throws -> ZCodeGrant {
        guard let grant else { throw ZCodeFailure.denied }
        guard try ZCodeGrant.load(grantURL) == grant else { throw ZCodeFailure.stale }
        return grant
    }
    func read() async throws -> ZCodeSessionSnapshot {
        let current = try currentGrant(), expected = epoch
        var rows: [ZCodeTaskRow] = []
        var selected: ZCodeTaskAddress?
        for scope in current.scopes {
            let result = try await exchange(op: "list", scope: scope, target: nil, grant: current, expected: expected)
            guard result.status == "ok", let scopedRows = result.rows else { throw failure(result.status) }
            guard scopedRows.allSatisfy({ $0.address.scope == scope }),
                  result.selected == nil || scopedRows.contains(where: { $0.address == result.selected }) else { throw ZCodeFailure.invalidReply }
            rows.append(contentsOf: scopedRows)
            if let next = result.selected {
                guard selected == nil || selected == next else { throw ZCodeFailure.invalidReply }
                selected = next
            }
        }
        let result = ZCodeSessionSnapshot(instanceId: current.instanceId, generation: current.generation,
                                          scopes: current.scopes, rows: rows, selected: selected)
        try result.validate()
        return result
    }
    func open(_ selection: ZCodeSelection) async throws -> ZCodeOpenReply {
        let current = try currentGrant(), expected = epoch
        guard selection.instanceId == current.instanceId, selection.generation == current.generation else { throw ZCodeFailure.stale }
        guard selection.address.valid, current.scopes.contains(selection.address.scope) else { throw ZCodeFailure.denied }
        let result = try await exchange(op: "open", scope: selection.address.scope, target: selection.address,
                                        grant: current, expected: expected)
        guard let status = ZCodeOpenResult(rawValue: result.status), result.rows == nil,
              status == .confirmed || result.selected == nil else { throw ZCodeFailure.invalidReply }
        return ZCodeOpenReply(instanceId: result.instanceId, generation: result.generation, status: status, selected: result.selected)
    }
    func create(_ intent: ZCodeCreateIntent) async throws -> ZCodeCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.instanceId == current.instanceId, intent.generation == current.generation else { throw ZCodeFailure.stale }
        guard intent.scope.valid, current.scopes.contains(intent.scope),
              !intent.commandId.isEmpty, intent.commandId.utf8.count <= 4096,
              !intent.clientId.isEmpty, intent.clientId.utf8.count <= 4096,
              intent.sessionId == nil || (intent.sessionId?.isEmpty == false && (intent.sessionId?.utf8.count ?? 0) <= 4096),
              intent.firstInputText == nil || (intent.firstInputText?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false && (intent.firstInputText?.utf8.count ?? 0) <= 32768) else { throw ZCodeFailure.denied }
        let result = try await exchange(op: "create", scope: intent.scope, target: nil, commandId: intent.commandId,
                                        clientId: intent.clientId, sessionId: intent.sessionId, text: intent.firstInputText,
                                        grant: current, expected: expected, transportTimeoutEligible: true)
        guard let status = ZCodeCommandResult(rawValue: result.status),
              [.confirmed, .unconfirmed, .stale, .denied, .unavailable].contains(status),
              result.rows == nil else { throw ZCodeFailure.invalidReply }
        if status == .confirmed {
            guard let selected = result.selected, selected.valid, selected.scope == intent.scope,
                  intent.sessionId == nil || selected.taskId == intent.sessionId else { throw ZCodeFailure.invalidReply }
        }
        return mapCommandReply(result, status: status)
    }
    func send(_ intent: ZCodeSendIntent) async throws -> ZCodeCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.selection.instanceId == current.instanceId, intent.selection.generation == current.generation else { throw ZCodeFailure.stale }
        guard intent.selection.address.valid, current.scopes.contains(intent.selection.address.scope),
              !intent.commandId.isEmpty, intent.commandId.utf8.count <= 4096,
              !intent.clientId.isEmpty, intent.clientId.utf8.count <= 4096,
              !intent.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, intent.text.utf8.count <= 32768 else { throw ZCodeFailure.denied }
        let result = try await exchange(op: "send", scope: intent.selection.address.scope, target: intent.selection.address,
                                        commandId: intent.commandId, clientId: intent.clientId, text: intent.text,
                                        requestedDelivery: intent.requestedDelivery?.rawValue,
                                        grant: current, expected: expected, transportTimeoutEligible: true)
        guard let status = ZCodeCommandResult(rawValue: result.status),
              [.started, .queued, .steered, .unconfirmed, .stale, .denied, .unavailable].contains(status),
              result.rows == nil else { throw ZCodeFailure.invalidReply }
        if [.started, .queued, .steered].contains(status) {
            guard result.selected == intent.selection.address else { throw ZCodeFailure.invalidReply }
            switch status {
            case .steered: guard result.delivery == .guide else { throw ZCodeFailure.invalidReply }
            case .queued: guard result.delivery == .queue else { throw ZCodeFailure.invalidReply }
            case .started: guard result.delivery == .startNow else { throw ZCodeFailure.invalidReply }
            default: break
            }
        }
        return mapCommandReply(result, status: status)
    }
    func stop(_ intent: ZCodeStopIntent) async throws -> ZCodeCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.selection.instanceId == current.instanceId, intent.selection.generation == current.generation else { throw ZCodeFailure.stale }
        guard intent.selection.address.valid, current.scopes.contains(intent.selection.address.scope),
              !intent.commandId.isEmpty, intent.commandId.utf8.count <= 4096,
              !intent.clientId.isEmpty, intent.clientId.utf8.count <= 4096,
              !intent.expectedExecutionId.isEmpty, intent.expectedExecutionId.utf8.count <= 4096 else { throw ZCodeFailure.denied }
        let result = try await exchange(op: "stop", scope: intent.selection.address.scope, target: intent.selection.address,
                                        commandId: intent.commandId, clientId: intent.clientId,
                                        executionId: intent.expectedExecutionId,
                                        grant: current, expected: expected, transportTimeoutEligible: true)
        guard let status = ZCodeCommandResult(rawValue: result.status),
              [.stopped, .noop, .unconfirmed, .stale, .denied, .unavailable].contains(status),
              result.rows == nil else { throw ZCodeFailure.invalidReply }
        if [.stopped, .noop].contains(status) {
            guard result.selected == intent.selection.address else { throw ZCodeFailure.invalidReply }
        }
        return mapCommandReply(result, status: status)
    }
    func resolve(_ intent: ZCodeResolveIntent) async throws -> ZCodeCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.selection.instanceId == current.instanceId, intent.selection.generation == current.generation else { throw ZCodeFailure.stale }
        guard intent.selection.address.valid, current.scopes.contains(intent.selection.address.scope),
              !intent.commandId.isEmpty, intent.commandId.utf8.count <= 4096,
              !intent.clientId.isEmpty, intent.clientId.utf8.count <= 4096,
              !intent.interactionId.isEmpty, intent.interactionId.utf8.count <= 4096,
              intent.optionId == nil || (intent.optionId?.isEmpty == false && (intent.optionId?.utf8.count ?? 0) <= 4096) else { throw ZCodeFailure.denied }
        let result = try await exchange(
            op: "resolve",
            scope: intent.selection.address.scope,
            target: intent.selection.address,
            commandId: intent.commandId,
            clientId: intent.clientId,
            interactionId: intent.interactionId,
            interactionType: intent.interactionType.rawValue,
            decision: intent.decision.rawValue,
            optionId: intent.optionId,
            grant: current,
            expected: expected,
            transportTimeoutEligible: true
        )
        guard let rawStatus = ZCodeCommandResult(rawValue: result.status),
              [.resolved, .noop, .unconfirmed, .stale, .denied, .unavailable].contains(rawStatus),
              result.rows == nil else { throw ZCodeFailure.invalidReply }
        if [.resolved, .noop].contains(rawStatus) {
            guard result.selected == intent.selection.address,
                  result.interactionId == nil || result.interactionId == intent.interactionId,
                  result.interactionType == nil || result.interactionType == intent.interactionType else { throw ZCodeFailure.invalidReply }
        }
        let mappedStatus: ZCodeCommandResult = (rawStatus == .unconfirmed && (result.reasonCode == "guard.questionCannotApproveAsPermission" || result.reasonCode == "guard.interactionTypeMismatch"))
            ? .interactionTypeMismatch
            : rawStatus
        return mapCommandReply(result, status: mappedStatus)
    }
    func switchModel(_ intent: ZCodeSwitchModelIntent) async throws -> ZCodeCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.selection.instanceId == current.instanceId, intent.selection.generation == current.generation else { throw ZCodeFailure.stale }
        guard intent.selection.address.valid, current.scopes.contains(intent.selection.address.scope),
              !intent.commandId.isEmpty, intent.commandId.utf8.count <= 4096,
              !intent.clientId.isEmpty, intent.clientId.utf8.count <= 4096,
              !intent.provider.isEmpty, intent.provider.utf8.count <= 4096,
              !intent.model.isEmpty, intent.model.utf8.count <= 4096,
              !intent.thought.isEmpty, intent.thought.utf8.count <= 4096 else { throw ZCodeFailure.denied }
        let result = try await exchange(
            op: "switchModel",
            scope: intent.selection.address.scope,
            target: intent.selection.address,
            commandId: intent.commandId,
            clientId: intent.clientId,
            provider: intent.provider,
            model: intent.model,
            thought: intent.thought,
            grant: current,
            expected: expected,
            transportTimeoutEligible: true
        )
        guard let rawStatus = ZCodeCommandResult(rawValue: result.status),
              [.confirmed, .modelSwitched, .unconfirmed, .stale, .denied, .unavailable].contains(rawStatus),
              result.rows == nil else { throw ZCodeFailure.invalidReply }
        let mappedStatus: ZCodeCommandResult
        if rawStatus == .confirmed || rawStatus == .modelSwitched {
            guard result.selected == intent.selection.address,
                  let readback = result.modelConfig,
                  readback.valid,
                  readback.provider == intent.provider,
                  readback.model == intent.model,
                  readback.thought == intent.thought else {
                return mapCommandReply(result, status: .unconfirmed)
            }
            mappedStatus = .modelSwitched
        } else if rawStatus == .unconfirmed && result.reasonCode == "guard.unsupportedThoughtLevel" {
            mappedStatus = .unsupportedThought
        } else {
            mappedStatus = rawStatus
        }
        return mapCommandReply(result, status: mappedStatus)
    }
    func query(_ intent: ZCodeQueryIntent) async throws -> ZCodeCommandReply {
        let current = try currentGrant(), expected = epoch
        guard intent.instanceId == current.instanceId, intent.generation == current.generation else { throw ZCodeFailure.stale }
        guard intent.scope.valid, current.scopes.contains(intent.scope),
              intent.target == nil || (intent.target?.valid == true && intent.target?.scope == intent.scope),
              !intent.commandId.isEmpty, intent.commandId.utf8.count <= 4096 else { throw ZCodeFailure.denied }
        let result = try await exchange(op: "query", scope: intent.scope, target: intent.target,
                                        commandId: intent.commandId, grant: current, expected: expected,
                                        transportTimeoutEligible: true)
        guard let status = ZCodeCommandResult(rawValue: result.status), result.rows == nil else { throw ZCodeFailure.invalidReply }
        if [.started, .queued, .steered, .stopped, .resolved, .noop].contains(status), let target = intent.target {
            guard result.selected == target else { throw ZCodeFailure.invalidReply }
        }
        if status == .confirmed {
            guard let selected = result.selected, selected.valid, selected.scope == intent.scope else { throw ZCodeFailure.invalidReply }
        }
        return mapCommandReply(result, status: status)
    }
    private func mapCommandReply(_ reply: ZCodeReply, status: ZCodeCommandResult) -> ZCodeCommandReply {
        ZCodeCommandReply(instanceId: reply.instanceId, generation: reply.generation, status: status,
                          selected: reply.selected, commandId: reply.commandId, duplicate: reply.duplicate ?? false,
                          delivery: reply.delivery, executionId: reply.executionId,
                          interactionId: reply.interactionId, interactionType: reply.interactionType,
                          modelConfig: reply.modelConfig, reasonCode: reply.reasonCode)
    }
    private func failure(_ status: String) -> ZCodeFailure {
        switch status {
        case "denied": return .denied
        case "stale": return .stale
        case "unavailable": return .unavailable
        default: return .invalidReply
        }
    }
    private func exchange(op: String, scope: ZCodeScope, target: ZCodeTaskAddress?,
                          commandId: String? = nil, clientId: String? = nil, sessionId: String? = nil,
                          text: String? = nil, requestedDelivery: String? = nil, executionId: String? = nil,
                          interactionId: String? = nil, interactionType: String? = nil,
                          decision: String? = nil, optionId: String? = nil,
                          provider: String? = nil, model: String? = nil, thought: String? = nil,
                          grant: ZCodeGrant, expected: UInt64, transportTimeoutEligible: Bool = false) async throws -> ZCodeReply {
        guard expected == epoch, self.grant == grant, grant.scopes.contains(scope), !Task.isCancelled else { throw ZCodeFailure.stale }
        let request = ZCodeRequest(id: UUID().uuidString, token: grant.token, instanceId: grant.instanceId,
                                   generation: grant.generation, op: op, scope: scope, target: target,
                                   commandId: commandId, clientId: clientId, sessionId: sessionId,
                                   text: text, requestedDelivery: requestedDelivery, executionId: executionId,
                                   interactionId: interactionId, interactionType: interactionType,
                                   decision: decision, optionId: optionId,
                                   provider: provider, model: model, thought: thought)
        let wire = try JSONEncoder().encode(request)
        let data: Data
        do {
            data = try await UnixNativeOwnershipTransport(path: grant.socketPath, requestLimit: 65536,
                                                          responseLimit: 1_048_576, timeout: timeout).exchangeJSON(wire)
        } catch {
            if transportTimeoutEligible {
                throw ZCodeFailure.transportTimeout
            }
            throw error
        }
        guard expected == epoch, self.grant == grant, !Task.isCancelled,
              try currentGrant() == grant else { throw ZCodeFailure.stale }
        let allowedKeys: Set<String> = [
            "version", "id", "instanceId", "generation", "status", "rows",
            "selected", "commandId", "duplicate", "delivery", "executionId",
            "interactionId", "interactionType", "modelConfig", "reasonCode"
        ]
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              Set(object.keys).isSubset(of: allowedKeys) else { throw ZCodeFailure.invalidReply }
        let addressKeys: Set<String> = ["workspacePath", "workspaceIdentity", "sourceServerId", "remoteSessionId", "taskId"]
        let pendingInteractionKeys: Set<String> = ["interactionId", "interactionType", "toolName", "options"]
        let modelOptionKeys: Set<String> = ["provider", "model", "defaultThought", "supportedThoughts"]
        let modelConfigKeys: Set<String> = ["provider", "model", "thought", "supportedThoughts", "availableModels"]
        let rowKeys: Set<String> = ["address", "title", "liveStatus", "executionId", "pendingInteraction", "modelConfig"]
        func validModelConfigObject(_ raw: Any?) -> Bool {
            guard let raw else { return true }
            guard let config = raw as? [String: Any], Set(config.keys).isSubset(of: modelConfigKeys) else { return false }
            if let available = config["availableModels"] {
                guard let list = available as? [[String: Any]],
                      list.allSatisfy({ Set($0.keys).isSubset(of: modelOptionKeys) }) else { return false }
            }
            return true
        }
        if let rows = object["rows"] as? [[String: Any]] {
            guard rows.allSatisfy({ row in
                guard Set(row.keys).isSubset(of: rowKeys),
                      row["address"] != nil, row["title"] != nil, row["liveStatus"] != nil,
                      let address = row["address"] as? [String: Any],
                      Set(address.keys).isSubset(of: addressKeys) else { return false }
                if let pending = row["pendingInteraction"] {
                    guard let pendingDict = pending as? [String: Any],
                          Set(pendingDict.keys).isSubset(of: pendingInteractionKeys) else { return false }
                }
                return validModelConfigObject(row["modelConfig"])
            }) else { throw ZCodeFailure.invalidReply }
        }
        if let selected = object["selected"] as? [String: Any], !Set(selected.keys).isSubset(of: addressKeys) {
            throw ZCodeFailure.invalidReply
        }
        if !validModelConfigObject(object["modelConfig"]) {
            throw ZCodeFailure.invalidReply
        }
        let result = try JSONDecoder().decode(ZCodeReply.self, from: data)
        guard result.version == 1, result.id == request.id else { throw ZCodeFailure.invalidReply }
        guard result.instanceId == grant.instanceId, result.generation == grant.generation else { throw ZCodeFailure.stale }
        let validStatuses: Set<String> = [
            "ok", "confirmed", "started", "queued", "steered", "stopped", "resolved",
            "noop", "unknown", "denied", "stale", "unavailable", "unconfirmed"
        ]
        guard validStatuses.contains(result.status),
              result.status == "ok" || result.rows == nil else { throw ZCodeFailure.invalidReply }
        return result
    }
}
