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
        XCTAssertEqual(snapshot.tokenReads, 0)
        XCTAssertEqual(snapshot.rows.count, 2)

        guard let rowA = snapshot.rows.first(where: { $0.conversationId == "conv-alpha" && $0.workspacePath.hasSuffix("/ws-alpha") }),
              let rowB = snapshot.rows.first(where: { $0.conversationId == "conv-beta" && $0.workspacePath.hasSuffix("/ws-beta") }) else {
            throw Failure.verification
        }
        XCTAssertEqual(rowA.title, "Shared Investigation")
        XCTAssertEqual(rowB.title, "Shared Investigation")
        XCTAssertEqual(rowA.projectId, "proj-alpha")
        XCTAssertEqual(rowB.projectId, "proj-beta")
        XCTAssertEqual(rowA.phase, .backgroundActive)
        XCTAssertFalse(rowA.phase.isCompleted)
        XCTAssertFalse(rowA.fullyIdle)
        XCTAssertEqual(rowA.transitionCount, 3)

        XCTAssertEqual(rowB.phase, .completed)
        XCTAssertTrue(rowB.phase.isCompleted)
        XCTAssertTrue(rowB.fullyIdle)
        XCTAssertEqual(rowB.transitionCount, 2)

        let openA = try await client.open(rowA.key)
        XCTAssertEqual(openA.status, "ok")
        XCTAssertEqual(openA.selected, rowA.key)
        XCTAssertEqual(openA.transcriptReads, 0)
        XCTAssertEqual(openA.tokenReads, 0)

        let readAfterA = try await client.read()
        XCTAssertEqual(readAfterA.selected, rowA.key)
        XCTAssertEqual(readAfterA.transcriptReads, 0)
        XCTAssertEqual(readAfterA.tokenReads, 0)

        let openB = try await client.open(rowB.key)
        XCTAssertEqual(openB.status, "ok")
        XCTAssertEqual(openB.selected, rowB.key)
        XCTAssertEqual(openB.transcriptReads, 0)
        XCTAssertEqual(openB.tokenReads, 0)

        let readback = try await client.read()
        XCTAssertEqual(readback.cursor, snapshot.cursor)
        XCTAssertTrue(readback.changes.isEmpty)
        XCTAssertEqual(readback.selected, rowB.key)
        XCTAssertEqual(readback.transcriptReads, 0)
        XCTAssertEqual(readback.tokenReads, 0)

        await client.close()
        var closedDenied = false
        do { _ = try await client.read() } catch { closedDenied = true }
        XCTAssertTrue(closedDenied)
        var openAfterCloseDenied = false
        do { _ = try await client.open(rowA.key) } catch { openAfterCloseDenied = true }
        XCTAssertTrue(openAfterCloseDenied)
        guard closedDenied && openAfterCloseDenied else { throw Failure.verification }

        let data = try JSONSerialization.data(withJSONObject: [
            "status": "PASS",
            "transport": "real-unix-socket",
            "installed_version": snapshot.capabilities.installedVersion,
            "language_server_cl": snapshot.capabilities.languageServerCl,
            "session_count": snapshot.rows.count,
            "same_title_isolated": rowA.title == rowB.title && rowA.key != rowB.key,
            "open_a_confirmed": openA.selected == rowA.key,
            "open_b_confirmed": openB.selected == rowB.key,
            "session_a_phase": rowA.phase.rawValue,
            "session_b_phase": rowB.phase.rawValue,
            "transcript_reads": snapshot.transcriptReads,
            "token_reads": snapshot.tokenReads,
            "native_install": "NOT_RUN",
            "renderer_ui": "NOT_RUN",
            "physical_key": "NOT_RUN",
        ], options: [.prettyPrinted, .sortedKeys])
        try data.write(to: URL(fileURLWithPath: output))
    }
}
