import Foundation
import XCTest
@testable import MicroInput

final class DSHPairedTests: XCTestCase {
    enum Failure: Error { case verification }

    func testNativeSourceSocketSelectsExactTargetsWithoutStartingColdSessions() async throws {
        guard let path = ProcessInfo.processInfo.environment["MI_DSH_PAIRED_GRANT"],
              let output = ProcessInfo.processInfo.environment["MI_DSH_PAIRED_RESULT"] else {
            throw XCTSkip("Requires the isolated DSH native source fixture")
        }
        let client = DSHSessionClient(grantURL: URL(fileURLWithPath: path))
        let snapshot = try await client.connect()
        XCTAssertEqual(snapshot.rows.count, 2)
        XCTAssertEqual(Set(snapshot.rows.map(\.title)).count, 1)
        XCTAssertEqual(Set(snapshot.rows.map(\.workspaceId)).count, 2)
        XCTAssertEqual(snapshot.unavailableSessionIds.count, 1)
        guard snapshot.rows.count == 2, Set(snapshot.rows.map(\.title)).count == 1,
              Set(snapshot.rows.map(\.workspaceId)).count == 2,
              snapshot.unavailableSessionIds.count == 1 else { throw Failure.verification }
        var selected: [String] = []
        for row in snapshot.rows.reversed() {
            let target = DSHSelection(sessionId: row.sessionId, workspaceId: row.workspaceId,
                hostId: snapshot.hostId, generation: snapshot.generation, localEpoch: 0)
            let reply = try await client.open(target)
            let readback = try await client.read()
            XCTAssertEqual(reply.status, .confirmed)
            XCTAssertEqual(reply.selected, row.sessionId)
            XCTAssertEqual(readback.hostId, snapshot.hostId)
            guard reply.status == .confirmed, reply.selected == row.sessionId,
                  readback.hostId == snapshot.hostId,
                  readback.rows.contains(where: { $0.sessionId == row.sessionId }) else { throw Failure.verification }
            selected.append(row.sessionId)
        }
        await client.close()
        var closedDenied = false
        do { _ = try await client.read() } catch { closedDenied = true }
        XCTAssertTrue(closedDenied)
        guard closedDenied, selected.count == 2, Set(selected).count == 2 else { throw Failure.verification }
        let data = try JSONSerialization.data(withJSONObject: [
            "status": "PASS", "transport": "real-unix-socket", "confirmed_targets": selected.count,
            "cold_targets_unavailable": snapshot.unavailableSessionIds.count,
            "native_install": "NOT_RUN", "renderer_ui": "NOT_RUN", "physical_key": "NOT_RUN",
        ], options: [.prettyPrinted, .sortedKeys])
        try data.write(to: URL(fileURLWithPath: output))
    }
}
