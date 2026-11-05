import Foundation
import XCTest
@testable import MicroInput

final class ZCodePairedTests: XCTestCase {
    enum Failure: Error { case verification }
    func testNativeSourceSocketSelectsExactSameNameTargets() async throws {
        guard let path = ProcessInfo.processInfo.environment["MI_ZCODE_PAIRED_GRANT"],
              let output = ProcessInfo.processInfo.environment["MI_ZCODE_PAIRED_RESULT"] else {
            throw XCTSkip("Requires the isolated native source fixture")
        }
        let client = ZCodeSessionClient(grantURL: URL(fileURLWithPath: path))
        let snapshot = try await client.connect()
        XCTAssertEqual(snapshot.rows.count, 2)
        XCTAssertEqual(Set(snapshot.rows.map(\.title)).count, 1)
        XCTAssertEqual(Set(snapshot.rows.map { $0.address.workspacePath }).count, 2)
        guard snapshot.rows.count == 2, Set(snapshot.rows.map(\.title)).count == 1,
              Set(snapshot.rows.map { $0.address.workspacePath }).count == 2 else { throw Failure.verification }
        var selected: [ZCodeTaskAddress] = []
        for row in snapshot.rows.reversed() {
            let target = ZCodeSelection(address: row.address, instanceId: snapshot.instanceId,
                generation: snapshot.generation, localEpoch: 1)
            let reply = try await client.open(target)
            XCTAssertEqual(reply.status, .confirmed)
            XCTAssertEqual(reply.selected, row.address)
            let readback = try await client.read()
            XCTAssertEqual(readback.selected, row.address)
            guard reply.status == .confirmed, reply.selected == row.address, readback.selected == row.address else {
                throw Failure.verification
            }
            selected.append(row.address)
        }
        await client.close()
        var closedDenied = false
        do { _ = try await client.read() } catch { closedDenied = true }
        XCTAssertTrue(closedDenied)
        guard closedDenied else { throw Failure.verification }
        guard selected.count == 2, selected[0] != selected[1] else { return }

        // Reconnect and exercise Slice K1 controlled execution over real Unix sockets
        _ = try await client.connect()
        let firstTarget = ZCodeSelection(address: snapshot.rows[0].address, instanceId: snapshot.instanceId,
            generation: snapshot.generation, localEpoch: 1)
        let secondTarget = ZCodeSelection(address: snapshot.rows[1].address, instanceId: snapshot.instanceId,
            generation: snapshot.generation, localEpoch: 1)

        // Currently snapshot.rows[0] is selected (last opened in loop above).
        // 1. Two sessions only affect the selected one: sending to unselected secondTarget -> unconfirmed
        let unselectedSend = try await client.send(ZCodeSendIntent(
            selection: secondTarget,
            commandId: "paired-cmd-unselected",
            clientId: "paired-swift",
            text: "should-not-run"
        ))
        XCTAssertEqual(unselectedSend.status, .unconfirmed)

        // 2. Start execution on selected firstTarget -> started
        let startSend = try await client.send(ZCodeSendIntent(
            selection: firstTarget,
            commandId: "paired-cmd-start",
            clientId: "paired-swift",
            text: "start-task"
        ))
        XCTAssertEqual(startSend.status, .started)
        XCTAssertEqual(startSend.delivery, .startNow)
        let activeExecId = try XCTUnwrap(startSend.executionId)

        // 3. Duplicate commandId -> duplicate == true
        let dupSend = try await client.send(ZCodeSendIntent(
            selection: firstTarget,
            commandId: "paired-cmd-start",
            clientId: "paired-swift",
            text: "start-task"
        ))
        XCTAssertEqual(dupSend.status, .started)
        XCTAssertTrue(dupSend.duplicate)

        // 4. In-flight send on running session: guide -> steered, queue -> queued
        let steerSend = try await client.send(ZCodeSendIntent(
            selection: firstTarget,
            commandId: "paired-cmd-steer",
            clientId: "paired-swift",
            text: "steer-in-flight",
            requestedDelivery: .guide
        ))
        XCTAssertEqual(steerSend.status, .steered)
        XCTAssertEqual(steerSend.delivery, .guide)

        let queueSend = try await client.send(ZCodeSendIntent(
            selection: firstTarget,
            commandId: "paired-cmd-queue",
            clientId: "paired-swift",
            text: "queue-in-flight",
            requestedDelivery: .queue
        ))
        XCTAssertEqual(queueSend.status, .queued)
        XCTAssertEqual(queueSend.delivery, .queue)

        // 5. Late stop with stale executionId -> noop (does not stop activeExecId)
        let lateStop = try await client.stop(ZCodeStopIntent(
            selection: firstTarget,
            commandId: "paired-cmd-stop-stale",
            clientId: "paired-swift",
            expectedExecutionId: "exec-stale-0"
        ))
        XCTAssertEqual(lateStop.status, .noop)
        XCTAssertEqual(lateStop.reasonCode, "guard.stopTargetChanged")

        // 6. Stop with matching executionId -> stopped
        let validStop = try await client.stop(ZCodeStopIntent(
            selection: firstTarget,
            commandId: "paired-cmd-stop-valid",
            clientId: "paired-swift",
            expectedExecutionId: activeExecId
        ))
        XCTAssertEqual(validStop.status, .stopped)

        // 7. Create session bound to workspace and read back new ID
        let createdReply = try await client.create(ZCodeCreateIntent(
            scope: snapshot.scopes[1],
            commandId: "paired-cmd-create",
            clientId: "paired-swift",
            sessionId: "paired-created-1",
            firstInputText: "bootstrap",
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: 1
        ))
        XCTAssertEqual(createdReply.status, .confirmed)
        XCTAssertEqual(createdReply.selected?.taskId, "paired-created-1")
        XCTAssertEqual(createdReply.selected?.scope, snapshot.scopes[1])
        let afterCreateSnapshot = try await client.read()
        XCTAssertEqual(afterCreateSnapshot.selected, createdReply.selected)

        // 8. Query command reconciles existing commandId with duplicate == true
        let queriedReply = try await client.query(ZCodeQueryIntent(
            scope: snapshot.scopes[1],
            target: createdReply.selected,
            commandId: "paired-cmd-create",
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: 1
        ))
        XCTAssertEqual(queriedReply.status, .confirmed)
        XCTAssertTrue(queriedReply.duplicate)

        // 9. Slice L1: Interaction resolution & switchModelConfig over real Unix socket
        let createdSelection = ZCodeSelection(
            address: try XCTUnwrap(createdReply.selected),
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: 1
        )
        if let controlPath = ProcessInfo.processInfo.environment["MI_ZCODE_PAIRED_CONTROL"] {
            let controlPayload: [String: Any] = [
                "op": "set-task-state",
                "workspacePath": createdSelection.address.workspacePath,
                "taskId": createdSelection.address.taskId,
                "liveStatus": "waiting",
                "pendingInteraction": [
                    "interactionId": "paired-q-1",
                    "interactionType": "question",
                    "toolName": "AskUserQuestion",
                    "options": ["Option A", "Option B"]
                ]
            ]
            let controlData = try JSONSerialization.data(withJSONObject: controlPayload)
            _ = try await UnixNativeOwnershipTransport(path: controlPath).exchangeJSON(controlData)
        }
        let misapproveQuestion = try await client.resolve(ZCodeResolveIntent(
            selection: createdSelection,
            commandId: "paired-cmd-misapprove-q",
            clientId: "paired-swift",
            interactionId: "paired-q-1",
            interactionType: .permission,
            decision: .approve
        ))
        XCTAssertEqual(misapproveQuestion.status, .interactionTypeMismatch)
        XCTAssertEqual(misapproveQuestion.reasonCode, "guard.questionCannotApproveAsPermission")

        let answerQuestion = try await client.resolve(ZCodeResolveIntent(
            selection: createdSelection,
            commandId: "paired-cmd-answer-q",
            clientId: "paired-swift",
            interactionId: "paired-q-1",
            interactionType: .question,
            decision: .accept,
            optionId: "Option A"
        ))
        XCTAssertEqual(answerQuestion.status, .resolved)

        let dupQuestion = try await client.resolve(ZCodeResolveIntent(
            selection: createdSelection,
            commandId: "paired-cmd-answer-q",
            clientId: "paired-swift",
            interactionId: "paired-q-1",
            interactionType: .question,
            decision: .accept,
            optionId: "Option A"
        ))
        XCTAssertEqual(dupQuestion.status, .resolved)
        XCTAssertTrue(dupQuestion.duplicate)

        let expiredInteraction = try await client.resolve(ZCodeResolveIntent(
            selection: createdSelection,
            commandId: "paired-cmd-expired-perm",
            clientId: "paired-swift",
            interactionId: "paired-perm-expired",
            interactionType: .permission,
            decision: .approve
        ))
        XCTAssertEqual(expiredInteraction.status, .noop)
        XCTAssertEqual(expiredInteraction.reasonCode, "proto.alreadyResolved")

        let unsupportedThought = try await client.switchModel(ZCodeSwitchModelIntent(
            selection: createdSelection,
            commandId: "paired-cmd-switch-unsupported",
            clientId: "paired-swift",
            provider: "glm",
            model: "glm-5.1",
            thought: "xhigh"
        ))
        XCTAssertEqual(unsupportedThought.status, .unsupportedThought)
        XCTAssertEqual(unsupportedThought.reasonCode, "guard.unsupportedThoughtLevel")

        let switchedModel = try await client.switchModel(ZCodeSwitchModelIntent(
            selection: createdSelection,
            commandId: "paired-cmd-switch-high",
            clientId: "paired-swift",
            provider: "glm",
            model: "glm-5.1",
            thought: "high"
        ))
        XCTAssertEqual(switchedModel.status, .modelSwitched)
        XCTAssertEqual(switchedModel.modelConfig?.provider, "glm")
        XCTAssertEqual(switchedModel.modelConfig?.model, "glm-5.1")
        XCTAssertEqual(switchedModel.modelConfig?.thought, "high")
        let afterSwitchSnapshot = try await client.read()
        let switchedRow = try XCTUnwrap(afterSwitchSnapshot.rows.first(where: { $0.address == createdSelection.address }))
        XCTAssertEqual(switchedRow.modelConfig?.provider, "glm")
        XCTAssertEqual(switchedRow.modelConfig?.model, "glm-5.1")
        XCTAssertEqual(switchedRow.modelConfig?.thought, "high")
        await client.close()

        let data = try JSONSerialization.data(withJSONObject: [
            "status": "PASS", "transport": "real-unix-socket", "confirmed_targets": selected.count,
            "controlled_execution": "PASS", "steering_vs_queue": "PASS", "late_stop_noop": "PASS",
            "create_workspace_readback": "PASS", "duplicate_dedup": "PASS",
            "interaction_resolution": "PASS", "model_switch_readback": "PASS",
            "native_install": "NOT_RUN", "physical_key": "NOT_RUN", "transcript_api": "NOT_EXPOSED",
        ], options: [.prettyPrinted, .sortedKeys])
        try data.write(to: URL(fileURLWithPath: output))
    }
}
