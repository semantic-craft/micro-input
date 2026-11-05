import Foundation
import XCTest
@testable import MicroInput

final class CindyPairedTests: XCTestCase {
    enum Failure: Error { case verification }

    private func sendControlCommand(_ command: [String: Any], controlPath: String) async throws {
        let data = try JSONSerialization.data(withJSONObject: command)
        let transport = UnixNativeOwnershipTransport(
            path: controlPath,
            requestLimit: 4096,
            responseLimit: 4096,
            timeout: .seconds(2)
        )
        _ = try await transport.exchangeJSON(data)
    }

    func testNativeSourceSocketSeparatesDeliveredAndConfirmedAcrossSameNameTargets() async throws {
        guard let grantPath = ProcessInfo.processInfo.environment["MI_CINDY_PAIRED_GRANT"],
              let outputPath = ProcessInfo.processInfo.environment["MI_CINDY_PAIRED_RESULT"] else {
            throw XCTSkip("Requires the isolated Cindy native navigation fixture")
        }
        let controlPath = (grantPath as NSString)
            .deletingLastPathComponent
            .appending("/cindy.control")

        let client = CindySessionClient(grantPath: grantPath)
        guard case .success(let snapshot) = await client.connect() else {
            throw Failure.verification
        }

        XCTAssertEqual(snapshot.rows.count, 3)
        XCTAssertEqual(Set(snapshot.rows.map(\.title)), ["Duplicate Task Title"])
        XCTAssertEqual(
            Set(snapshot.rows.map { $0.address.workspacePath }),
            ["/Users/test/CindyAlpha", "/Users/test/CindyBeta"]
        )
        XCTAssertFalse(snapshot.rows.contains { $0.address.workspacePath == "/Users/test/CindySecret" })

        let alpha1 = CindyTaskAddress(workspacePath: "/Users/test/CindyAlpha", sessionId: "cindy-alpha-1")
        let alpha2 = CindyTaskAddress(workspacePath: "/Users/test/CindyAlpha", sessionId: "cindy-alpha-2")
        let beta1 = CindyTaskAddress(workspacePath: "/Users/test/CindyBeta", sessionId: "cindy-beta-1")

        try await sendControlCommand(["cmd": "set-mode", "mode": "delivered"], controlPath: controlPath)
        let alpha2Selection = CindySelection(
            address: alpha2,
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            ownerEpoch: snapshot.ownerEpoch,
            localEpoch: 1
        )
        let firstReply = await client.open(alpha2Selection)
        XCTAssertEqual(firstReply.status, .delivered)
        XCTAssertEqual(firstReply.delivered, alpha2)
        XCTAssertNil(firstReply.selected)
        XCTAssertEqual(firstReply.deepLink, "cindy://session/cindy-alpha-2")

        try await sendControlCommand(["cmd": "set-mode", "mode": "confirmed"], controlPath: controlPath)
        let secondReply = await client.open(alpha2Selection)
        XCTAssertEqual(secondReply.status, .confirmed)
        XCTAssertEqual(secondReply.delivered, alpha2)
        XCTAssertEqual(secondReply.selected, alpha2)
        XCTAssertEqual(secondReply.deepLink, "cindy://session/cindy-alpha-2")

        guard case .success(let readbackAlpha2) = await client.read() else {
            throw Failure.verification
        }
        XCTAssertEqual(readbackAlpha2.selected, alpha2)

        // Verify resolve guards and exact request-bound resolution on alpha2 over real UDS.
        let questionAsPermReply = await client.resolve(
            CindyResolveIntent(
                commandId: "paired-resolve-q-guard",
                selection: alpha2Selection,
                interactionId: "perm-cindy-alpha-2",
                interactionType: .question,
                decision: .approve,
                optionId: nil
            )
        )
        XCTAssertEqual(questionAsPermReply.status, .interactionTypeMismatch)
        XCTAssertEqual(questionAsPermReply.reasonCode, "guard.questionCannotApproveAsPermission")

        let expiredResolveReply = await client.resolve(
            CindyResolveIntent(
                commandId: "paired-resolve-expired",
                selection: alpha2Selection,
                interactionId: "perm-expired-0",
                interactionType: .permission,
                decision: .approve,
                optionId: nil
            )
        )
        XCTAssertEqual(expiredResolveReply.status, .noop)
        XCTAssertEqual(expiredResolveReply.reasonCode, "guard.interactionAlreadyResolved")

        let exactResolveReply = await client.resolve(
            CindyResolveIntent(
                commandId: "paired-resolve-exact",
                selection: alpha2Selection,
                interactionId: "perm-cindy-alpha-2",
                interactionType: .permission,
                decision: .approve,
                optionId: nil
            )
        )
        XCTAssertEqual(exactResolveReply.status, .resolved)
        XCTAssertEqual(exactResolveReply.interactionId, "perm-cindy-alpha-2")

        let beta1Selection = CindySelection(
            address: beta1,
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            ownerEpoch: snapshot.ownerEpoch,
            localEpoch: 1
        )
        let betaReply = await client.open(beta1Selection)
        XCTAssertEqual(betaReply.status, .confirmed)
        XCTAssertEqual(betaReply.delivered, beta1)
        XCTAssertEqual(betaReply.selected, beta1)
        XCTAssertEqual(betaReply.deepLink, "cindy://session/cindy-beta-1")

        guard case .success(let readbackBeta1) = await client.read() else {
            throw Failure.verification
        }
        XCTAssertEqual(readbackBeta1.selected, beta1)

        // Verify send (started/steered/queued), query reconciliation, and run-bound stop on beta1 over real UDS.
        let startedReply = await client.send(
            CindySendIntent(
                commandId: "paired-send-start",
                selection: beta1Selection,
                text: "Start turn on beta1",
                deliveryMode: .auto
            )
        )
        XCTAssertEqual(startedReply.status, .started)
        let startedRunId = try XCTUnwrap(startedReply.runId)

        let queriedReply = await client.query(
            CindyQueryIntent(
                commandId: "paired-send-start",
                scope: CindyScope(workspacePath: "/Users/test/CindyBeta"),
                target: beta1,
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                localEpoch: 1
            )
        )
        XCTAssertEqual(queriedReply.status, .started)
        XCTAssertTrue(queriedReply.duplicate)
        XCTAssertEqual(queriedReply.runId, startedRunId)

        let steeredReply = await client.send(
            CindySendIntent(
                commandId: "paired-send-steer",
                selection: beta1Selection,
                text: "Steer beta1 active turn",
                deliveryMode: .steer
            )
        )
        XCTAssertEqual(steeredReply.status, .steered)
        XCTAssertEqual(steeredReply.runId, startedRunId)

        let staleStopReply = await client.stop(
            CindyStopIntent(
                commandId: "paired-stop-stale",
                selection: beta1Selection,
                expectedRunId: "run-wrong-0"
            )
        )
        XCTAssertEqual(staleStopReply.status, .noop)
        XCTAssertEqual(staleStopReply.reasonCode, "guard.stopTargetChanged")

        let exactStopReply = await client.stop(
            CindyStopIntent(
                commandId: "paired-stop-exact",
                selection: beta1Selection,
                expectedRunId: startedRunId
            )
        )
        XCTAssertEqual(exactStopReply.status, .stopped)
        XCTAssertEqual(exactStopReply.runId, startedRunId)

        try await sendControlCommand(
            ["cmd": "remove-task", "workspacePath": "/Users/test/CindyAlpha", "sessionId": "cindy-alpha-2"],
            controlPath: controlPath
        )
        let deletedReply = await client.open(alpha2Selection)
        XCTAssertEqual(deletedReply.status, .unavailable)
        XCTAssertNil(deletedReply.delivered)
        XCTAssertNil(deletedReply.selected)

        guard case .success(let afterDeleteSnapshot) = await client.connect() else {
            throw Failure.verification
        }
        XCTAssertFalse(afterDeleteSnapshot.rows.contains { $0.address == alpha2 })
        XCTAssertTrue(afterDeleteSnapshot.rows.contains { $0.address == alpha1 })

        try await sendControlCommand(["cmd": "rotate-account"], controlPath: controlPath)
        let staleAfterRotate = await client.read()
        XCTAssertEqual(staleAfterRotate, .failure(.stale))

        guard case .success(let rotatedSnapshot) = await client.connect() else {
            throw Failure.verification
        }
        XCTAssertGreaterThan(rotatedSnapshot.generation, snapshot.generation)
        XCTAssertNotEqual(rotatedSnapshot.ownerEpoch, snapshot.ownerEpoch)

        await client.close()

        let data = try JSONSerialization.data(
            withJSONObject: [
                "status": "PASS",
                "transport": "real-unix-socket",
                "delivered_target": alpha2.id,
                "confirmed_targets": [alpha2.id, beta1.id],
                "deep_link": "cindy://session/cindy-alpha-2",
                "local_agent_api_relaxed": false,
                "native_install": "NOT_RUN",
                "physical_key": "NOT_RUN",
            ],
            options: [.prettyPrinted, .sortedKeys]
        )
        try data.write(to: URL(fileURLWithPath: outputPath), options: .atomic)
    }
}
