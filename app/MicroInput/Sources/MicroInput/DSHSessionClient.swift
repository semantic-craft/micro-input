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
}

actor DSHSessionClient: DSHSessionServing {
    private let grantURL: URL
    private var grant: DSHGrant?
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
        subscriptionId = UUID().uuidString
        let expected = epoch
        let response = try await exchange(op: "subscribe", grant: current, expected: expected)
        let result = try snapshot(response, grant: current, after: nil)
        cursor = result.cursor
        return result
    }
    func close() async {
        let previous = grant, previousSubscription = subscriptionId
        epoch &+= 1; grant = nil; cursor = nil; subscriptionId = nil; readingEpoch = nil
        guard let previous, let previousSubscription else { return }
        let request = DSHRequest(subscriptionId: previousSubscription, id: UUID().uuidString, token: previous.token, hostId: previous.hostId,
                                 generation: previous.generation, op: "close", activate: nil, after: nil,
                                 sessionId: nil, workspaceId: nil)
        guard let data = try? JSONEncoder().encode(request) else { return }
        _ = try? await UnixNativeOwnershipTransport(path: previous.socketPath, requestLimit: 65536,
                                                    responseLimit: 1_048_576, timeout: .seconds(2)).exchangeJSON(data)
    }
    private func currentGrant() throws -> DSHGrant {
        guard let grant else { throw DSHFailure.denied }
        guard try DSHGrant.load(grantURL) == grant else { throw DSHFailure.stale }
        return grant
    }
    func read() async throws -> DSHSessionSnapshot {
        let current = try currentGrant(), expected = epoch
        guard let after = cursor, readingEpoch == nil else { throw DSHFailure.stale }
        readingEpoch = expected
        defer { if readingEpoch == expected { readingEpoch = nil } }
        let response = try await exchange(op: "read", after: after, grant: current, expected: expected)
        let result = try snapshot(response, grant: current, after: after)
        cursor = result.cursor
        return result
    }
    private func snapshot(_ response: DSHReply, grant: DSHGrant, after: UInt64?) throws -> DSHSessionSnapshot {
        guard response.status == "ok", let cursor = response.cursor, let rows = response.rows,
              let unavailable = response.unavailableSessionIds, let changes = response.changes else { throw failure(response.status) }
        let result = DSHSessionSnapshot(hostId: grant.hostId, generation: grant.generation,
            sessionIds: grant.sessionIds, cursor: cursor, rows: rows, unavailableSessionIds: unavailable,
            changes: changes, selected: response.selected)
        try result.validate(after: after)
        return result
    }
    func open(_ selection: DSHSelection) async throws -> DSHOpenReply {
        let current = try currentGrant(), expected = epoch
        guard selection.hostId == current.hostId, selection.generation == current.generation else { throw DSHFailure.stale }
        guard current.sessionIds.contains(selection.sessionId), selection.workspaceId.utf8.count <= 4096 else { throw DSHFailure.denied }
        let response = try await exchange(op: "open", selection: selection, grant: current, expected: expected)
        guard let status = DSHOpenResult(rawValue: response.status), response.rows == nil,
              response.changes == nil, response.cursor == nil, response.unavailableSessionIds == nil,
              status == .confirmed || response.selected == nil else { throw DSHFailure.invalidReply }
        return DSHOpenReply(hostId: response.hostId, generation: response.generation, status: status, selected: response.selected)
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
    private func exchange(op: String, after: UInt64? = nil, selection: DSHSelection? = nil,
                          grant: DSHGrant, expected: UInt64) async throws -> DSHReply {
        guard expected == epoch, self.grant == grant, let subscription = subscriptionId, !Task.isCancelled else { throw DSHFailure.stale }
        let request = DSHRequest(subscriptionId: subscription, id: UUID().uuidString, token: grant.token, hostId: grant.hostId,
                                 generation: grant.generation, op: op, activate: op == "subscribe" ? false : nil,
                                 after: after, sessionId: selection?.sessionId, workspaceId: selection?.workspaceId)
        let data = try await UnixNativeOwnershipTransport(path: grant.socketPath, requestLimit: 65536,
            responseLimit: 1_048_576, timeout: .seconds(2)).exchangeJSON(JSONEncoder().encode(request))
        guard expected == epoch, self.grant == grant, subscriptionId == subscription, !Task.isCancelled,
              try currentGrant() == grant else { throw DSHFailure.stale }
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              Set(object.keys).isSubset(of: ["version", "subscriptionId", "id", "hostId", "generation", "status", "cursor", "rows", "unavailableSessionIds", "changes", "selected"]) else { throw DSHFailure.invalidReply }
        let rowKeys: Set<String> = ["sessionId", "workspaceId", "title", "active", "running", "pendingInput"]
        if let rows = object["rows"] as? [[String: Any]], !rows.allSatisfy({ Set($0.keys) == rowKeys }) { throw DSHFailure.invalidReply }
        if let changes = object["changes"] as? [[String: Any]] {
            guard changes.allSatisfy({ change in
                guard Set(change.keys) == ["sequence", "row"], let row = change["row"] as? [String: Any] else { return false }
                return Set(row.keys) == rowKeys
            }) else { throw DSHFailure.invalidReply }
        }
        let response = try JSONDecoder().decode(DSHReply.self, from: data)
        guard response.version == 1, response.id == request.id, response.subscriptionId == subscription else { throw DSHFailure.invalidReply }
        guard response.hostId == grant.hostId, response.generation == grant.generation else { throw DSHFailure.stale }
        guard ["ok", "confirmed", "unconfirmed", "denied", "stale", "unavailable", "gap"].contains(response.status),
              ["ok", "confirmed"].contains(response.status) || (response.rows == nil && response.changes == nil && response.selected == nil) else { throw DSHFailure.invalidReply }
        return response
    }
}
