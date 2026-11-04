import XCTest
import UniformTypeIdentifiers
@testable import MicroInput
import MicroInputCore

private final class DiagnosticBackend: Backend {
    let core = MicroInputCore.MicroInput()
    func loadDemo() async throws -> String { try await core.loadDemo() }
    func listDevices() async throws -> [DeviceInfo] { [] }
    func connect(id: String) async throws {}
    func disconnect() async {}
    func readConfig() async throws -> String { throw BridgeError.Failed(message: "协议未验证：UNKNOWN-04 fs schema") }
    func inputMonitoringStatus() async -> Bool { true }
}

final class DiagnosticsTests: XCTestCase {
    func testSensitiveErrorMessageIsRedactedBeforeStorageAndExport() {
        let message = DiagnosticRedactor.sanitize(#"读取 /Users/private/secret/workbench.json 失败 serial="SECRET" {"command":"rm -rf"}"#)
        XCTAssertFalse(message.contains("/Users/private/secret"))
        XCTAssertFalse(message.contains("SECRET"))
        XCTAssertFalse(message.contains("rm -rf"))
        XCTAssertTrue(message.contains("[path]"))
        XCTAssertTrue(message.contains("[redacted]"))
        XCTAssertTrue(message.contains("[redacted-payload]"))
    }

    func testPathsWithSpacesStopAtTheFollowingSentenceText() {
        let message = DiagnosticRedactor.sanitize("读取 /Users/private/Application Support/workbench.json 失败")
        XCTAssertEqual(message, "读取 [path] 失败")
    }

    @MainActor func testFailedDeviceReadProducesActionableSanitizedReport() async throws {
        let backend = DiagnosticBackend()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: backend)
        await store.loadDemo()
        await store.connect("synthetic")
        await store.readDeviceConfig()
        let report = await store.makeDiagnosticReport()

        XCTAssertEqual(report.lastFailure?.stage, "config_read")
        XCTAssertEqual(report.lastFailure?.message, "协议未验证，设备读取待确认")
        XCTAssertEqual(report.connection.phase, "connected")
        XCTAssertEqual(report.document.source, "Demo")
        XCTAssertTrue(report.statusMessage.contains("配置读取失败"))
        XCTAssertTrue(report.inputMonitorReady)
        XCTAssertNil(report.lastHostAction)

        let json = try await store.diagnosticJSON()
        let decodedReport = try JSONDecoder().decode(DiagnosticsReport.self, from: Data(json.utf8))
        XCTAssertEqual(decodedReport.lastFailure?.stage, "config_read")
        XCTAssertFalse(json.contains("协议未验证：UNKNOWN"))
        XCTAssertFalse(json.contains("/Users/"))
        XCTAssertFalse(json.contains("SECRET"))
        await store.shutdown()
    }
}
