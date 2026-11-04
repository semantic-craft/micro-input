import Foundation
import Darwin

struct T3ConnectionInfo {
    let environmentID: String
    let origin: URL
    let executable: URL
    let home: URL

    static func validateOrigin(_ raw: String) throws -> URL {
        guard let components = URLComponents(string: raw), components.scheme == "http",
              ["127.0.0.1", "[::1]", "::1"].contains(components.host ?? ""),
              components.user == nil, components.password == nil, components.query == nil, components.fragment == nil,
              components.path.isEmpty || components.path == "/",
              let port = components.port, (1...65535).contains(port), let url = components.url else { throw T3Failure.discovery }
        return url
    }
    static func discover(environment: [String: String] = ProcessInfo.processInfo.environment) throws -> T3ConnectionInfo {
        let home = URL(fileURLWithPath: environment["T3CODE_HOME"] ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".t3").path)
        let candidates = [environment["MI_T3_BIN"], FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".local/bin/t3").path,
                          "/opt/homebrew/bin/t3", "/usr/local/bin/t3"].compactMap { $0 }
        guard let binary = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else { throw T3Failure.discovery }
        struct Runtime: Decodable { let version: Int; let origin: String }
        func read(_ name: String) throws -> Data {
            let url = home.appendingPathComponent("userdata").appendingPathComponent(name)
            let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
            guard (attributes[.size] as? NSNumber)?.intValue ?? Int.max < 16384 else { throw T3Failure.discovery }
            return try Data(contentsOf: url)
        }
        do {
            let runtime = try JSONDecoder().decode(Runtime.self, from: read("server-runtime.json"))
            let identity = String(decoding: try read("environment-id"), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            guard runtime.version == 1, !identity.isEmpty, identity.utf8.count <= 512,
                  !identity.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else { throw T3Failure.discovery }
            return T3ConnectionInfo(environmentID: identity, origin: try validateOrigin(runtime.origin), executable: URL(fileURLWithPath: binary), home: home)
        } catch { throw T3Failure.discovery }
    }
}

struct T3ReadSnapshot {
    let environmentID: String
    let shell: T3Shell
}
protocol T3Reading: AnyObject {
    func connect() async throws -> T3ReadSnapshot
    func read() async throws -> T3ReadSnapshot
    func close() async -> Bool
}

struct T3ProcessOutput { let status: Int32; let stdout: Data }
enum T3PrivateProcess {
    static func run(_ info: T3ConnectionInfo, _ arguments: [String]) async throws -> T3ProcessOutput {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                do { continuation.resume(returning: try runBounded(info, arguments)) }
                catch { continuation.resume(throwing: error as? T3Failure ?? T3Failure.process) }
            }
        }
    }
    private static func runBounded(_ info: T3ConnectionInfo, _ arguments: [String]) throws -> T3ProcessOutput {
        let process = Process(), output = Pipe(), errors = Pipe()
        process.executableURL = info.executable; process.arguments = arguments
        process.environment = ["HOME": FileManager.default.homeDirectoryForCurrentUser.path,
                               "PATH": "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
                               "T3CODE_HOME": info.home.path, "NO_COLOR": "1"]
        process.standardInput = FileHandle.nullDevice; process.standardOutput = output; process.standardError = errors
        try process.run()
        let descriptors = [output.fileHandleForReading.fileDescriptor, errors.fileHandleForReading.fileDescriptor]
        for descriptor in descriptors { _ = fcntl(descriptor, F_SETFL, O_NONBLOCK) }
        defer {
            try? output.fileHandleForReading.close(); try? errors.fileHandleForReading.close()
            if process.isRunning { kill(process.processIdentifier, SIGKILL) }
        }
        var stdout = Data(), counts = [0, 0], eof = [false, false]
        var bytes = [UInt8](repeating: 0, count: 8192)
        let deadline = Date().addingTimeInterval(8)
        while process.isRunning || !eof.allSatisfy({ $0 }) {
            guard Date() < deadline else { throw T3Failure.process }
            for index in 0..<2 where !eof[index] {
                let count = Darwin.read(descriptors[index], &bytes, bytes.count)
                if count == 0 { eof[index] = true }
                if count > 0 {
                    counts[index] += count
                    guard counts[index] <= 65536 else { throw T3Failure.oversized }
                    if index == 0 { stdout.append(contentsOf: bytes.prefix(count)) }
                }
            }
            usleep(1000)
        }
        return T3ProcessOutput(status: process.terminationStatus, stdout: stdout)
    }
}

final class T3NoRedirect: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) { completionHandler(nil) }
}
enum T3PrivateHTTP {
    static func read(_ request: URLRequest) async throws -> Data {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil; configuration.httpShouldSetCookies = false
        configuration.urlCredentialStorage = nil; configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = 5; configuration.timeoutIntervalForResource = 7
        let session = URLSession(configuration: configuration, delegate: T3NoRedirect(), delegateQueue: nil)
        defer { session.invalidateAndCancel() }
        do {
            let (bytes, response) = try await session.bytes(for: request)
            guard let response = response as? HTTPURLResponse else { throw T3Failure.transport }
            guard response.statusCode == 200 else { throw response.statusCode == 401 ? T3Failure.authDenied : T3Failure.transport }
            guard response.expectedContentLength <= 8 * 1024 * 1024 else { throw T3Failure.oversized }
            var data = Data()
            for try await byte in bytes {
                guard data.count < 8 * 1024 * 1024 else { throw T3Failure.oversized }
                data.append(byte)
            }
            return data
        } catch { throw error as? T3Failure ?? T3Failure.transport }
    }
}

actor T3AttentionClient: T3Reading {
    typealias ProcessRunner = (T3ConnectionInfo, [String]) async throws -> T3ProcessOutput
    typealias HTTPReader = (URLRequest) async throws -> Data
    private struct Session: Decodable {
        let sessionId: String
        let token: String
        let scopes: [String]
        let expiresAt: String
    }
    private let discovery: () throws -> T3ConnectionInfo
    private let process: ProcessRunner
    private let http: HTTPReader
    private var generation: UInt64 = 0
    private var connection: (T3ConnectionInfo, Session)?
    private var issuing = false
    private var readTask: Task<T3ReadSnapshot, Error>?
    private var readID: UInt64 = 0
    init(discovery: @escaping () throws -> T3ConnectionInfo = { try T3ConnectionInfo.discover() },
         process: @escaping ProcessRunner = T3PrivateProcess.run, http: @escaping HTTPReader = T3PrivateHTTP.read) {
        self.discovery = discovery; self.process = process; self.http = http
    }
    func connect() async throws -> T3ReadSnapshot {
        guard connection == nil, !issuing else { throw T3Failure.authDenied }
        let info = try discovery(), epoch = generation
        issuing = true
        defer { issuing = false }
        let output = try await process(info, ["auth", "session", "issue", "--scope", "orchestration:read", "--ttl", "5m", "--label", "micro-input", "--json"])
        guard let session = try? JSONDecoder().decode(Session.self, from: output.stdout) else { throw T3Failure.authDenied }
        guard output.status == 0, session.scopes == ["orchestration:read"], !session.sessionId.isEmpty,
              !session.token.isEmpty, session.token.utf8.count < 8192,
              !session.token.unicodeScalars.contains(where: { CharacterSet.whitespacesAndNewlines.contains($0) }),
              epoch == generation, !Task.isCancelled else {
            _ = await revoke(info, session)
            throw T3Failure.authDenied
        }
        connection = (info, session)
        do { return try await read() }
        catch {
            guard await close() else { throw T3Failure.revocation }
            throw error
        }
    }
    func read() async throws -> T3ReadSnapshot {
        if let readTask { return try await readTask.value }
        readID &+= 1
        let id = readID
        let task = Task { try await self.fetch() }
        readTask = task
        defer { if id == readID { readTask = nil } }
        return try await task.value
    }
    private func fetch() async throws -> T3ReadSnapshot {
        guard let (info, session) = connection else { throw T3Failure.transport }
        let epoch = generation
        guard let expiry = T3Shell.date(session.expiresAt), expiry > Date().addingTimeInterval(10) else { throw T3Failure.expired }
        let current = try discovery()
        guard current.environmentID == info.environmentID, current.origin == info.origin else { throw T3Failure.discovery }
        var request = URLRequest(url: info.origin.appendingPathComponent("api/orchestration/shell"))
        request.httpMethod = "GET"
        request.setValue("Bearer \(session.token)", forHTTPHeaderField: "Authorization")
        request.setValue("2", forHTTPHeaderField: "x-t3-orchestration-protocol")
        let data = try await http(request)
        guard epoch == generation, connection != nil, !Task.isCancelled else { throw T3Failure.transport }
        return T3ReadSnapshot(environmentID: info.environmentID, shell: try T3Shell.decode(data))
    }
    func close() async -> Bool {
        generation &+= 1
        readID &+= 1; readTask?.cancel(); readTask = nil
        guard let (info, session) = connection else { return !issuing }
        connection = nil
        return await revoke(info, session)
    }
    private func revoke(_ info: T3ConnectionInfo, _ session: Session) async -> Bool {
        guard !session.sessionId.isEmpty, session.sessionId.utf8.count <= 512, !session.sessionId.hasPrefix("-") else { return false }
        return (try? await process(info, ["auth", "session", "revoke", session.sessionId]))?.status == 0
    }
}
