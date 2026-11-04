import Foundation
import Darwin

struct UnixNativeOwnershipTransport: NativeOwnershipTransport {
    var path = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/MicroInput/cindy-ownership.sock").path

    func exchange(_ request: NativeOwnershipRequest) async throws -> NativeOwnershipReply {
        let data = try await exchangeJSON(JSONEncoder().encode(request))
        return try JSONDecoder().decode(NativeOwnershipReply.self, from: data)
    }
    func exchangeJSON(_ request: Data) async throws -> Data {
        let path = self.path
        return try await Task.detached {
            try Self.exchangeBlocking(request, path: path)
        }.value
    }
    enum Failure: Error { case unavailable, invalidPeer, invalidReply }

    private static func exchangeBlocking(_ request: Data, path: String) throws -> Data {
        var metadata = stat()
        guard lstat(path, &metadata) == 0, metadata.st_uid == getuid(),
              metadata.st_mode & S_IFMT == S_IFSOCK, metadata.st_mode & 0o077 == 0 else { throw Failure.invalidPeer }
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw Failure.unavailable }
        defer { close(fd) }
        var noSignal: Int32 = 1
        guard setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, socklen_t(MemoryLayout.size(ofValue: noSignal))) == 0,
              fcntl(fd, F_SETFL, O_NONBLOCK) == 0 else { throw Failure.unavailable }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8) + [0]
        guard bytes.count <= MemoryLayout.size(ofValue: address.sun_path) else { throw Failure.unavailable }
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: bytes) }
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        let result = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        let deadline = ContinuousClock.now.advanced(by: .milliseconds(750))
        if result != 0 {
            guard errno == EINPROGRESS else { throw Failure.unavailable }
            try wait(fd, for: Int16(POLLOUT), deadline: deadline)
            var error: Int32 = 0
            var size = socklen_t(MemoryLayout.size(ofValue: error))
            guard getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &size) == 0, error == 0 else { throw Failure.unavailable }
        }
        var uid: uid_t = 0
        var gid: gid_t = 0
        guard getpeereid(fd, &uid, &gid) == 0, uid == getuid() else { throw Failure.invalidPeer }
        guard request.count <= 4096 else { throw Failure.invalidReply }
        var data = request
        data.append(0x0a)
        var written = 0
        while written < data.count {
            try wait(fd, for: Int16(POLLOUT), deadline: deadline)
            let count = data.withUnsafeBytes { send(fd, $0.baseAddress!.advanced(by: written), data.count - written, 0) }
            if count < 0, errno == EAGAIN || errno == EINTR { continue }
            guard count > 0 else { throw Failure.unavailable }
            written += count
        }
        var response = Data()
        var buffer = [UInt8](repeating: 0, count: 1024)
        while response.count <= 4096 {
            try wait(fd, for: Int16(POLLIN), deadline: deadline)
            let count = recv(fd, &buffer, buffer.count, 0)
            if count < 0, errno == EAGAIN || errno == EINTR { continue }
            guard count > 0 else { throw Failure.unavailable }
            response.append(contentsOf: buffer.prefix(count))
            if let newline = response.firstIndex(of: 0x0a) {
                guard newline <= 4096, response.count == newline + 1 else { throw Failure.invalidReply }
                return Data(response.prefix(upTo: newline))
            }
        }
        throw Failure.invalidReply
    }
    private static func wait(_ fd: Int32, for events: Int16, deadline: ContinuousClock.Instant) throws {
        while ContinuousClock.now < deadline {
            try Task.checkCancellation()
            var descriptor = pollfd(fd: fd, events: events, revents: 0)
            let result = poll(&descriptor, 1, 25)
            if result < 0, errno == EINTR { continue }
            guard result >= 0 else { throw Failure.unavailable }
            if descriptor.revents & events != 0 { return }
            if descriptor.revents & Int16(POLLERR | POLLHUP | POLLNVAL) != 0 { throw Failure.unavailable }
        }
        throw Failure.unavailable
    }
}
