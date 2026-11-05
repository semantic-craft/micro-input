import Darwin
import Foundation

struct CindyGrant: Codable, Equatable, Sendable {
    var version: Int
    var socketPath: String
    var token: String
    var instanceId: String
    var generation: UInt64
    var ownerEpoch: String
    var scopes: [CindyScope]

    static let defaultPath = (NSHomeDirectory() as NSString)
        .appendingPathComponent("Library/Application Support/MicroInput/cindy-grant.json")

    static func load(path: String = defaultPath) throws -> CindyGrant {
        let uid = geteuid()
        let parent = (path as NSString).deletingLastPathComponent
        var parentStat = stat()
        guard lstat(parent, &parentStat) == 0,
              (parentStat.st_mode & S_IFMT) == S_IFDIR,
              parentStat.st_uid == uid,
              (parentStat.st_mode & 0o077) == 0 else {
            throw CindyFailure.grant
        }

        let fd = Darwin.open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
        guard fd >= 0 else {
            throw CindyFailure.grant
        }
        defer { _ = Darwin.close(fd) }

        var fileStat = stat()
        guard fstat(fd, &fileStat) == 0,
              (fileStat.st_mode & S_IFMT) == S_IFREG,
              fileStat.st_uid == uid,
              (fileStat.st_mode & 0o077) == 0,
              fileStat.st_size > 0,
              fileStat.st_size <= 65536 else {
            throw CindyFailure.grant
        }

        var buffer = [UInt8](repeating: 0, count: Int(fileStat.st_size))
        var offset = 0
        while offset < buffer.count {
            let remaining = buffer.count - offset
            let count = buffer.withUnsafeMutableBytes { rawBuffer -> Int in
                guard let base = rawBuffer.baseAddress else { return -1 }
                return Darwin.read(fd, base.advanced(by: offset), remaining)
            }
            guard count > 0 else {
                throw CindyFailure.grant
            }
            offset += count
        }

        let data = Data(buffer)
        guard let grant = try? JSONDecoder().decode(CindyGrant.self, from: data),
              grant.isValid else {
            throw CindyFailure.grant
        }
        return grant
    }

    var isValid: Bool {
        guard version == 1,
              !socketPath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              socketPath.hasPrefix("/"),
              socketPath.utf8.count < 104,
              token.utf8.count >= 32,
              token.utf8.count <= 256,
              !instanceId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              instanceId.utf8.count <= 256,
              generation > 0,
              !ownerEpoch.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              ownerEpoch.utf8.count <= 256,
              !scopes.isEmpty,
              scopes.count <= 64,
              scopes.allSatisfy(\.isValid) else {
            return false
        }
        var unique = Set<CindyScope>()
        for scope in scopes where !unique.insert(scope).inserted {
            return false
        }
        return true
    }
}

private struct CindyRequest: Encodable {
    var version: Int = 1
    var id: String
    var token: String
    var instanceId: String
    var generation: UInt64
    var ownerEpoch: String
    var op: String
    var scope: CindyScope
    var target: CindyTaskAddress?
}

private struct CindyReply: Decodable {
    var version: Int
    var id: String
    var instanceId: String
    var generation: UInt64
    var ownerEpoch: String
    var status: String
    var rows: [CindyTaskRow]?
    var delivered: CindyTaskAddress?
    var selected: CindyTaskAddress?
    var deepLink: String?
}

actor CindySessionClient: CindySessionServing {
    private let grantPath: String
    private let timeout: Duration
    private var activeGrant: CindyGrant?

    init(
        grantPath: String = CindyGrant.defaultPath,
        timeout: Duration = .seconds(2)
    ) {
        self.grantPath = grantPath
        self.timeout = timeout
    }

    func connect() async -> Result<CindySessionSnapshot, CindyFailure> {
        do {
            let grant = try CindyGrant.load(path: grantPath)
            activeGrant = grant
            return await list(using: grant)
        } catch let failure as CindyFailure {
            activeGrant = nil
            return .failure(failure)
        } catch {
            activeGrant = nil
            return .failure(.grant)
        }
    }

    func read() async -> Result<CindySessionSnapshot, CindyFailure> {
        guard let grant = activeGrant else {
            return await connect()
        }
        guard let currentGrant = try? CindyGrant.load(path: grantPath) else {
            activeGrant = nil
            return .failure(.grant)
        }
        guard currentGrant.token == grant.token,
              currentGrant.socketPath == grant.socketPath,
              currentGrant.scopes == grant.scopes else {
            activeGrant = nil
            return .failure(.grant)
        }
        guard currentGrant.instanceId == grant.instanceId,
              currentGrant.generation == grant.generation,
              currentGrant.ownerEpoch == grant.ownerEpoch else {
            activeGrant = nil
            return .failure(.stale)
        }
        return await list(using: grant)
    }

    func open(_ selection: CindySelection) async -> CindyOpenReply {
        guard let grant = activeGrant else {
            return unavailableReply(for: selection, status: .unavailable)
        }
        guard let currentGrant = try? CindyGrant.load(path: grantPath),
              currentGrant.token == grant.token,
              currentGrant.socketPath == grant.socketPath,
              currentGrant.scopes == grant.scopes else {
            activeGrant = nil
            return unavailableReply(for: selection, status: .denied)
        }
        guard selection.instanceId == grant.instanceId,
              selection.generation == grant.generation,
              selection.ownerEpoch == grant.ownerEpoch,
              currentGrant.instanceId == grant.instanceId,
              currentGrant.generation == grant.generation,
              currentGrant.ownerEpoch == grant.ownerEpoch else {
            activeGrant = nil
            return CindyOpenReply(
                instanceId: currentGrant.instanceId,
                generation: currentGrant.generation,
                ownerEpoch: currentGrant.ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }
        let targetScope = CindyScope(workspacePath: selection.address.workspacePath)
        guard grant.scopes.contains(targetScope), selection.address.isValid else {
            activeGrant = nil
            return unavailableReply(for: selection, status: .denied)
        }

        let requestId = UUID().uuidString
        let request = CindyRequest(
            id: requestId,
            token: grant.token,
            instanceId: grant.instanceId,
            generation: grant.generation,
            ownerEpoch: grant.ownerEpoch,
            op: "open",
            scope: targetScope,
            target: selection.address
        )
        guard let reply = await send(request, socketPath: grant.socketPath) else {
            activeGrant = nil
            return unavailableReply(for: selection, status: .unavailable)
        }
        guard reply.version == 1, reply.id == requestId else {
            activeGrant = nil
            return unavailableReply(for: selection, status: .unavailable)
        }
        if reply.status == "denied" {
            activeGrant = nil
            return unavailableReply(for: selection, status: .denied)
        }
        if reply.status == "stale" ||
            reply.instanceId != grant.instanceId ||
            reply.generation != grant.generation ||
            reply.ownerEpoch != grant.ownerEpoch {
            activeGrant = nil
            return CindyOpenReply(
                instanceId: reply.instanceId,
                generation: reply.generation,
                ownerEpoch: reply.ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }
        guard let status = CindyOpenResult(rawValue: reply.status) else {
            activeGrant = nil
            return unavailableReply(for: selection, status: .unavailable)
        }
        if let selected = reply.selected, !grant.scopes.contains(CindyScope(workspacePath: selected.workspacePath)) {
            activeGrant = nil
            return unavailableReply(for: selection, status: .denied)
        }
        if let delivered = reply.delivered, !grant.scopes.contains(CindyScope(workspacePath: delivered.workspacePath)) {
            activeGrant = nil
            return unavailableReply(for: selection, status: .denied)
        }
        let resolvedStatus: CindyOpenResult
        switch status {
        case .confirmed:
            if reply.delivered == selection.address &&
                reply.selected == selection.address &&
                reply.deepLink == selection.address.deepLink {
                resolvedStatus = .confirmed
            } else {
                resolvedStatus = .unconfirmed
            }
        case .delivered:
            if reply.delivered == selection.address &&
                reply.selected == nil &&
                reply.deepLink == selection.address.deepLink {
                resolvedStatus = .delivered
            } else {
                resolvedStatus = .unconfirmed
            }
        default:
            resolvedStatus = status
        }
        return CindyOpenReply(
            instanceId: reply.instanceId,
            generation: reply.generation,
            ownerEpoch: reply.ownerEpoch,
            status: resolvedStatus,
            delivered: reply.delivered,
            selected: reply.selected,
            deepLink: reply.deepLink
        )
    }

    func close() async {
        activeGrant = nil
    }

    private func list(using grant: CindyGrant) async -> Result<CindySessionSnapshot, CindyFailure> {
        var collected: [CindyTaskRow] = []
        var seenAddresses = Set<CindyTaskAddress>()
        var selected: CindyTaskAddress?

        for scope in grant.scopes {
            let requestId = UUID().uuidString
            let request = CindyRequest(
                id: requestId,
                token: grant.token,
                instanceId: grant.instanceId,
                generation: grant.generation,
                ownerEpoch: grant.ownerEpoch,
                op: "list",
                scope: scope,
                target: nil
            )
            guard let reply = await send(request, socketPath: grant.socketPath) else {
                activeGrant = nil
                return .failure(.unavailable)
            }
            guard reply.version == 1, reply.id == requestId else {
                activeGrant = nil
                return .failure(.invalidReply)
            }
            if reply.status == "denied" {
                activeGrant = nil
                return .failure(.denied)
            }
            if reply.status == "stale" ||
                reply.instanceId != grant.instanceId ||
                reply.generation != grant.generation ||
                reply.ownerEpoch != grant.ownerEpoch {
                activeGrant = nil
                return .failure(.stale)
            }
            if reply.status == "unavailable" {
                activeGrant = nil
                return .failure(.unavailable)
            }
            guard reply.status == "ok", let rows = reply.rows else {
                activeGrant = nil
                return .failure(.invalidReply)
            }

            for row in rows {
                guard row.isValid,
                      row.address.workspacePath == scope.workspacePath,
                      seenAddresses.insert(row.address).inserted else {
                    activeGrant = nil
                    return .failure(.invalidReply)
                }
                collected.append(row)
            }

            if let replySelected = reply.selected {
                guard replySelected.isValid,
                      replySelected.workspacePath == scope.workspacePath else {
                    activeGrant = nil
                    return .failure(.invalidReply)
                }
                selected = replySelected
            }
        }

        if let selected, !seenAddresses.contains(selected) {
            activeGrant = nil
            return .failure(.invalidReply)
        }

        return .success(
            CindySessionSnapshot(
                instanceId: grant.instanceId,
                generation: grant.generation,
                ownerEpoch: grant.ownerEpoch,
                scopes: grant.scopes,
                rows: collected,
                selected: selected
            )
        )
    }

    private func send(_ request: CindyRequest, socketPath: String) async -> CindyReply? {
        guard let wire = try? JSONEncoder().encode(request) else {
            return nil
        }
        let transport = UnixNativeOwnershipTransport(
            path: socketPath,
            requestLimit: 65536,
            responseLimit: 1_048_576,
            timeout: timeout
        )
        guard let data = try? await transport.exchangeJSON(wire),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              Set(object.keys).isSubset(of: [
                  "version",
                  "id",
                  "instanceId",
                  "generation",
                  "ownerEpoch",
                  "status",
                  "rows",
                  "delivered",
                  "selected",
                  "deepLink",
              ]) else {
            return nil
        }
        let addressKeys: Set<String> = ["workspacePath", "sessionId"]
        if let rows = object["rows"] as? [[String: Any]] {
            guard rows.allSatisfy({ row in
                guard Set(row.keys) == ["address", "title", "phase", "attention", "pinned"],
                      let address = row["address"] as? [String: Any] else {
                    return false
                }
                return Set(address.keys) == addressKeys
            }) else {
                return nil
            }
        }
        if let selected = object["selected"] as? [String: Any],
           Set(selected.keys) != addressKeys {
            return nil
        }
        if let delivered = object["delivered"] as? [String: Any],
           Set(delivered.keys) != addressKeys {
            return nil
        }
        return try? JSONDecoder().decode(CindyReply.self, from: data)
    }

    private func unavailableReply(
        for selection: CindySelection,
        status: CindyOpenResult
    ) -> CindyOpenReply {
        CindyOpenReply(
            instanceId: selection.instanceId,
            generation: selection.generation,
            ownerEpoch: selection.ownerEpoch,
            status: status,
            delivered: nil,
            selected: nil,
            deepLink: nil
        )
    }
}
