import Foundation
import XCTest
@testable import MicroInput

final class AntigravityPairedTests: XCTestCase {
    enum Failure: Error { case verification }

    func testUnixSocketBridgeIsolatesSessionsDeduplicatesAndRejectsAfterClose() async throws {
        guard let path = ProcessInfo.processInfo.environment["MI_ANTIGRAVITY_PAIRED_GRANT"],
              let output = ProcessInfo.processInfo.environment["MI_ANTIGRAVITY_PAIRED_RESULT"] else {
            throw XCTSkip("Requires the isolated Antigravity hooks UDS bridge fixture")
        }
        let client = AntigravityStatusClient(grantURL: URL(fileURLWithPath: path))
        let snapshot = try await client.connect()
        XCTAssertEqual(snapshot.capabilities, .installed2_29_0)
        XCTAssertEqual(snapshot.transcriptReads, 0)
        XCTAssertEqual(snapshot.rows.count, 2)

        guard let rowA = snapshot.rows.first(where: { $0.conversationId == "conv-alpha" && $0.workspacePath.hasSuffix("/ws-alpha") }),
              let rowB = snapshot.rows.first(where: { $0.conversationId == "conv-beta" && $0.workspacePath.hasSuffix("/ws-beta") }) else {
            throw Failure.verification
        }
        XCTAssertEqual(rowA.phase, .backgroundActive)
        XCTAssertFalse(rowA.phase.isCompleted)
        XCTAssertFalse(rowA.fullyIdle)
        XCTAssertEqual(rowA.transitionCount, 3)

        XCTAssertEqual(rowB.phase, .completed)
        XCTAssertTrue(rowB.phase.isCompleted)
        XCTAssertTrue(rowB.fullyIdle)
        XCTAssertEqual(rowB.transitionCount, 2)

        let readback = try await client.read()
        XCTAssertEqual(readback.cursor, snapshot.cursor)
        XCTAssertTrue(readback.changes.isEmpty)
        XCTAssertEqual(readback.transcriptReads, 0)

        await client.close()
        var closedDenied = false
        do { _ = try await client.read() } catch { closedDenied = true }
        XCTAssertTrue(closedDenied)
        guard closedDenied else { throw Failure.verification }

        let data = try JSONSerialization.data(withJSONObject: [
            "status": "PASS",
            "transport": "real-unix-socket",
            "installed_version": snapshot.capabilities.installedVersion,
            "language_server_cl": snapshot.capabilities.languageServerCl,
            "session_count": snapshot.rows.count,
            "session_a_phase": rowA.phase.rawValue,
            "session_b_phase": rowB.phase.rawValue,
            "transcript_reads": snapshot.transcriptReads,
            "native_install": "NOT_RUN",
            "renderer_ui": "NOT_RUN",
            "physical_key": "NOT_RUN",
        ], options: [.prettyPrinted, .sortedKeys])
        try data.write(to: URL(fileURLWithPath: output))
    }
}
