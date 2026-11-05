import Darwin
import Foundation
import MicroInputCore
import XCTest
@testable import MicroInput

private let cindyAlpha1 = CindyTaskAddress(workspacePath: "/workspace/alpha", sessionId: "sess-alpha-1")
private let cindyAlpha2 = CindyTaskAddress(workspacePath: "/workspace/alpha", sessionId: "sess-alpha-2")
private let cindyBeta1 = CindyTaskAddress(workspacePath: "/workspace/beta", sessionId: "sess-beta-1")

private func makeCindySnapshot(
    rows: [CindyTaskAddress] = [cindyAlpha1, cindyAlpha2, cindyBeta1],
    generation: UInt64 = 1,
    ownerEpoch: String = "owner-1:1",
    selected: CindyTaskAddress? = nil
) -> CindySessionSnapshot {
    CindySessionSnapshot(
        instanceId: "cindy-instance-a",
        generation: generation,
        ownerEpoch: ownerEpoch,
        scopes: [
            CindyScope(workspacePath: "/workspace/alpha"),
            CindyScope(workspacePath: "/workspace/beta"),
        ],
        rows: rows.map {
            CindyTaskRow(
                address: $0,
                title: "Daily Standup",
                phase: "running",
                attention: false,
                pinned: false
            )
        },
        selected: selected
    )
}

private actor CindyTestPeer: CindySessionServing {
    var snapshot = makeCindySnapshot()
    var replyOverride: CindyOpenReply?
    var failure: CindyFailure?
    var connections = 0
    var reads = 0
    var opens: [CindyTaskAddress] = []
    var creates = 0
    var sends = 0
    var stops = 0
    var resolves = 0
    var timeoutNextSend = false
    var recordedCommands: [String: CindyCommandReply] = [:]

    func connect() async -> Result<CindySessionSnapshot, CindyFailure> {
        connections += 1
        if let failure { return .failure(failure) }
        return .success(snapshot)
    }

    func read() async -> Result<CindySessionSnapshot, CindyFailure> {
        reads += 1
        if let failure { return .failure(failure) }
        return .success(snapshot)
    }

    func open(_ selection: CindySelection) async -> CindyOpenReply {
        if failure != nil {
            return CindyOpenReply(
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .unavailable,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }
        if let replyOverride {
            if replyOverride.status == .confirmed || replyOverride.status == .delivered {
                opens.append(selection.address)
            }
            return replyOverride
        }
        guard snapshot.rows.contains(where: { $0.address == selection.address }) else {
            return CindyOpenReply(
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }
        opens.append(selection.address)
        snapshot.selected = selection.address
        return CindyOpenReply(
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            ownerEpoch: snapshot.ownerEpoch,
            status: .confirmed,
            delivered: selection.address,
            selected: selection.address,
            deepLink: selection.address.deepLink
        )
    }

    func create(_ intent: CindyCreateIntent) async -> CindyCommandReply {
        creates += 1
        let created = CindyTaskAddress(
            workspacePath: intent.scope.workspacePath,
            sessionId: "sess-created-\(creates)"
        )
        let row = CindyTaskRow(
            address: created,
            title: intent.text ?? "Created",
            phase: intent.text != nil ? "running" : "idle",
            attention: false,
            pinned: false,
            runId: intent.text != nil ? "run-created-\(creates)" : nil,
            unread: false,
            eventWatermark: UInt64(100 + creates)
        )
        snapshot.rows.append(row)
        snapshot.selected = created
        let reply = CindyCommandReply(
            commandId: intent.commandId,
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            ownerEpoch: snapshot.ownerEpoch,
            status: .confirmed,
            delivered: created,
            selected: created,
            deepLink: created.deepLink,
            runId: row.runId
        )
        recordedCommands["\(intent.scope.workspacePath)::\(intent.commandId)"] = reply
        return reply
    }

    func send(_ intent: CindySendIntent) async -> CindyCommandReply {
        let addr = intent.selection.address
        guard let idx = snapshot.rows.firstIndex(where: { $0.address == addr }) else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .stale,
                reasonCode: "guard.targetMissing"
            )
        }
        sends += 1
        let reply: CindyCommandReply
        if snapshot.rows[idx].phase != "running" {
            let runId = "run-send-\(sends)"
            snapshot.rows[idx].phase = "running"
            snapshot.rows[idx].runId = runId
            reply = CindyCommandReply(
                commandId: intent.commandId,
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .started,
                selected: addr,
                delivery: .auto,
                runId: runId
            )
        } else if intent.deliveryMode == .steer {
            reply = CindyCommandReply(
                commandId: intent.commandId,
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .steered,
                selected: addr,
                delivery: .steer,
                runId: snapshot.rows[idx].runId
            )
        } else {
            snapshot.rows[idx].queuedCount += 1
            reply = CindyCommandReply(
                commandId: intent.commandId,
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .queued,
                selected: addr,
                delivery: .queue,
                runId: snapshot.rows[idx].runId,
                queuedCount: snapshot.rows[idx].queuedCount
            )
        }
        recordedCommands["\(addr.workspacePath)::\(intent.commandId)"] = reply
        if timeoutNextSend {
            timeoutNextSend = false
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .unknown,
                reasonCode: "bridge.transportTimeout"
            )
        }
        return reply
    }

    func stop(_ intent: CindyStopIntent) async -> CindyCommandReply {
        let addr = intent.selection.address
        guard let idx = snapshot.rows.firstIndex(where: { $0.address == addr }),
              snapshot.rows[idx].phase == "running",
              snapshot.rows[idx].runId == intent.expectedRunId else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .noop,
                selected: addr,
                reasonCode: "guard.stopTargetChanged"
            )
        }
        stops += 1
        snapshot.rows[idx].phase = "idle"
        snapshot.rows[idx].runId = nil
        let reply = CindyCommandReply(
            commandId: intent.commandId,
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            ownerEpoch: snapshot.ownerEpoch,
            status: .stopped,
            selected: addr,
            runId: intent.expectedRunId
        )
        recordedCommands["\(addr.workspacePath)::\(intent.commandId)"] = reply
        return reply
    }

    func resolve(_ intent: CindyResolveIntent) async -> CindyCommandReply {
        let addr = intent.selection.address
        guard let idx = snapshot.rows.firstIndex(where: { $0.address == addr }),
              let pending = snapshot.rows[idx].pendingInteraction,
              pending.interactionId == intent.interactionId else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                ownerEpoch: snapshot.ownerEpoch,
                status: .noop,
                selected: addr,
                reasonCode: "guard.interactionAlreadyResolved"
            )
        }
        resolves += 1
        snapshot.rows[idx].pendingInteraction = nil
        snapshot.rows[idx].attention = false
        snapshot.rows[idx].phase = "running"
        let reply = CindyCommandReply(
            commandId: intent.commandId,
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            ownerEpoch: snapshot.ownerEpoch,
            status: .resolved,
            selected: addr,
            interactionId: pending.interactionId,
            interactionType: pending.interactionType
        )
        recordedCommands["\(addr.workspacePath)::\(intent.commandId)"] = reply
        return reply
    }

    func query(_ intent: CindyQueryIntent) async -> CindyCommandReply {
        if let recorded = recordedCommands["\(intent.scope.workspacePath)::\(intent.commandId)"] {
            var dup = recorded
            dup.duplicate = true
            return dup
        }
        return CindyCommandReply(
            commandId: intent.commandId,
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            ownerEpoch: snapshot.ownerEpoch,
            status: .noop,
            reasonCode: "query.notFound"
        )
    }

    func close() async {}

    func setSnapshot(_ value: CindySessionSnapshot) { snapshot = value }
    func setReplyOverride(_ value: CindyOpenReply?) { replyOverride = value }
    func setFailure(_ value: CindyFailure?) { failure = value }
    func setTimeoutNextSend(_ value: Bool) { timeoutNextSend = value }
    func counts() -> (Int, Int, Int) { (connections, reads, opens.count) }
    func commandStats() -> (creates: Int, sends: Int, stops: Int, resolves: Int) { (creates, sends, stops, resolves) }
    func opened() -> [CindyTaskAddress] { opens }
}

@MainActor
final class CindySessionTests: XCTestCase {
    func testDisabledAndOutOfScopeHaveZeroMetadataReads() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        await controller.refreshNow()
        let disabledCounts = await peer.counts()
        XCTAssertEqual(disabledCounts.0 + disabledCounts.1 + disabledCounts.2, 0)

        controller.setEnabled(true)
        await controller.refreshNow()
        let valid = try XCTUnwrap(controller.selection(for: cindyAlpha1))
        let outOfScope = CindySelection(
            address: CindyTaskAddress(workspacePath: "/workspace/secret", sessionId: "sess-secret-1"),
            instanceId: valid.instanceId,
            generation: valid.generation,
            ownerEpoch: valid.ownerEpoch,
            localEpoch: valid.localEpoch
        )
        let before = await peer.counts()
        let result = await controller.open(outOfScope, admitted: { true })
        let after = await peer.counts()
        XCTAssertEqual(result, .stale)
        XCTAssertEqual(before.1, after.1)
        XCTAssertEqual(after.2, 0)
        await controller.shutdown()
    }

    func testSameNameTasksRemainDistinctWithinAndAcrossWorkspaces() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()
        XCTAssertEqual(controller.rows.count, 3)

        for target in [cindyAlpha2, cindyBeta1, cindyAlpha1] {
            let selection = try XCTUnwrap(controller.selection(for: target))
            let result = await controller.open(selection, admitted: { true })
            XCTAssertEqual(result, .confirmed)
            XCTAssertEqual(controller.delivered, target)
            XCTAssertEqual(controller.selected, target)
            XCTAssertEqual(controller.lastDeepLink, target.deepLink)
        }

        let opened = await peer.opened()
        XCTAssertEqual(opened, [cindyAlpha2, cindyBeta1, cindyAlpha1])
        await controller.shutdown()
    }

    func testDeliveredAndConfirmedAreStrictlySeparated() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()

        await peer.setReplyOverride(
            CindyOpenReply(
                instanceId: "cindy-instance-a",
                generation: 1,
                ownerEpoch: "owner-1:1",
                status: .delivered,
                delivered: cindyAlpha2,
                selected: nil,
                deepLink: cindyAlpha2.deepLink
            )
        )
        let firstSelection = try XCTUnwrap(controller.selection(for: cindyAlpha2))
        let deliveredResult = await controller.open(firstSelection, admitted: { true })
        XCTAssertEqual(deliveredResult, .delivered)
        XCTAssertEqual(controller.delivered, cindyAlpha2)
        XCTAssertNil(controller.selected)
        XCTAssertEqual(controller.lastDeepLink, "cindy://session/sess-alpha-2")

        await peer.setReplyOverride(nil)
        let secondSelection = try XCTUnwrap(controller.selection(for: cindyAlpha2))
        let confirmedResult = await controller.open(secondSelection, admitted: { true })
        XCTAssertEqual(confirmedResult, .confirmed)
        XCTAssertEqual(controller.delivered, cindyAlpha2)
        XCTAssertEqual(controller.selected, cindyAlpha2)
        await controller.shutdown()
    }

    func testWrongSelectedReadbackDowngradesToUnconfirmed() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()

        await peer.setReplyOverride(
            CindyOpenReply(
                instanceId: "cindy-instance-a",
                generation: 1,
                ownerEpoch: "owner-1:1",
                status: .confirmed,
                delivered: cindyAlpha2,
                selected: cindyAlpha1,
                deepLink: cindyAlpha2.deepLink
            )
        )
        let selection = try XCTUnwrap(controller.selection(for: cindyAlpha2))
        let result = await controller.open(selection, admitted: { true })
        XCTAssertEqual(result, .unconfirmed)
        XCTAssertEqual(controller.delivered, cindyAlpha2)
        XCTAssertNil(controller.selected)
        await controller.shutdown()
    }

    func testOwnerEpochOrGenerationChangeInvalidatesOldSelection() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()

        let oldSelection = try XCTUnwrap(controller.selection(for: cindyAlpha1))
        await peer.setSnapshot(makeCindySnapshot(generation: 2, ownerEpoch: "owner-2:2"))
        await controller.refreshNow()
        XCTAssertEqual(controller.state, .ready)
        XCTAssertEqual(controller.ownerEpoch, "owner-2:2")

        let staleResult = await controller.open(oldSelection, admitted: { true })
        XCTAssertEqual(staleResult, .stale)
        let opened = await peer.opened()
        XCTAssertTrue(opened.isEmpty)
        await controller.shutdown()
    }

    func testStoreQueueRecordsDeliveredWithoutHistoryVisitAndConfirmedWithHistoryVisit() async throws {
        let peer = CindyTestPeer()
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let core = MicroInputCore.MicroInput()
        let executor = RecordingHostActionExecutor()
        let store = Store(
            nativeOwnership: NativeOwnershipController(isolated: true),
            backend: core,
            documentURL: directory.appendingPathComponent("workbench.json"),
            actionExecutor: executor,
            cindySessions: controller
        )
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        let profile = try XCTUnwrap(store.profile?.id.text)
        await store.setHostActiveProfile(profile)
        controller.setEnabled(true)
        await controller.refreshNow()

        await peer.setReplyOverride(
            CindyOpenReply(
                instanceId: "cindy-instance-a",
                generation: 1,
                ownerEpoch: "owner-1:1",
                status: .delivered,
                delivered: cindyAlpha2,
                selected: nil,
                deepLink: cindyAlpha2.deepLink
            )
        )
        let deliveredSelection = try XCTUnwrap(controller.selection(for: cindyAlpha2))
        XCTAssertTrue(store.openCindySession(deliveredSelection))
        await store.hostExecutionTask?.value
        XCTAssertEqual(store.lastHostActionReceipt?.actionType, "cindy_open")
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "delivered")
        XCTAssertNotEqual(
            store.navigationHistory.current,
            .cindy(workspacePath: "/workspace/alpha", sessionId: "sess-alpha-2")
        )

        await peer.setReplyOverride(nil)
        let confirmedSelection = try XCTUnwrap(controller.selection(for: cindyAlpha2))
        XCTAssertTrue(store.openCindySession(confirmedSelection))
        await store.hostExecutionTask?.value
        XCTAssertEqual(store.lastHostActionReceipt?.actionType, "cindy_open")
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "confirmed")
        XCTAssertEqual(
            store.navigationHistory.current,
            .cindy(workspacePath: "/workspace/alpha", sessionId: "sess-alpha-2")
        )
        await store.shutdown()
    }

    func testStopAndResolveRequireCommittedSelectionAndExactRunOrInteractionIdentity() async throws {
        let peer = CindyTestPeer()
        await peer.setSnapshot(
            CindySessionSnapshot(
                instanceId: "cindy-instance-a",
                generation: 1,
                ownerEpoch: "owner-1:1",
                scopes: [
                    CindyScope(workspacePath: "/workspace/alpha"),
                    CindyScope(workspacePath: "/workspace/beta"),
                ],
                rows: [
                    CindyTaskRow(
                        address: cindyAlpha1,
                        title: "Running Task",
                        phase: "running",
                        attention: false,
                        pinned: true,
                        runId: "run-alpha-1",
                        unread: false,
                        eventWatermark: 10
                    ),
                    CindyTaskRow(
                        address: cindyAlpha2,
                        title: "Interaction Task",
                        phase: "needs-interaction",
                        attention: true,
                        pinned: false,
                        runId: "run-alpha-2",
                        unread: false,
                        eventWatermark: 20,
                        pendingInteraction: CindyPendingInteraction(
                            interactionId: "perm-alpha-2",
                            interactionType: .permission,
                            toolName: "bash"
                        )
                    ),
                ],
                selected: cindyAlpha1
            )
        )
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()

        // Unselected target cannot be stopped.
        let unselectedStop = await controller.stop(address: cindyAlpha2, expectedRunId: "run-alpha-2")
        XCTAssertEqual(unselectedStop.status, .unconfirmed)
        XCTAssertEqual(unselectedStop.reasonCode, "guard.selectionNotCommitted")
        let stats1 = await peer.commandStats()
        XCTAssertEqual(stats1.stops, 0)

        // Mismatched runId on selected target is noop with zero abort calls.
        let mismatchStop = await controller.stop(address: cindyAlpha1, expectedRunId: "run-old-0")
        XCTAssertEqual(mismatchStop.status, .noop)
        XCTAssertEqual(mismatchStop.reasonCode, "guard.stopTargetChanged")
        let stats2 = await peer.commandStats()
        XCTAssertEqual(stats2.stops, 0)

        // Exact runId stops once; duplicate commandId returns duplicate without re-running.
        let exactStop = await controller.stop(
            address: cindyAlpha1,
            expectedRunId: "run-alpha-1",
            commandId: "cmd-stop-1"
        )
        XCTAssertEqual(exactStop.status, .stopped)
        let dupStop = await controller.stop(
            address: cindyAlpha1,
            expectedRunId: "run-alpha-1",
            commandId: "cmd-stop-1"
        )
        XCTAssertEqual(dupStop.status, .stopped)
        XCTAssertTrue(dupStop.duplicate)
        let stats3 = await peer.commandStats()
        XCTAssertEqual(stats3.stops, 1)

        // Select cindyAlpha2 to test resolve guards.
        _ = await controller.open(cindyAlpha2)
        let questionAsPerm = await controller.resolve(
            address: cindyAlpha2,
            interactionId: "perm-alpha-2",
            interactionType: .question,
            decision: .approve
        )
        XCTAssertEqual(questionAsPerm.status, .interactionTypeMismatch)
        XCTAssertEqual(questionAsPerm.reasonCode, "guard.questionCannotApproveAsPermission")

        let expiredResolve = await controller.resolve(
            address: cindyAlpha2,
            interactionId: "perm-expired",
            interactionType: .permission,
            decision: .approve
        )
        XCTAssertEqual(expiredResolve.status, .noop)
        XCTAssertEqual(expiredResolve.reasonCode, "guard.interactionAlreadyResolved")
        let stats4 = await peer.commandStats()
        XCTAssertEqual(stats4.resolves, 0)

        let exactResolve = await controller.resolve(
            address: cindyAlpha2,
            interactionId: "perm-alpha-2",
            interactionType: .permission,
            decision: .approve,
            commandId: "cmd-resolve-1"
        )
        XCTAssertEqual(exactResolve.status, .resolved)
        let stats5 = await peer.commandStats()
        XCTAssertEqual(stats5.resolves, 1)

        await controller.shutdown()
    }

    func testUnreadTriStateNeverFabricatesUnreadFromCompletedAndPendingRoundDoesNotRepeat() async throws {
        let completedRead = CindyTaskAddress(workspacePath: "/workspace/alpha", sessionId: "sess-completed-read")
        let completedUnknown = CindyTaskAddress(workspacePath: "/workspace/alpha", sessionId: "sess-completed-unknown")
        let completedUnread = CindyTaskAddress(workspacePath: "/workspace/alpha", sessionId: "sess-completed-unread")
        let errorUnread = CindyTaskAddress(workspacePath: "/workspace/beta", sessionId: "sess-error-unread")

        let peer = CindyTestPeer()
        await peer.setSnapshot(
            CindySessionSnapshot(
                instanceId: "cindy-instance-a",
                generation: 1,
                ownerEpoch: "owner-1:1",
                scopes: [
                    CindyScope(workspacePath: "/workspace/alpha"),
                    CindyScope(workspacePath: "/workspace/beta"),
                ],
                rows: [
                    CindyTaskRow(
                        address: completedRead,
                        title: "Completed Read",
                        phase: "completed",
                        attention: false,
                        pinned: false,
                        unread: false,
                        eventWatermark: 10
                    ),
                    CindyTaskRow(
                        address: completedUnknown,
                        title: "Completed Unknown Unread",
                        phase: "completed",
                        attention: false,
                        pinned: false,
                        unread: nil,
                        eventWatermark: 11
                    ),
                    CindyTaskRow(
                        address: completedUnread,
                        title: "Completed Unread",
                        phase: "completed",
                        attention: true,
                        pinned: false,
                        unread: true,
                        eventWatermark: 12
                    ),
                    CindyTaskRow(
                        address: errorUnread,
                        title: "Error Unread",
                        phase: "error",
                        attention: true,
                        pinned: false,
                        unread: true,
                        eventWatermark: 13
                    ),
                ],
                selected: nil
            )
        )
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()

        let first = await controller.advanceAttentionRound()
        XCTAssertEqual(first.status, .confirmed)
        XCTAssertEqual(first.selected, errorUnread)

        let second = await controller.advanceAttentionRound()
        XCTAssertEqual(second.status, .confirmed)
        XCTAssertEqual(second.selected, completedUnread)

        // Neither completedRead nor completedUnknown is ever visited, and the round is now exhausted.
        let third = await controller.advanceAttentionRound()
        XCTAssertEqual(third.status, .noop)
        XCTAssertEqual(third.reasonCode, "round.exhausted")

        // When completedUnread receives a new eventWatermark, it immediately re-enters the round.
        var updatedSnapshot = await peer.snapshot
        updatedSnapshot.rows[2].eventWatermark = 14
        await peer.setSnapshot(updatedSnapshot)
        await controller.refreshNow()

        let fourth = await controller.advanceAttentionRound()
        XCTAssertEqual(fourth.status, .confirmed)
        XCTAssertEqual(fourth.selected, completedUnread)

        await controller.shutdown()
    }

    func testCreateSendAndTimeoutQueryReconcileWithoutDoubleSend() async throws {
        let peer = CindyTestPeer()
        await peer.setSnapshot(
            CindySessionSnapshot(
                instanceId: "cindy-instance-a",
                generation: 1,
                ownerEpoch: "owner-1:1",
                scopes: [
                    CindyScope(workspacePath: "/workspace/alpha"),
                    CindyScope(workspacePath: "/workspace/beta"),
                ],
                rows: [
                    CindyTaskRow(
                        address: cindyBeta1,
                        title: "Beta Idle Task",
                        phase: "idle",
                        attention: false,
                        pinned: false,
                        unread: nil,
                        eventWatermark: 1
                    ),
                ],
                selected: cindyBeta1
            )
        )
        let controller = CindySessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refreshNow()

        // Steer on idle session is rejected locally.
        let steerIdle = await controller.send(
            address: cindyBeta1,
            text: "Steer while idle",
            deliveryMode: .steer
        )
        XCTAssertEqual(steerIdle.status, .denied)
        XCTAssertEqual(steerIdle.reasonCode, "guard.steerRequiresRunningTurn")

        // Start turn on cindyBeta1.
        let started = await controller.send(
            address: cindyBeta1,
            text: "Start turn",
            deliveryMode: .auto,
            commandId: "cmd-start-1"
        )
        XCTAssertEqual(started.status, .started)

        // Simulate transport timeout on next queued message.
        await peer.setTimeoutNextSend(true)
        let timedOut = await controller.send(
            address: cindyBeta1,
            text: "Queue follow-up",
            deliveryMode: .queue,
            commandId: "cmd-queue-timeout"
        )
        XCTAssertEqual(timedOut.status, .unknown)
        XCTAssertEqual(controller.pendingUnknownCommand?.commandId, "cmd-queue-timeout")

        // Any new command in /workspace/beta is blocked until cmd-queue-timeout is reconciled.
        let blocked = await controller.send(
            address: cindyBeta1,
            text: "Must be blocked",
            deliveryMode: .queue,
            commandId: "cmd-queue-blocked"
        )
        XCTAssertEqual(blocked.status, .unknown)
        XCTAssertEqual(blocked.reasonCode, "guard.unknownOutcomeRequiresReconcile")

        // Reconcile via query with the original commandId.
        let reconciled = await controller.query(
            scope: CindyScope(workspacePath: "/workspace/beta"),
            target: cindyBeta1,
            commandId: "cmd-queue-timeout"
        )
        XCTAssertEqual(reconciled.status, .queued)
        XCTAssertTrue(reconciled.duplicate)
        XCTAssertNil(controller.pendingUnknownCommand)
        let stats = await peer.commandStats()
        XCTAssertEqual(stats.sends, 2)

        // Create a new session in /workspace/alpha and confirm selected readback.
        let created = await controller.create(
            scope: CindyScope(workspacePath: "/workspace/alpha"),
            text: "New Alpha Task",
            commandId: "cmd-create-alpha"
        )
        XCTAssertEqual(created.status, .confirmed)
        XCTAssertEqual(created.selected?.workspacePath, "/workspace/alpha")
        XCTAssertEqual(controller.selected, created.selected)

        await controller.shutdown()
    }
}

final class CindyGrantTests: XCTestCase {
    func testGrantRequiresPrivateRegularFileAndPrivateParent() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("cindy-grant-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: folder) }
        let file = folder.appendingPathComponent("cindy-grant.json")
        let grant = CindyGrant(
            version: 1,
            socketPath: "/private/tmp/cindy.sock",
            token: String(repeating: "c", count: 32),
            instanceId: "cindy-instance-a",
            generation: 1,
            ownerEpoch: "owner-1:1",
            scopes: [CindyScope(workspacePath: "/workspace/alpha")]
        )
        try JSONEncoder().encode(grant).write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        XCTAssertEqual(try CindyGrant.load(path: file.path), grant)

        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: file.path)
        XCTAssertThrowsError(try CindyGrant.load(path: file.path))

        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder.path)
        XCTAssertThrowsError(try CindyGrant.load(path: file.path))

        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: folder.path)
        let link = folder.appendingPathComponent("link.json")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)
        XCTAssertThrowsError(try CindyGrant.load(path: link.path))
    }
}
