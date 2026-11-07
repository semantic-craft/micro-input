import XCTest
@testable import MicroInput

@MainActor final class NativeOwnershipTransportTests: XCTestCase {
    func testRealUnixPeerEchoesFreshHandshakeAndDoesNotGrantRelease() async throws {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let candidateScratch = root.appendingPathComponent(".scratch")
        let socketName = "own-\(UUID().uuidString.prefix(8)).sock"
        let scratch = candidateScratch.appendingPathComponent(socketName).path.utf8.count < 100
            ? candidateScratch
            : FileManager.default.temporaryDirectory
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        let socket = scratch.appendingPathComponent(socketName)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
        process.arguments = ["-c", #"""
import socket,os,json,sys,uuid
path=sys.argv[1]
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
s.settimeout(4)
s.bind(path);os.chmod(path,0o600);s.listen(1)
try:
 c,_=s.accept();c.settimeout(2)
 with c:
  data=b''
  while b'\n' not in data and len(data)<4096:data+=c.recv(512)
  q=json.loads(data)
  r={k:q[k] for k in ('version','requestId','clientInstance','generation')}
  r.update(nativeInstance=str(uuid.uuid4()),nativeGeneration=1,state='native',reason='fixture',inputDrained=False,pttDrained=False,lightingDrained=False,capabilities=[])
  c.sendall(json.dumps(r).encode()+b'\n')
finally:
 s.close();os.unlink(path)
"""#, socket.path]
        process.standardOutput = Pipe(); process.standardError = Pipe()
        try process.run()
        defer {
            if process.isRunning { process.terminate() }
            process.waitUntilExit()
            try? FileManager.default.removeItem(at: socket)
        }
        for _ in 0..<200 where !FileManager.default.fileExists(atPath: socket.path) {
            try await Task.sleep(for: .milliseconds(10))
        }
        let controller = NativeOwnershipController(transport: nil)
        let request = controller.beginRequest(microDrained: true)
        let response = try await UnixNativeOwnershipTransport(path: socket.path).exchange(request)
        controller.receive(response, for: request)
        XCTAssertEqual(response.requestId, request.requestId)
        XCTAssertEqual(response.generation, request.generation)
        XCTAssertFalse(controller.allowsExecution)
        guard case .native = controller.state else { return XCTFail("expected active native reply") }
    }
    func testMissingPeerNeverOpensGate() async {
        let controller = NativeOwnershipController(transport: nil)
        let request = controller.beginRequest(microDrained: true)
        do {
            _ = try await UnixNativeOwnershipTransport(path: "/nonexistent/ownership.sock").exchange(request)
            XCTFail("missing endpoint accepted")
        } catch { controller.failed(request) }
        XCTAssertFalse(controller.allowsExecution)
    }
}
