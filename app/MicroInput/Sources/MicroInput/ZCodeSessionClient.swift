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
    let target: ZCodeTaskAddress?
}

private struct ZCodeReply: Decodable {
    let version: Int
    let id: String
    let instanceId: String
    let generation: UInt64
    let status: String
    let rows: [ZCodeTaskRow]?
    let selected: ZCodeTaskAddress?
}

actor ZCodeSessionClient: ZCodeSessionServing {
    private let grantURL: URL
    private var grant: ZCodeGrant?
    private var epoch: UInt64 = 0
    init(grantURL: URL = FileManager.default.homeDirectoryForCurrentUser
         .appendingPathComponent("Library/Application Support/MicroInput/zcode-grant.json")) {
        self.grantURL = grantURL
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
    private func failure(_ status: String) -> ZCodeFailure {
        switch status {
        case "denied": return .denied
        case "stale": return .stale
        case "unavailable": return .unavailable
        default: return .invalidReply
        }
    }
    private func exchange(op: String, scope: ZCodeScope, target: ZCodeTaskAddress?, grant: ZCodeGrant,
                          expected: UInt64) async throws -> ZCodeReply {
        guard expected == epoch, self.grant == grant, grant.scopes.contains(scope), !Task.isCancelled else { throw ZCodeFailure.stale }
        let request = ZCodeRequest(id: UUID().uuidString, token: grant.token, instanceId: grant.instanceId,
                                   generation: grant.generation, op: op, scope: scope, target: target)
        let wire = try JSONEncoder().encode(request)
        let data = try await UnixNativeOwnershipTransport(path: grant.socketPath, requestLimit: 65536,
                                                         responseLimit: 1_048_576, timeout: .seconds(2)).exchangeJSON(wire)
        guard expected == epoch, self.grant == grant, !Task.isCancelled,
              try currentGrant() == grant else { throw ZCodeFailure.stale }
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              Set(object.keys).isSubset(of: ["version", "id", "instanceId", "generation", "status", "rows", "selected"]) else { throw ZCodeFailure.invalidReply }
        let addressKeys: Set<String> = ["workspacePath", "workspaceIdentity", "sourceServerId", "remoteSessionId", "taskId"]
        if let rows = object["rows"] as? [[String: Any]] {
            guard rows.allSatisfy({ row in
                guard Set(row.keys) == ["address", "title", "liveStatus"], let address = row["address"] as? [String: Any] else { return false }
                return Set(address.keys).isSubset(of: addressKeys)
            }) else { throw ZCodeFailure.invalidReply }
        }
        if let selected = object["selected"] as? [String: Any], !Set(selected.keys).isSubset(of: addressKeys) {
            throw ZCodeFailure.invalidReply
        }
        let result = try JSONDecoder().decode(ZCodeReply.self, from: data)
        guard result.version == 1, result.id == request.id else { throw ZCodeFailure.invalidReply }
        guard result.instanceId == grant.instanceId, result.generation == grant.generation else { throw ZCodeFailure.stale }
        guard ["ok", "confirmed", "denied", "stale", "unavailable", "unconfirmed"].contains(result.status),
              ["ok", "confirmed"].contains(result.status) || (result.rows == nil && result.selected == nil) else { throw ZCodeFailure.invalidReply }
        return result
    }
}
