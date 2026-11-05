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
        let firstRow = snapshot.rows[0]
        let secondRow = snapshot.rows[1]
        let steerIntent = DSHPromptIntent(
            sessionId: firstRow.sessionId, workspaceId: firstRow.workspaceId,
            requestId: "paired-steer-1", mode: .steer, text: "steer active turn",
            hostId: snapshot.hostId, generation: snapshot.generation, localEpoch: 0
        )
        let steered = try await client.prompt(steerIntent)
        let duplicateSteer = try await client.prompt(steerIntent)
        let queueIntent = DSHPromptIntent(
            sessionId: firstRow.sessionId, workspaceId: firstRow.workspaceId,
            requestId: "paired-queue-1", mode: .queue, text: "queue next turn",
            hostId: snapshot.hostId, generation: snapshot.generation, localEpoch: 0
        )
        let queued = try await client.prompt(queueIntent)
        guard steered.status == "steered", steered.delivery == .steer, steered.currentTurn == 1, steered.queuedCount == 1, !steered.duplicate,
              duplicateSteer.status == "steered", duplicateSteer.duplicate,
              queued.status == "queued", queued.delivery == .queue, queued.currentTurn == 1, queued.queuedCount == 2, !queued.duplicate else {
            throw Failure.verification
        }
        // Switch to secondRow and verify cancelling firstRow does not mis-stop
        _ = try await client.open(DSHSelection(
            sessionId: secondRow.sessionId, workspaceId: secondRow.workspaceId,
            hostId: snapshot.hostId, generation: snapshot.generation, localEpoch: 0
        ))
        let wrongSessionCancel = try await client.cancel(DSHCancelIntent(
            sessionId: firstRow.sessionId, workspaceId: firstRow.workspaceId,
            requestId: "paired-cancel-wrong", expectedTurn: 1,
            hostId: snapshot.hostId, generation: snapshot.generation, localEpoch: 0
        ))
        guard wrongSessionCancel.status == "unconfirmed", wrongSessionCancel.reasonCode == "guard.selectionMismatch" else {
            throw Failure.verification
        }
        // Switch back to firstRow; verify mismatched expectedTurn=99 does zero cancellation, then expectedTurn=1 returns cancelledQueueRemaining
        _ = try await client.open(DSHSelection(
            sessionId: firstRow.sessionId, workspaceId: firstRow.workspaceId,
            hostId: snapshot.hostId, generation: snapshot.generation, localEpoch: 0
        ))
        let staleCancel = try await client.cancel(DSHCancelIntent(
            sessionId: firstRow.sessionId, workspaceId: firstRow.workspaceId,
            requestId: "paired-cancel-stale", expectedTurn: 99,
            hostId: snapshot.hostId, generation: snapshot.generation, localEpoch: 0
        ))
        let activeCancel = try await client.cancel(DSHCancelIntent(
            sessionId: firstRow.sessionId, workspaceId: firstRow.workspaceId,
            requestId: "paired-cancel-active", expectedTurn: 1,
            hostId: snapshot.hostId, generation: snapshot.generation, localEpoch: 0
        ))
        guard staleCancel.status == "noop", staleCancel.reasonCode == "guard.turnChanged", staleCancel.currentTurn == 1,
              activeCancel.status == "cancelledQueueRemaining", activeCancel.expectedTurn == 1,
              activeCancel.currentTurn == nil, activeCancel.queuedCount == 2 else {
            throw Failure.verification
        }
        // Create a workspace-bound session and verify selected readback
        let created = try await client.create(DSHCreateIntent(
            workspaceId: firstRow.workspaceId, requestId: "paired-create-1",
            hostId: snapshot.hostId, generation: snapshot.generation, localEpoch: 0
        ))
        let createdReadback = try await client.read()
        guard created.status == "confirmed", let createdId = created.sessionId,
              created.selected == createdId, created.workspaceId == firstRow.workspaceId,
              createdReadback.selected == createdId,
              createdReadback.rows.contains(where: { $0.sessionId == createdId && $0.workspaceId == firstRow.workspaceId }) else {
            throw Failure.verification
        }
        await client.close()
        var closedDenied = false
        do { _ = try await client.read() } catch { closedDenied = true }
        XCTAssertTrue(closedDenied)
        guard closedDenied, selected.count == 2, Set(selected).count == 2 else { throw Failure.verification }
        let data = try JSONSerialization.data(withJSONObject: [
            "status": "PASS", "transport": "real-unix-socket", "confirmed_targets": selected.count,
            "created_session": createdId, "prompt_modes_confirmed": ["steer", "queue"],
            "cancel_status": activeCancel.status,
            "cold_targets_unavailable": snapshot.unavailableSessionIds.count,
            "native_install": "NOT_RUN", "renderer_ui": "NOT_RUN", "physical_key": "NOT_RUN",
        ], options: [.prettyPrinted, .sortedKeys])
        try data.write(to: URL(fileURLWithPath: output))
    }
}
