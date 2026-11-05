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

        let badProjectCreate = try await client.create(AntigravityCreateIntent(
            requestId: "paired-create-bad-project",
            workspacePath: rowA.workspacePath,
            projectId: "proj-wrong",
            prompt: "Must reject mismatched projectId",
            title: "Bad Project",
            conversationId: "conv-bad-proj"
        ))
        XCTAssertEqual(badProjectCreate.status, "unconfirmed")
        XCTAssertEqual(badProjectCreate.reasonCode, "guard.projectIdMismatch")
        XCTAssertEqual(badProjectCreate.rows.count, 2)

        let createdKey = AntigravitySessionKey(
            conversationId: "conv-alpha-created",
            workspacePath: rowA.workspacePath
        )
        let createReply = try await client.create(AntigravityCreateIntent(
            requestId: "paired-create-alpha-1",
            workspacePath: rowA.workspacePath,
            projectId: "proj-alpha",
            prompt: "Create new conversation bound to proj-alpha",
            title: "Alpha Created",
            modelName: "gemini-2.5-flash",
            conversationId: createdKey.conversationId
        ))
        XCTAssertEqual(createReply.status, "created")
        XCTAssertEqual(createReply.projectId, "proj-alpha")
        XCTAssertEqual(createReply.selected, createdKey)
        XCTAssertEqual(createReply.rows.count, 3)

        let dupCreateReply = try await client.create(AntigravityCreateIntent(
            requestId: "paired-create-alpha-1",
            workspacePath: rowA.workspacePath,
            projectId: "proj-alpha",
            prompt: "Duplicate create request must not create another conversation",
            title: "Alpha Created",
            modelName: "gemini-2.5-flash",
            conversationId: createdKey.conversationId
        ))
        XCTAssertEqual(dupCreateReply.status, "created")
        XCTAssertTrue(dupCreateReply.duplicate)
        XCTAssertEqual(dupCreateReply.rows.count, 3)

        _ = try await client.open(rowB.key)
        let unselectedSend = try await client.send(AntigravitySendIntent(
            requestId: "paired-send-unselected-a",
            target: rowA.key,
            projectId: "proj-alpha",
            message: "Must not modify unselected session A",
            deliveryMode: .auto
        ))
        XCTAssertEqual(unselectedSend.status, "unconfirmed")
        XCTAssertEqual(unselectedSend.reasonCode, "guard.selectionMismatch")
        XCTAssertEqual(unselectedSend.selected, rowB.key)

        let missingTargetSend = try await client.send(AntigravitySendIntent(
            requestId: "paired-send-missing",
            target: AntigravitySessionKey(conversationId: "conv-nonexistent", workspacePath: rowB.workspacePath),
            projectId: "proj-beta",
            message: "Must not redirect or create session",
            deliveryMode: .auto
        ))
        XCTAssertEqual(missingTargetSend.status, "unconfirmed")
        XCTAssertEqual(missingTargetSend.reasonCode, "guard.targetNotFound")
        XCTAssertEqual(missingTargetSend.selected, rowB.key)
        XCTAssertEqual(missingTargetSend.rows.count, 3)

        let sendIdleB = try await client.send(AntigravitySendIntent(
            requestId: "paired-send-b-start",
            target: rowB.key,
            projectId: "proj-beta",
            message: "Start session B",
            deliveryMode: .auto
        ))
        XCTAssertEqual(sendIdleB.status, "started")
        XCTAssertEqual(sendIdleB.delivery, .started)
        XCTAssertFalse(sendIdleB.inFlightSteering)
        XCTAssertEqual(sendIdleB.rows.first(where: { $0.key == rowB.key })?.phase, .running)
        XCTAssertEqual(sendIdleB.rows.first(where: { $0.key == rowA.key })?.transitionCount, 3)
        XCTAssertEqual(sendIdleB.rows.first(where: { $0.key == createdKey })?.transitionCount, 1)

        let sendRunningB = try await client.send(AntigravitySendIntent(
            requestId: "paired-send-b-running-default",
            target: rowB.key,
            projectId: "proj-beta",
            message: "Follow-up while B is running",
            deliveryMode: .steer
        ))
        XCTAssertEqual(sendRunningB.status, "queued")
        XCTAssertEqual(sendRunningB.delivery, .nextInvocation)
        XCTAssertFalse(sendRunningB.inFlightSteering)
        XCTAssertEqual(sendRunningB.reasonCode, "agentapi.steerUnsupportedQueuedNextInvocation")
        XCTAssertEqual(sendRunningB.rows.count, 3)

        let queryReconciled = try await client.query(AntigravityQueryIntent(
            requestId: "paired-send-b-running-default",
            expectedOp: "send",
            conversationId: rowB.conversationId,
            workspacePath: rowB.workspacePath,
            projectId: "proj-beta"
        ))
        XCTAssertEqual(queryReconciled.status, "queued")
        XCTAssertTrue(queryReconciled.reconciled)
        XCTAssertTrue(queryReconciled.duplicate)

        let unselectedStopA = try await client.stop(AntigravityStopIntent(
            requestId: "paired-stop-unselected-a",
            target: rowA.key,
            expectedInvocationNum: 1
        ))
        XCTAssertEqual(unselectedStopA.status, "unconfirmed")
        XCTAssertEqual(unselectedStopA.reasonCode, "guard.selectionMismatch")

        let staleStopB = try await client.stop(AntigravityStopIntent(
            requestId: "paired-stop-stale-b",
            target: rowB.key,
            expectedInvocationNum: 99,
            expectedExecutionNum: 1
        ))
        XCTAssertEqual(staleStopB.status, "noop")
        XCTAssertEqual(staleStopB.reasonCode, "guard.stopTargetChanged")
        XCTAssertEqual(staleStopB.rows.first(where: { $0.key == rowB.key })?.phase, .running)

        let validStopB = try await client.stop(AntigravityStopIntent(
            requestId: "paired-stop-valid-b",
            target: rowB.key,
            expectedInvocationNum: 1,
            expectedExecutionNum: 1
        ))
        XCTAssertEqual(validStopB.status, "stopped")
        XCTAssertEqual(validStopB.delivery, .canceled)
        XCTAssertEqual(validStopB.rows.first(where: { $0.key == rowB.key })?.phase, .canceled)
        XCTAssertEqual(validStopB.rows.first(where: { $0.key == rowB.key })?.terminationReason, "USER_CANCELED")
        XCTAssertEqual(validStopB.rows.first(where: { $0.key == rowA.key })?.phase, .backgroundActive)

        let unsupportedModelB = try await client.switchModel(AntigravitySwitchModelIntent(
            requestId: "paired-model-unsupported-b",
            target: rowB.key,
            modelName: "gemini-2.5-flash-lite",
            thinkingLevel: "high"
        ))
        XCTAssertEqual(unsupportedModelB.status, "unconfirmed")
        XCTAssertEqual(unsupportedModelB.reasonCode, "guard.unsupportedThinkingLevel")

        let validModelB = try await client.switchModel(AntigravitySwitchModelIntent(
            requestId: "paired-model-switch-b",
            target: rowB.key,
            modelName: "gemini-2.5-pro",
            thinkingLevel: "medium"
        ))
        XCTAssertEqual(validModelB.status, "modelSwitched")
        XCTAssertEqual(validModelB.delivery, .modelSwitched)
        XCTAssertEqual(validModelB.rows.first(where: { $0.key == rowB.key })?.modelName, "gemini-2.5-pro")
        XCTAssertEqual(validModelB.rows.first(where: { $0.key == rowB.key })?.thinkingLevel, "medium")
        XCTAssertEqual(validModelB.rows.first(where: { $0.key == rowA.key })?.modelName, "gemini-2.5-pro")
        XCTAssertEqual(validModelB.rows.first(where: { $0.key == rowA.key })?.thinkingLevel, "high")

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
            "create_project_bound_confirmed": createReply.status == "created" && createReply.projectId == "proj-alpha",
            "two_sessions_only_target_modified": unselectedSend.status == "unconfirmed" && sendIdleB.status == "started",
            "missing_target_zero_redirect": missingTargetSend.status == "unconfirmed" && missingTargetSend.rows.count == 3,
            "running_send_queued_next_invocation": sendRunningB.status == "queued" && sendRunningB.delivery == .nextInvocation && !sendRunningB.inFlightSteering,
            "query_reconciled_without_resend": queryReconciled.reconciled && queryReconciled.duplicate,
            "stop_isolated_and_stale_noop_confirmed": unselectedStopA.status == "unconfirmed" && staleStopB.status == "noop" && validStopB.status == "stopped",
            "model_switch_and_thinking_guard_confirmed": unsupportedModelB.status == "unconfirmed" && validModelB.status == "modelSwitched",
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
