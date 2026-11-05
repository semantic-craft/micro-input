import XCTest
import Foundation
import MicroInputCore
@testable import MicroInput

private func agRow(
    conversationId: String = "conv-a",
    workspacePath: String = "/ws/a",
    phase: AntigravitySessionPhase = .idle,
    lastEventType: AntigravityHookEventType = .sessionStart,
    fullyIdle: Bool = true,
    terminationReason: String? = nil,
    transitionCount: UInt64 = 1
) -> AntigravitySessionRow {
    AntigravitySessionRow(
        conversationId: conversationId,
        workspacePath: workspacePath,
        modelName: "gemini-2.5-pro",
        phase: phase,
        lastEventType: lastEventType,
        invocationNum: 1,
        stepIdx: 1,
        executionNum: 1,
        lastToolName: phase == .runningTool ? "run_command" : nil,
        terminationReason: terminationReason,
        fullyIdle: fullyIdle,
        transitionCount: transitionCount
    )
}

private func agSnapshot(
    generation: UInt64 = 1,
    cursor: UInt64 = 0,
    rows: [AntigravitySessionRow] = [agRow(), agRow(conversationId: "conv-b", workspacePath: "/ws/b")],
    changes: [AntigravityEventChange] = [],
    transcriptReads: UInt64 = 0
) -> AntigravityStatusSnapshot {
    AntigravityStatusSnapshot(
        instanceId: "ag-instance-1",
        generation: generation,
        allowedWorkspaces: ["/ws/a", "/ws/b"],
        capabilities: .installed2_29_0,
        cursor: cursor,
        rows: rows,
        changes: changes,
        transcriptReads: transcriptReads
    )
}

private actor AntigravityTestPeer: AntigravityStatusServing {
    var value = agSnapshot()
    var errorToThrow: AntigravityFailure?
    var connections = 0
    var reads = 0
    var closes = 0
    var heldConnect = false
    var connectWaiting: CheckedContinuation<Void, Never>?
    var heldRead = false
    var readWaiting: CheckedContinuation<Void, Never>?
    var heldClose = false
    var closeWaiting: CheckedContinuation<Void, Never>?

    func connect() async throws -> AntigravityStatusSnapshot {
        connections += 1
        if let errorToThrow { throw errorToThrow }
        let captured = value
        if heldConnect { await withCheckedContinuation { connectWaiting = $0 } }
        return captured
    }

    func read() async throws -> AntigravityStatusSnapshot {
        reads += 1
        if let errorToThrow { throw errorToThrow }
        let captured = value
        if heldRead { await withCheckedContinuation { readWaiting = $0 } }
        return captured
    }

    func close() async {
        closes += 1
        if heldClose { await withCheckedContinuation { closeWaiting = $0 } }
    }

    func set(_ snapshot: AntigravityStatusSnapshot) {
        value = snapshot
        errorToThrow = nil
    }

    func failWith(_ error: AntigravityFailure?) {
        errorToThrow = error
    }

    func counts() -> (connections: Int, reads: Int, closes: Int) {
        (connections, reads, closes)
    }

    func holdConnect() { heldConnect = true }
    func hasConnectWaiter() -> Bool { connectWaiting != nil }
    func releaseConnect() { heldConnect = false; connectWaiting?.resume(); connectWaiting = nil }

    func holdRead() { heldRead = true }
    func hasReadWaiter() -> Bool { readWaiting != nil }
    func releaseRead() { heldRead = false; readWaiting?.resume(); readWaiting = nil }

    func holdClose() { heldClose = true }
    func hasCloseWaiter() -> Bool { closeWaiting != nil }
    func releaseClose() { heldClose = false; closeWaiting?.resume(); closeWaiting = nil }
}

@MainActor
final class AntigravityStatusTests: XCTestCase {
    func testVersionAndCapabilityPartitionRecorded() {
        let caps = AntigravityCapabilities.installed2_29_0
        XCTAssertTrue(caps.valid)
        XCTAssertTrue(caps.supportsCatalogNavigation)
        XCTAssertEqual(caps.bundleId, "com.google.antigravity")
        XCTAssertEqual(caps.installedVersion, "2.29.0")
        XCTAssertEqual(caps.languageServerCl, "1013060670")
        XCTAssertEqual(caps.agentapiCommands, ["get-conversation-metadata", "new-conversation", "send-message"])
        XCTAssertEqual(caps.hookEvents, ["SessionStart", "PreInvocation", "PostInvocation", "PreToolUse", "PostToolUse", "Stop"])
        XCTAssertTrue(caps.supportsSessionCatalog)
        XCTAssertTrue(caps.supportsTargetFocus)
        XCTAssertTrue(caps.schemaVerified)
        XCTAssertTrue(caps.supportsStopTurn)
        XCTAssertTrue(caps.supportsApprovalGate)
        XCTAssertTrue(caps.supportsModelConfig)
        XCTAssertTrue(caps.stopSchemaVerified)
        XCTAssertTrue(caps.approvalSchemaVerified)
        XCTAssertTrue(caps.modelConfigSchemaVerified)
        XCTAssertTrue(caps.csrfAuthVerified)
        XCTAssertFalse(caps.arbitraryRpcExposed)
        XCTAssertTrue(caps.supportsStopConversation)
        XCTAssertTrue(caps.supportsResolveInteraction)
        XCTAssertTrue(caps.supportsSwitchModelConfig)

        let overclaimed = AntigravityCapabilities(
            bundleId: caps.bundleId,
            installedVersion: caps.installedVersion,
            languageServerCl: caps.languageServerCl,
            agentapiCommands: caps.agentapiCommands,
            sidecarSdkVersion: caps.sidecarSdkVersion,
            hookEvents: caps.hookEvents,
            supportsSessionCatalog: true,
            supportsTargetFocus: true,
            supportsStopTurn: true,
            supportsApprovalGate: false,
            supportsModelConfig: false,
            schemaVerified: true,
            stopSchemaVerified: false,
            approvalSchemaVerified: false,
            modelConfigSchemaVerified: false,
            csrfAuthVerified: true,
            arbitraryRpcExposed: false
        )
        XCTAssertFalse(overclaimed.valid)

        let arbitraryExposed = AntigravityCapabilities(
            bundleId: caps.bundleId,
            installedVersion: caps.installedVersion,
            languageServerCl: caps.languageServerCl,
            agentapiCommands: caps.agentapiCommands,
            sidecarSdkVersion: caps.sidecarSdkVersion,
            hookEvents: caps.hookEvents,
            supportsSessionCatalog: true,
            supportsTargetFocus: true,
            supportsStopTurn: true,
            supportsApprovalGate: true,
            supportsModelConfig: true,
            schemaVerified: true,
            stopSchemaVerified: true,
            approvalSchemaVerified: true,
            modelConfigSchemaVerified: true,
            csrfAuthVerified: true,
            arbitraryRpcExposed: true
        )
        XCTAssertFalse(arbitraryExposed.valid)

        let unverifiedSchema = AntigravityCapabilities(
            bundleId: caps.bundleId,
            installedVersion: caps.installedVersion,
            languageServerCl: caps.languageServerCl,
            agentapiCommands: caps.agentapiCommands,
            sidecarSdkVersion: caps.sidecarSdkVersion,
            hookEvents: caps.hookEvents,
            supportsSessionCatalog: true,
            supportsTargetFocus: true,
            supportsStopTurn: false,
            supportsApprovalGate: false,
            schemaVerified: false
        )
        XCTAssertFalse(unverifiedSchema.valid)
        XCTAssertFalse(unverifiedSchema.supportsCatalogNavigation)
    }

    func testTwoSessionsNeverCrossAndDuplicatesAreDeduplicated() async throws {
        let fixture = AntigravityStatusFixture()
        let controller = AntigravityStatusController(client: fixture, pollNanoseconds: 60_000_000_000)
        await controller.refresh()
        let initialCounts = await fixture.counts()
        XCTAssertEqual(initialCounts.reads, 0)

        let startA = AntigravityHookEvent(
            conversationId: AntigravityStatusFixture.sessionA.conversationId,
            workspacePath: AntigravityStatusFixture.sessionA.workspacePath,
            eventType: .sessionStart,
            modelName: "gemini-2.5-pro",
            invocationNum: 0,
            stepIdx: 0,
            executionNum: 0,
            toolName: nil,
            terminationReason: nil,
            hasError: false,
            fullyIdle: true,
            eventId: nil
        )
        let startB = AntigravityHookEvent(
            conversationId: AntigravityStatusFixture.sessionB.conversationId,
            workspacePath: AntigravityStatusFixture.sessionB.workspacePath,
            eventType: .sessionStart,
            modelName: "gemini-2.5-flash",
            invocationNum: 0,
            stepIdx: 0,
            executionNum: 0,
            toolName: nil,
            terminationReason: nil,
            hasError: false,
            fullyIdle: true,
            eventId: nil
        )
        let firstAccepted = try await fixture.ingest(startA)
        let secondAccepted = try await fixture.ingest(startB)
        XCTAssertTrue(firstAccepted)
        XCTAssertTrue(secondAccepted)

        controller.setEnabled(true)
        for _ in 0..<200 {
            if controller.state == .ready { break }
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertEqual(controller.state, .ready)
        XCTAssertEqual(controller.rows.count, 2)

        let toolA = AntigravityHookEvent(
            conversationId: AntigravityStatusFixture.sessionA.conversationId,
            workspacePath: AntigravityStatusFixture.sessionA.workspacePath,
            eventType: .preToolUse,
            modelName: "gemini-2.5-pro",
            invocationNum: 1,
            stepIdx: 3,
            executionNum: 1,
            toolName: "edit_file",
            terminationReason: nil,
            hasError: false,
            fullyIdle: false,
            eventId: nil
        )
        let toolAccepted1 = try await fixture.ingest(toolA)
        let toolAccepted2 = try await fixture.ingest(toolA)
        XCTAssertTrue(toolAccepted1)
        XCTAssertFalse(toolAccepted2)

        await controller.refresh()
        let rowA = try XCTUnwrap(controller.row(for: AntigravityStatusFixture.sessionA))
        let rowB = try XCTUnwrap(controller.row(for: AntigravityStatusFixture.sessionB))
        XCTAssertEqual(rowA.phase, .runningTool)
        XCTAssertEqual(rowA.lastToolName, "edit_file")
        XCTAssertEqual(rowA.transitionCount, 2)
        XCTAssertEqual(rowB.phase, .idle)
        XCTAssertNil(rowB.lastToolName)
        XCTAssertEqual(rowB.transitionCount, 1)
        XCTAssertEqual(controller.lastChanges.count, 1)
        let counts = await fixture.counts()
        XCTAssertEqual(counts.duplicates, 1)
        XCTAssertEqual(counts.transcriptReads, 0)
        await controller.shutdown()
    }

    func testStopFullyIdleFalseNeverReportsCompleted() {
        let start = AntigravityHookEvent(
            conversationId: "conv-1",
            workspacePath: "/ws/a",
            eventType: .sessionStart,
            modelName: "gemini-2.5-pro",
            invocationNum: 0,
            stepIdx: 0,
            executionNum: 0,
            toolName: nil,
            terminationReason: nil,
            hasError: false,
            fullyIdle: true,
            eventId: nil
        )
        let initial = AntigravitySessionRow.reduce(prior: nil, event: start)
        XCTAssertEqual(initial.phase, .idle)

        let stopWithSubagents = AntigravityHookEvent(
            conversationId: "conv-1",
            workspacePath: "/ws/a",
            eventType: .stop,
            modelName: "gemini-2.5-pro",
            invocationNum: nil,
            stepIdx: nil,
            executionNum: nil,
            toolName: nil,
            terminationReason: "NO_TOOL_CALL",
            hasError: false,
            fullyIdle: false,
            eventId: nil
        )
        let backgroundRow = AntigravitySessionRow.reduce(prior: initial, event: stopWithSubagents)
        XCTAssertEqual(backgroundRow.phase, .backgroundActive)
        XCTAssertFalse(backgroundRow.phase.isCompleted)
        XCTAssertFalse(backgroundRow.fullyIdle)
        XCTAssertTrue(backgroundRow.valid)

        let invalidCompleted = agRow(
            phase: .completed,
            lastEventType: .stop,
            fullyIdle: false,
            terminationReason: "NO_TOOL_CALL"
        )
        XCTAssertFalse(invalidCompleted.valid)

        let stopFullyIdle = AntigravityHookEvent(
            conversationId: "conv-1",
            workspacePath: "/ws/a",
            eventType: .stop,
            modelName: "gemini-2.5-pro",
            invocationNum: nil,
            stepIdx: nil,
            executionNum: nil,
            toolName: nil,
            terminationReason: "NO_TOOL_CALL",
            hasError: false,
            fullyIdle: true,
            eventId: nil
        )
        XCTAssertNotEqual(
            stopWithSubagents.eventKey(prior: initial),
            stopFullyIdle.eventKey(prior: backgroundRow)
        )
        let completedRow = AntigravitySessionRow.reduce(prior: backgroundRow, event: stopFullyIdle)
        XCTAssertEqual(completedRow.phase, .completed)
        XCTAssertTrue(completedRow.phase.isCompleted)
        XCTAssertTrue(completedRow.fullyIdle)
        XCTAssertTrue(completedRow.valid)
    }

    func testStreamDisconnectAndGenerationChangeMarkStale() async {
        let peer = AntigravityTestPeer()
        let controller = AntigravityStatusController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refresh()
        XCTAssertEqual(controller.state, .ready)
        XCTAssertEqual(controller.rows.count, 2)

        await peer.failWith(.stale)
        await controller.refresh()
        XCTAssertEqual(controller.state, .unavailable(.stale))
        XCTAssertTrue(controller.rows.isEmpty)

        await peer.set(agSnapshot(generation: 1))
        controller.setEnabled(true)
        await controller.refresh()
        XCTAssertEqual(controller.state, .ready)

        await peer.set(agSnapshot(generation: 2))
        await controller.refresh()
        XCTAssertEqual(controller.state, .unavailable(.stale))
        XCTAssertTrue(controller.rows.isEmpty)
        await controller.shutdown()
    }

    func testSnapshotValidationRejectsTranscriptReadsAndSequenceGaps() {
        XCTAssertNoThrow(try agSnapshot().validate())
        XCTAssertThrowsError(try agSnapshot(transcriptReads: 1).validate()) { error in
            XCTAssertEqual(error as? AntigravityFailure, .invalidReply)
        }
        let change2 = AntigravityEventChange(sequence: 2, eventKey: "conv-a:/ws/a:Stop:1:1:1", row: agRow())
        XCTAssertThrowsError(try agSnapshot(cursor: 2, changes: [change2]).validate(after: 0)) { error in
            XCTAssertEqual(error as? AntigravityFailure, .gap)
        }
    }

    func testLateCallbacksCannotCorruptControllerState() async {
        let peer = AntigravityTestPeer()
        let controller = AntigravityStatusController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true)
        await controller.refresh()
        await peer.holdRead()
        let pending = Task { await controller.refresh() }
        for _ in 0..<1000 {
            if await peer.hasReadWaiter() { break }
            await Task.yield()
        }
        let hasWaiter = await peer.hasReadWaiter()
        XCTAssertTrue(hasWaiter)
        controller.setEnabled(false)
        await peer.releaseRead()
        await pending.value
        XCTAssertEqual(controller.state, .disabled)
        XCTAssertTrue(controller.rows.isEmpty)

        await peer.holdConnect()
        controller.setEnabled(true)
        for _ in 0..<1000 {
            if await peer.hasConnectWaiter() { break }
            await Task.yield()
        }
        let connectWaiting = await peer.hasConnectWaiter()
        XCTAssertTrue(connectWaiting)
        controller.setEnabled(false)
        await peer.set(agSnapshot(generation: 2))
        controller.setEnabled(true)
        await peer.releaseConnect()
        await controller.refresh()
        XCTAssertEqual(controller.state, .ready)

        await peer.holdClose()
        controller.setEnabled(false)
        controller.setEnabled(true)
        for _ in 0..<1000 {
            if await peer.hasCloseWaiter() { break }
            await Task.yield()
        }
        let closeWaiting = await peer.hasCloseWaiter()
        XCTAssertTrue(closeWaiting)
        XCTAssertEqual(controller.state, .connecting)
        await peer.releaseClose()
        await controller.refresh()
        XCTAssertEqual(controller.state, .ready)

        let beforeResume = await peer.counts()
        controller.resume()
        let afterNoopResume = await peer.counts()
        XCTAssertEqual(beforeResume.connections, afterNoopResume.connections)
        XCTAssertEqual(controller.state, .ready)

        controller.suspend()
        XCTAssertEqual(controller.state, .disabled)
        controller.resume()
        await controller.refresh()
        XCTAssertEqual(controller.state, .ready)

        await controller.shutdown()
    }

    func testGrantRequiresPrivateFileAndParent() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("ag-grant-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: folder) }
        let file = folder.appendingPathComponent("antigravity-grant.json")
        let grant = AntigravityGrant(
            version: 1,
            socketPath: "/private/ag.sock",
            token: String(repeating: "b", count: 64),
            instanceId: "ag-local",
            generation: 1,
            allowedWorkspaces: ["/ws/a"],
            capabilities: .installed2_29_0
        )
        try JSONEncoder().encode(grant).write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        XCTAssertEqual(try AntigravityGrant.load(file), grant)

        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: file.path)
        XCTAssertThrowsError(try AntigravityGrant.load(file))
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)

        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder.path)
        XCTAssertThrowsError(try AntigravityGrant.load(file))
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: folder.path)

        let link = folder.appendingPathComponent("link.json")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)
        XCTAssertThrowsError(try AntigravityGrant.load(link))
    }

    func testExistingCatalogAndExactNavigationConfirmSelectedReadbackAndIsolateSameNameAcrossWorkspaces() async throws {
        let fixture = AntigravityStatusFixture()
        let controller = AntigravityStatusController(client: fixture, pollNanoseconds: 60_000_000_000)

        try await fixture.ingest(AntigravityHookEvent(
            conversationId: AntigravityStatusFixture.sessionA.conversationId,
            workspacePath: AntigravityStatusFixture.sessionA.workspacePath,
            eventType: .sessionStart,
            modelName: "gemini-2.5-pro",
            invocationNum: 0,
            stepIdx: 0,
            executionNum: 0,
            toolName: nil,
            terminationReason: nil,
            hasError: false,
            fullyIdle: true,
            eventId: nil,
            title: "Shared Task Name",
            projectId: "proj-alpha"
        ))
        try await fixture.ingest(AntigravityHookEvent(
            conversationId: AntigravityStatusFixture.sessionB.conversationId,
            workspacePath: AntigravityStatusFixture.sessionB.workspacePath,
            eventType: .sessionStart,
            modelName: "gemini-2.5-flash",
            invocationNum: 0,
            stepIdx: 0,
            executionNum: 0,
            toolName: nil,
            terminationReason: nil,
            hasError: false,
            fullyIdle: true,
            eventId: nil,
            title: "Shared Task Name",
            projectId: "proj-beta"
        ))

        controller.setEnabled(true)
        for _ in 0..<200 {
            if controller.state == .ready { break }
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertEqual(controller.state, .ready)
        XCTAssertEqual(controller.rows.count, 2)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.title, "Shared Task Name")
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionB)?.title, "Shared Task Name")

        let selA = try XCTUnwrap(controller.selection(for: AntigravityStatusFixture.sessionA))
        let resA = await controller.open(selA, admitted: { true })
        XCTAssertEqual(resA, .confirmed)
        XCTAssertEqual(controller.selected, AntigravityStatusFixture.sessionA)
        XCTAssertEqual(controller.lastResult, .confirmed)

        let selB = try XCTUnwrap(controller.selection(for: AntigravityStatusFixture.sessionB))
        let resB = await controller.open(selB, admitted: { true })
        XCTAssertEqual(resB, .confirmed)
        XCTAssertEqual(controller.selected, AntigravityStatusFixture.sessionB)
        XCTAssertEqual(controller.lastResult, .confirmed)

        await fixture.setMode(.wrongSelected)
        let selWrong = try XCTUnwrap(controller.selection(for: AntigravityStatusFixture.sessionA))
        let resWrong = await controller.open(selWrong, admitted: { true })
        XCTAssertEqual(resWrong, .unconfirmed)
        XCTAssertEqual(controller.lastResult, .unconfirmed)
        await fixture.setMode(.normal)

        _ = await controller.open(try XCTUnwrap(controller.selection(for: AntigravityStatusFixture.sessionA)), admitted: { true })
        XCTAssertEqual(controller.selected, AntigravityStatusFixture.sessionA)

        let staleDeletedB = try XCTUnwrap(controller.selection(for: AntigravityStatusFixture.sessionB))
        await fixture.deleteSession(AntigravityStatusFixture.sessionB)
        let resDeleted = await controller.open(staleDeletedB, admitted: { true })
        XCTAssertEqual(resDeleted, .unconfirmed)
        XCTAssertEqual(controller.selected, AntigravityStatusFixture.sessionA)

        let selBeforeMissingSchema = try XCTUnwrap(controller.selection(for: AntigravityStatusFixture.sessionA))
        await fixture.setMode(.missingSchema)
        let resDenied = await controller.open(selBeforeMissingSchema, admitted: { true })
        XCTAssertEqual(resDenied, .denied)
        XCTAssertEqual(controller.state, .unavailable(.denied))

        let counts = await fixture.counts()
        XCTAssertEqual(counts.transcriptReads, 0)
        XCTAssertEqual(counts.tokenReads, 0)
        await controller.shutdown()
    }

    func testSnapshotValidationRejectsTokenReadsAndInvalidSelected() {
        let validKey = AntigravitySessionKey(conversationId: "conv-a", workspacePath: "/ws/a")
        let validSelected = AntigravityStatusSnapshot(
            instanceId: "ag-instance-1",
            generation: 1,
            allowedWorkspaces: ["/ws/a", "/ws/b"],
            capabilities: .installed2_29_0,
            cursor: 0,
            rows: [agRow()],
            changes: [],
            selected: validKey,
            transcriptReads: 0,
            tokenReads: 0
        )
        XCTAssertNoThrow(try validSelected.validate())

        let nonZeroTokenReads = AntigravityStatusSnapshot(
            instanceId: "ag-instance-1",
            generation: 1,
            allowedWorkspaces: ["/ws/a", "/ws/b"],
            capabilities: .installed2_29_0,
            cursor: 0,
            rows: [agRow()],
            changes: [],
            selected: validKey,
            transcriptReads: 0,
            tokenReads: 1
        )
        XCTAssertThrowsError(try nonZeroTokenReads.validate()) { error in
            XCTAssertEqual(error as? AntigravityFailure, .invalidReply)
        }

        let unknownSelected = AntigravityStatusSnapshot(
            instanceId: "ag-instance-1",
            generation: 1,
            allowedWorkspaces: ["/ws/a", "/ws/b"],
            capabilities: .installed2_29_0,
            cursor: 0,
            rows: [agRow()],
            changes: [],
            selected: AntigravitySessionKey(conversationId: "conv-missing", workspacePath: "/ws/a"),
            transcriptReads: 0,
            tokenReads: 0
        )
        XCTAssertThrowsError(try unknownSelected.validate()) { error in
            XCTAssertEqual(error as? AntigravityFailure, .invalidReply)
        }
    }

    func testCreateBindsProjectIdAndSendBindsConversationIdOnlyModifyingTargetAndZeroRedirectOnMissing() async throws {
        let fixture = AntigravityStatusFixture()
        let controller = AntigravityStatusController(client: fixture, pollNanoseconds: 60_000_000_000)

        try await fixture.ingest(AntigravityHookEvent(
            conversationId: AntigravityStatusFixture.sessionA.conversationId,
            workspacePath: AntigravityStatusFixture.sessionA.workspacePath,
            eventType: .sessionStart,
            modelName: "gemini-2.5-pro",
            invocationNum: 0,
            stepIdx: 0,
            executionNum: 0,
            toolName: nil,
            terminationReason: nil,
            hasError: false,
            fullyIdle: true,
            eventId: nil,
            title: "Alpha Session",
            projectId: "proj-alpha"
        ))
        try await fixture.ingest(AntigravityHookEvent(
            conversationId: AntigravityStatusFixture.sessionB.conversationId,
            workspacePath: AntigravityStatusFixture.sessionB.workspacePath,
            eventType: .sessionStart,
            modelName: "gemini-2.5-flash",
            invocationNum: 0,
            stepIdx: 0,
            executionNum: 0,
            toolName: nil,
            terminationReason: nil,
            hasError: false,
            fullyIdle: true,
            eventId: nil,
            title: "Beta Session",
            projectId: "proj-beta"
        ))

        controller.setEnabled(true)
        for _ in 0..<200 {
            if controller.state == .ready { break }
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertEqual(controller.state, .ready)

        let wrongCreate = try XCTUnwrap(controller.createIntent(
            workspacePath: AntigravityStatusFixture.workspaceA,
            projectId: "proj-beta",
            prompt: "Wrong project binding",
            title: "Should Fail",
            conversationId: "conv-wrong-proj",
            requestId: "req-wrong-proj"
        ))
        let wrongCreateRes = await controller.execute(wrongCreate, admitted: { true })
        XCTAssertEqual(wrongCreateRes, .unconfirmed)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "guard.projectIdMismatch")
        XCTAssertEqual(controller.rows.count, 2)

        let createdKey = AntigravitySessionKey(
            conversationId: "conv-alpha-2",
            workspacePath: AntigravityStatusFixture.workspaceA
        )
        let createCmd = try XCTUnwrap(controller.createIntent(
            workspacePath: AntigravityStatusFixture.workspaceA,
            projectId: "proj-alpha",
            prompt: "Create conversation bound to proj-alpha",
            title: "Alpha Created",
            modelName: "gemini-2.5-flash",
            conversationId: createdKey.conversationId,
            requestId: "req-create-alpha-2"
        ))
        let createRes = await controller.execute(createCmd, admitted: { true })
        XCTAssertEqual(createRes, .created)
        XCTAssertEqual(controller.selected, createdKey)
        XCTAssertEqual(controller.row(for: createdKey)?.projectId, "proj-alpha")
        XCTAssertEqual(controller.row(for: createdKey)?.phase, .running)
        XCTAssertEqual(controller.rows.count, 3)

        _ = await controller.open(try XCTUnwrap(controller.selection(for: AntigravityStatusFixture.sessionA)), admitted: { true })
        XCTAssertEqual(controller.selected, AntigravityStatusFixture.sessionA)
        let beforeA = try XCTUnwrap(controller.row(for: AntigravityStatusFixture.sessionA))
        let beforeB = try XCTUnwrap(controller.row(for: AntigravityStatusFixture.sessionB))

        let sendUnselectedB = try XCTUnwrap(controller.sendIntent(
            for: AntigravityStatusFixture.sessionB,
            projectId: "proj-beta",
            message: "Must not modify B when A is selected",
            deliveryMode: .auto,
            requestId: "req-send-unselected-b"
        ))
        let unselectedRes = await controller.execute(sendUnselectedB, admitted: { true })
        XCTAssertEqual(unselectedRes, .unconfirmed)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "guard.selectionMismatch")
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA), beforeA)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionB), beforeB)

        let beforeCreated = try XCTUnwrap(controller.row(for: createdKey))
        let sendSelectedA = try XCTUnwrap(controller.sendIntent(
            for: AntigravityStatusFixture.sessionA,
            projectId: "proj-alpha",
            message: "Start session A",
            deliveryMode: .auto,
            requestId: "req-send-selected-a"
        ))
        let sendARes = await controller.execute(sendSelectedA, admitted: { true })
        XCTAssertEqual(sendARes, .started)
        XCTAssertEqual(controller.lastCommandReply?.delivery, .started)
        XCTAssertEqual(controller.lastCommandReply?.inFlightSteering, false)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.phase, .running)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.invocationNum, 1)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.transitionCount, 2)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionB), beforeB)
        XCTAssertEqual(controller.row(for: createdKey), beforeCreated)

        let afterASend = try XCTUnwrap(controller.row(for: AntigravityStatusFixture.sessionA))
        let missingKey = AntigravitySessionKey(
            conversationId: "conv-nonexistent",
            workspacePath: AntigravityStatusFixture.workspaceA
        )
        let sendMissing = try XCTUnwrap(controller.sendIntent(
            for: missingKey,
            projectId: "proj-alpha",
            message: "Missing target must not redirect",
            deliveryMode: .auto,
            requestId: "req-send-missing"
        ))
        let missingRes = await controller.execute(sendMissing, admitted: { true })
        XCTAssertEqual(missingRes, .unconfirmed)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "guard.targetNotFound")
        XCTAssertEqual(controller.selected, AntigravityStatusFixture.sessionA)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA), afterASend)
        XCTAssertEqual(controller.rows.count, 3)

        await controller.shutdown()
    }

    func testRunningSessionSteeringVsNextInvocationQueueingAndTimeoutReconcileWithoutResend() async throws {
        let fixture = AntigravityStatusFixture()
        let controller = AntigravityStatusController(client: fixture, pollNanoseconds: 60_000_000_000)

        try await fixture.ingest(AntigravityHookEvent(
            conversationId: AntigravityStatusFixture.sessionA.conversationId,
            workspacePath: AntigravityStatusFixture.sessionA.workspacePath,
            eventType: .preInvocation,
            modelName: "gemini-2.5-pro",
            invocationNum: 1,
            stepIdx: 1,
            executionNum: 1,
            toolName: nil,
            terminationReason: nil,
            hasError: false,
            fullyIdle: false,
            eventId: nil,
            title: "Running Session A",
            projectId: "proj-alpha"
        ))

        controller.setEnabled(true)
        for _ in 0..<200 {
            if controller.state == .ready { break }
            try await Task.sleep(for: .milliseconds(5))
        }
        _ = await controller.open(try XCTUnwrap(controller.selection(for: AntigravityStatusFixture.sessionA)), admitted: { true })
        XCTAssertEqual(controller.selected, AntigravityStatusFixture.sessionA)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.phase, .running)

        let steerAttemptDefault = try XCTUnwrap(controller.sendIntent(
            for: AntigravityStatusFixture.sessionA,
            projectId: "proj-alpha",
            message: "Follow-up while running",
            deliveryMode: .steer,
            requestId: "req-running-default-1"
        ))
        let resDefault = await controller.execute(steerAttemptDefault, admitted: { true })
        XCTAssertEqual(resDefault, .queued)
        XCTAssertEqual(controller.lastCommandReply?.delivery, .nextInvocation)
        XCTAssertEqual(controller.lastCommandReply?.inFlightSteering, false)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "agentapi.steerUnsupportedQueuedNextInvocation")
        XCTAssertEqual(controller.rows.count, 1)

        await fixture.setInFlightSteerEnabled(true)
        let steerEnabledCmd = try XCTUnwrap(controller.sendIntent(
            for: AntigravityStatusFixture.sessionA,
            projectId: "proj-alpha",
            message: "Steer active step",
            deliveryMode: .steer,
            requestId: "req-running-steered-2"
        ))
        let resSteered = await controller.execute(steerEnabledCmd, admitted: { true })
        XCTAssertEqual(resSteered, .steered)
        XCTAssertEqual(controller.lastCommandReply?.delivery, .steered)
        XCTAssertEqual(controller.lastCommandReply?.inFlightSteering, true)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "agentapi.sendMessageSteered")
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.stepIdx, 2)
        await fixture.setInFlightSteerEnabled(false)

        await fixture.setUnknownNextCommand(true)
        let timeoutCmd = try XCTUnwrap(controller.sendIntent(
            for: AntigravityStatusFixture.sessionA,
            projectId: "proj-alpha",
            message: "Timed out send",
            deliveryMode: .queue,
            requestId: "req-timeout-3"
        ))
        let resUnknown = await controller.execute(timeoutCmd, admitted: { true })
        XCTAssertEqual(resUnknown, .unknown)
        XCTAssertEqual(controller.pendingUnknownCommand?.operation.requestId, "req-timeout-3")
        let countsAfterTimeout = await fixture.commandCounts()
        XCTAssertEqual(countsAfterTimeout.sends, 3)

        XCTAssertNil(controller.sendIntent(
            for: AntigravityStatusFixture.sessionA,
            projectId: "proj-alpha",
            message: "Must be blocked",
            deliveryMode: .queue,
            requestId: "req-blocked-4"
        ))
        XCTAssertEqual(controller.lastCommandResult, .unknown)

        let queryCmd = try XCTUnwrap(controller.queryIntent(requestId: "req-timeout-3"))
        let resReconciled = await controller.execute(queryCmd, admitted: { true })
        XCTAssertEqual(resReconciled, .queued)
        XCTAssertEqual(controller.lastCommandReply?.reconciled, true)
        XCTAssertEqual(controller.lastCommandReply?.duplicate, true)
        XCTAssertNil(controller.pendingUnknownCommand)
        let countsAfterReconcile = await fixture.commandCounts()
        XCTAssertEqual(countsAfterReconcile.sends, 3)
        XCTAssertEqual(countsAfterReconcile.queries, 1)

        await controller.shutdown()
    }

    func testL3StopResolveAndSwitchModelTwoSessionIsolationStaleNoopAndFailClosed() async throws {
        let fixture = AntigravityStatusFixture()
        let controller = AntigravityStatusController(client: fixture, pollNanoseconds: 60_000_000_000)

        try await fixture.ingest(AntigravityHookEvent(
            conversationId: AntigravityStatusFixture.sessionA.conversationId,
            workspacePath: AntigravityStatusFixture.sessionA.workspacePath,
            eventType: .preInvocation,
            modelName: "gemini-2.5-pro",
            invocationNum: 2,
            stepIdx: 1,
            executionNum: 1,
            toolName: nil,
            terminationReason: nil,
            hasError: false,
            fullyIdle: false,
            eventId: nil,
            title: "Alpha L3",
            projectId: "proj-alpha"
        ))
        try await fixture.ingest(AntigravityHookEvent(
            conversationId: AntigravityStatusFixture.sessionB.conversationId,
            workspacePath: AntigravityStatusFixture.sessionB.workspacePath,
            eventType: .preInvocation,
            modelName: "gemini-2.5-flash",
            invocationNum: 1,
            stepIdx: 1,
            executionNum: 1,
            toolName: nil,
            terminationReason: nil,
            hasError: false,
            fullyIdle: false,
            eventId: nil,
            title: "Beta L3",
            projectId: "proj-beta"
        ))

        controller.setEnabled(true)
        for _ in 0..<200 {
            if controller.state == .ready { break }
            try await Task.sleep(for: .milliseconds(5))
        }
        _ = await controller.open(try XCTUnwrap(controller.selection(for: AntigravityStatusFixture.sessionA)), admitted: { true })
        XCTAssertEqual(controller.selected, AntigravityStatusFixture.sessionA)
        let bInitial = try XCTUnwrap(controller.row(for: AntigravityStatusFixture.sessionB))

        let stopUnselectedB = try XCTUnwrap(controller.stopIntent(
            for: AntigravityStatusFixture.sessionB,
            requestId: "unit-stop-unselected-b"
        ))
        let resStopUnselected = await controller.execute(stopUnselectedB, admitted: { true })
        XCTAssertEqual(resStopUnselected, .unconfirmed)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "guard.selectionMismatch")
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionB), bInitial)

        let stopStaleA = try XCTUnwrap(controller.stopIntent(
            for: AntigravityStatusFixture.sessionA,
            expectedInvocationNum: 1,
            expectedExecutionNum: 1,
            requestId: "unit-stop-stale-a"
        ))
        let resStopStale = await controller.execute(stopStaleA, admitted: { true })
        XCTAssertEqual(resStopStale, .noop)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "guard.stopTargetChanged")
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.phase, .running)

        let stopValidA = try XCTUnwrap(controller.stopIntent(
            for: AntigravityStatusFixture.sessionA,
            expectedInvocationNum: 2,
            expectedExecutionNum: 1,
            requestId: "unit-stop-valid-a"
        ))
        let resStopValid = await controller.execute(stopValidA, admitted: { true })
        XCTAssertEqual(resStopValid, .stopped)
        XCTAssertEqual(controller.lastCommandReply?.delivery, .canceled)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.phase, .canceled)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.fullyIdle, true)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.terminationReason, "USER_CANCELED")
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionB), bInitial)

        await fixture.setPendingInteraction(
            for: AntigravityStatusFixture.sessionA,
            pending: AntigravityPendingInteraction(
                interactionId: "unit-q-1",
                interactionType: .question,
                stepIndex: 3,
                optionIds: ["opt-a", "opt-b"]
            )
        )
        await fixture.setPendingInteraction(
            for: AntigravityStatusFixture.sessionB,
            pending: AntigravityPendingInteraction(
                interactionId: "unit-perm-b",
                interactionType: .filePermission,
                stepIndex: 2
            )
        )
        await controller.refresh()

        let badQuestionApprove = try XCTUnwrap(controller.resolveIntent(
            for: AntigravityStatusFixture.sessionA,
            interactionId: "unit-q-1",
            interactionType: .question,
            decision: .allow,
            requestId: "unit-q-bad-allow"
        ))
        let resBadQuestion = await controller.execute(badQuestionApprove, admitted: { true })
        XCTAssertEqual(resBadQuestion, .unconfirmed)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "guard.questionCannotApproveAsPermission")
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.pendingInteraction?.interactionId, "unit-q-1")

        let goodQuestionAnswer = try XCTUnwrap(controller.resolveIntent(
            for: AntigravityStatusFixture.sessionA,
            interactionId: "unit-q-1",
            interactionType: .question,
            decision: .answer,
            selectedOptionIds: ["opt-a"],
            requestId: "unit-q-answer"
        ))
        let resGoodQuestion = await controller.execute(goodQuestionAnswer, admitted: { true })
        XCTAssertEqual(resGoodQuestion, .resolved)
        XCTAssertEqual(controller.lastCommandReply?.delivery, .answer)
        XCTAssertNil(controller.row(for: AntigravityStatusFixture.sessionA)?.pendingInteraction)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionB)?.pendingInteraction?.interactionId, "unit-perm-b")

        await fixture.setPendingInteraction(
            for: AntigravityStatusFixture.sessionA,
            pending: AntigravityPendingInteraction(
                interactionId: "unit-exp-1",
                interactionType: .runCommand,
                stepIndex: 4
            )
        )
        await controller.refresh()
        let expiredCmd = try XCTUnwrap(controller.resolveIntent(
            for: AntigravityStatusFixture.sessionA,
            interactionId: "unit-exp-1",
            interactionType: .runCommand,
            decision: .allow,
            requestId: "unit-exp-allow"
        ))
        await fixture.expirePendingInteraction(for: AntigravityStatusFixture.sessionA, interactionId: "unit-exp-1")
        let resExpired = await controller.execute(expiredCmd, admitted: { true })
        XCTAssertEqual(resExpired, .noop)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "guard.interactionAlreadyResolved")

        let unsupportedModelCmd = try XCTUnwrap(controller.switchModelIntent(
            for: AntigravityStatusFixture.sessionA,
            modelName: "gemini-2.5-flash-lite",
            thinkingLevel: "high",
            requestId: "unit-model-unsupported"
        ))
        let resUnsupportedModel = await controller.execute(unsupportedModelCmd, admitted: { true })
        XCTAssertEqual(resUnsupportedModel, .unconfirmed)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "guard.unsupportedThinkingLevel")
        let countsAfterUnsupported = await fixture.commandCounts()
        XCTAssertEqual(countsAfterUnsupported.modelSwitches, 0)

        let validModelCmd = try XCTUnwrap(controller.switchModelIntent(
            for: AntigravityStatusFixture.sessionA,
            modelName: "gemini-2.5-flash",
            thinkingLevel: "medium",
            requestId: "unit-model-valid"
        ))
        let resValidModel = await controller.execute(validModelCmd, admitted: { true })
        XCTAssertEqual(resValidModel, .modelSwitched)
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.modelName, "gemini-2.5-flash")
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionA)?.thinkingLevel, "medium")
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionB)?.modelName, "gemini-2.5-flash")
        XCTAssertEqual(controller.row(for: AntigravityStatusFixture.sessionB)?.thinkingLevel, "high")

        await fixture.setModelReadbackOverride(modelName: "gemini-2.5-pro", thinkingLevel: "low")
        let mismatchModelCmd = try XCTUnwrap(controller.switchModelIntent(
            for: AntigravityStatusFixture.sessionA,
            modelName: "gemini-2.5-pro",
            thinkingLevel: "high",
            requestId: "unit-model-mismatch"
        ))
        let resMismatchModel = await controller.execute(mismatchModelCmd, admitted: { true })
        XCTAssertEqual(resMismatchModel, .unconfirmed)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "guard.modelReadbackMismatch")
        await fixture.setModelReadbackOverride(modelName: nil, thinkingLevel: nil)

        let stopBeforeCsrfFail = try XCTUnwrap(controller.stopIntent(
            for: AntigravityStatusFixture.sessionA,
            requestId: "unit-stop-csrf-fail"
        ))
        await fixture.setCsrfAuth(false)
        let resCsrfFail = await controller.execute(stopBeforeCsrfFail, admitted: { true })
        XCTAssertEqual(resCsrfFail, .denied)
        XCTAssertEqual(controller.state, .unavailable(.denied))

        await controller.shutdown()
    }
}
