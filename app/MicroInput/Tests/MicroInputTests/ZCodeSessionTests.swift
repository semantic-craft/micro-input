import XCTest
import Foundation
import Darwin
import MicroInputCore
@testable import MicroInput

private let zcodeA = ZCodeTaskAddress(workspacePath: "/workspace/a", workspaceIdentity: "a", taskId: "same-id")
private let zcodeB = ZCodeTaskAddress(workspacePath: "/workspace/b", workspaceIdentity: "b", taskId: "same-id")
private func zcodeSnapshot(rows: [ZCodeTaskAddress] = [zcodeA, zcodeB], generation: UInt64 = 1,
                           selected: ZCodeTaskAddress? = nil,
                           liveStatus: String = "idle",
                           executionId: String? = nil) -> ZCodeSessionSnapshot {
    ZCodeSessionSnapshot(instanceId: "instance-a", generation: generation, scopes: [zcodeA.scope, zcodeB.scope],
                         rows: rows.map { ZCodeTaskRow(address: $0, title: "Same name", liveStatus: liveStatus, executionId: executionId) }, selected: selected)
}

private actor ZCodeTestPeer: ZCodeSessionServing {
    struct SettledCommand {
        let status: ZCodeCommandResult
        let selected: ZCodeTaskAddress?
        let delivery: ZCodeDelivery?
        let executionId: String?
        let reasonCode: String?
    }
    var value = zcodeSnapshot()
    var reply: ZCodeOpenReply?
    var failed = false
    var connections = 0
    var reads = 0
    var opens: [ZCodeTaskAddress] = []
    var creates: [ZCodeCreateIntent] = []
    var sends: [ZCodeSendIntent] = []
    var stops: [ZCodeStopIntent] = []
    var queries: [ZCodeQueryIntent] = []
    var commandIdsSeen: [String] = []
    var settledCommands: [String: SettledCommand] = [:]
    var heldCommands: [String: SettledCommand] = [:]
    var forceQueueForGuide = false
    var timeoutNextCommand = false
    var holdUnknownOnQuery = false
    var activeExecutionByAddress: [ZCodeTaskAddress: String] = [:]
    var held = false
    var waiting: CheckedContinuation<Void, Never>?
    func connect() async throws -> ZCodeSessionSnapshot { connections += 1; return value }
    func read() async throws -> ZCodeSessionSnapshot {
        reads += 1
        let captured = value
        if held { await withCheckedContinuation { waiting = $0 } }
        if failed { throw ZCodeFailure.unavailable }
        return captured
    }
    func open(_ selection: ZCodeSelection) async throws -> ZCodeOpenReply {
        if failed { throw ZCodeFailure.unavailable }
        guard value.rows.contains(where: { $0.address == selection.address }) else { throw ZCodeFailure.stale }
        opens.append(selection.address)
        value = ZCodeSessionSnapshot(instanceId: value.instanceId, generation: value.generation,
                                     scopes: value.scopes, rows: value.rows, selected: selection.address)
        return reply ?? ZCodeOpenReply(instanceId: value.instanceId, generation: value.generation,
                                      status: .confirmed, selected: selection.address)
    }
    func create(_ intent: ZCodeCreateIntent) async throws -> ZCodeCommandReply {
        if failed { throw ZCodeFailure.unavailable }
        let sessionId = intent.sessionId ?? "created-\(intent.commandId)"
        let key = "\(sessionId)::\(intent.commandId)"
        commandIdsSeen.append(intent.commandId)
        if let settled = settledCommands[key] {
            return ZCodeCommandReply(instanceId: value.instanceId, generation: value.generation,
                                     status: settled.status, selected: settled.selected,
                                     commandId: intent.commandId, duplicate: true,
                                     delivery: settled.delivery, executionId: settled.executionId,
                                     reasonCode: settled.reasonCode)
        }
        creates.append(intent)
        let created = ZCodeTaskAddress(workspacePath: intent.scope.workspacePath,
                                       workspaceIdentity: intent.scope.workspaceIdentity,
                                       sourceServerId: intent.scope.sourceServerId,
                                       remoteSessionId: intent.scope.remoteSessionId,
                                       taskId: sessionId)
        let hasText = intent.firstInputText?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
        let execId = hasText ? "exec-create-\(creates.count)" : nil
        if let execId { activeExecutionByAddress[created] = execId }
        let newRow = ZCodeTaskRow(address: created, title: "New Session", liveStatus: hasText ? "running" : "idle", executionId: execId)
        value = ZCodeSessionSnapshot(instanceId: value.instanceId, generation: value.generation,
                                     scopes: value.scopes, rows: value.rows + [newRow], selected: created)
        let entry = SettledCommand(status: .confirmed, selected: created, delivery: hasText ? .startNow : nil, executionId: execId, reasonCode: nil)
        if timeoutNextCommand {
            timeoutNextCommand = false
            if holdUnknownOnQuery { heldCommands[key] = entry } else { settledCommands[key] = entry }
            throw ZCodeFailure.transportTimeout
        }
        settledCommands[key] = entry
        return ZCodeCommandReply(instanceId: value.instanceId, generation: value.generation,
                                 status: .confirmed, selected: created, commandId: intent.commandId,
                                 duplicate: false, delivery: entry.delivery, executionId: execId)
    }
    func send(_ intent: ZCodeSendIntent) async throws -> ZCodeCommandReply {
        if failed { throw ZCodeFailure.unavailable }
        let address = intent.selection.address
        let key = "\(address.taskId)::\(intent.commandId)"
        commandIdsSeen.append(intent.commandId)
        if let settled = settledCommands[key] {
            return ZCodeCommandReply(instanceId: value.instanceId, generation: value.generation,
                                     status: settled.status, selected: settled.selected,
                                     commandId: intent.commandId, duplicate: true,
                                     delivery: settled.delivery, executionId: settled.executionId,
                                     reasonCode: settled.reasonCode)
        }
        sends.append(intent)
        let row = value.rows.first(where: { $0.address == address })
        let isRunning = row?.liveStatus == "running" || activeExecutionByAddress[address] != nil
        let delivery: ZCodeDelivery
        let status: ZCodeCommandResult
        if !isRunning {
            delivery = .startNow
            status = .started
            activeExecutionByAddress[address] = "exec-\(sends.count)"
        } else if intent.requestedDelivery == .guide && !forceQueueForGuide {
            delivery = .guide
            status = .steered
        } else {
            delivery = .queue
            status = .queued
        }
        let execId = activeExecutionByAddress[address]
        let entry = SettledCommand(status: status, selected: address, delivery: delivery, executionId: execId, reasonCode: nil)
        if timeoutNextCommand {
            timeoutNextCommand = false
            if holdUnknownOnQuery { heldCommands[key] = entry } else { settledCommands[key] = entry }
            throw ZCodeFailure.transportTimeout
        }
        settledCommands[key] = entry
        return ZCodeCommandReply(instanceId: value.instanceId, generation: value.generation,
                                 status: status, selected: address, commandId: intent.commandId,
                                 duplicate: false, delivery: delivery, executionId: execId)
    }
    func stop(_ intent: ZCodeStopIntent) async throws -> ZCodeCommandReply {
        if failed { throw ZCodeFailure.unavailable }
        let address = intent.selection.address
        let key = "\(address.taskId)::\(intent.commandId)"
        commandIdsSeen.append(intent.commandId)
        if let settled = settledCommands[key] {
            return ZCodeCommandReply(instanceId: value.instanceId, generation: value.generation,
                                     status: settled.status, selected: settled.selected,
                                     commandId: intent.commandId, duplicate: true,
                                     delivery: settled.delivery, executionId: settled.executionId,
                                     reasonCode: settled.reasonCode)
        }
        stops.append(intent)
        let currentExec = activeExecutionByAddress[address]
        if currentExec == nil || currentExec != intent.expectedExecutionId {
            let entry = SettledCommand(status: .noop, selected: address, delivery: nil, executionId: currentExec, reasonCode: "guard.stopTargetChanged")
            settledCommands[key] = entry
            return ZCodeCommandReply(instanceId: value.instanceId, generation: value.generation,
                                     status: .noop, selected: address, commandId: intent.commandId,
                                     duplicate: false, executionId: currentExec, reasonCode: "guard.stopTargetChanged")
        }
        activeExecutionByAddress.removeValue(forKey: address)
        let entry = SettledCommand(status: .stopped, selected: address, delivery: nil, executionId: intent.expectedExecutionId, reasonCode: nil)
        settledCommands[key] = entry
        return ZCodeCommandReply(instanceId: value.instanceId, generation: value.generation,
                                 status: .stopped, selected: address, commandId: intent.commandId,
                                 duplicate: false, executionId: intent.expectedExecutionId)
    }
    func query(_ intent: ZCodeQueryIntent) async throws -> ZCodeCommandReply {
        if failed { throw ZCodeFailure.unavailable }
        queries.append(intent)
        let sessionId = intent.target?.taskId ?? "created-\(intent.commandId)"
        let key = "\(sessionId)::\(intent.commandId)"
        if holdUnknownOnQuery && heldCommands[key] != nil {
            return ZCodeCommandReply(instanceId: value.instanceId, generation: value.generation,
                                     status: .unknown, selected: value.selected, commandId: intent.commandId)
        }
        guard let settled = settledCommands[key] else {
            return ZCodeCommandReply(instanceId: value.instanceId, generation: value.generation,
                                     status: .unknown, selected: value.selected, commandId: intent.commandId)
        }
        return ZCodeCommandReply(instanceId: value.instanceId, generation: value.generation,
                                 status: settled.status, selected: settled.selected,
                                 commandId: intent.commandId, duplicate: true,
                                 delivery: settled.delivery, executionId: settled.executionId,
                                 reasonCode: settled.reasonCode)
    }
    func close() async {}
    func setValue(_ value: ZCodeSessionSnapshot) { self.value = value }
    func setReply(_ reply: ZCodeOpenReply?) { self.reply = reply }
    func setFailed() { failed = true }
    func setActiveExecution(_ address: ZCodeTaskAddress, executionId: String?) {
        if let executionId { activeExecutionByAddress[address] = executionId }
        else { activeExecutionByAddress.removeValue(forKey: address) }
        let updatedRows = value.rows.map { row -> ZCodeTaskRow in
            guard row.address == address else { return row }
            return ZCodeTaskRow(address: row.address, title: row.title,
                                liveStatus: executionId == nil ? "idle" : "running",
                                executionId: executionId)
        }
        value = ZCodeSessionSnapshot(instanceId: value.instanceId, generation: value.generation,
                                     scopes: value.scopes, rows: updatedRows, selected: value.selected)
    }
    func setForceQueueForGuide(_ flag: Bool) { forceQueueForGuide = flag }
    func configureTimeoutNextCommand(holdUnknown: Bool) {
        timeoutNextCommand = true
        holdUnknownOnQuery = holdUnknown
    }
    func settleHeldCommands() {
        for (key, value) in heldCommands { settledCommands[key] = value }
        heldCommands.removeAll()
        holdUnknownOnQuery = false
    }
    func hold() { held = true }
    func release() { held = false; waiting?.resume(); waiting = nil }
    func hasWaiter() -> Bool { waiting != nil }
    func counts() -> (Int, Int, Int) { (connections, reads, opens.count) }
    func commandStats() -> (creates: Int, sends: Int, stops: Int, queries: Int, commandIds: [String], activeB: String?) {
        (creates.count, sends.count, stops.count, queries.count, commandIdsSeen, activeExecutionByAddress[zcodeB])
    }
    func opened() -> [ZCodeTaskAddress] { opens }
}

@MainActor
final class ZCodeSessionTests: XCTestCase {
    func testDisabledConnectionAndOutOfScopeSelectionHaveZeroCalls() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        await controller.refresh()
        let disabled = await peer.counts()
        XCTAssertEqual(disabled.0 + disabled.1 + disabled.2, 0)
        controller.setEnabled(true); await controller.refresh()
        let valid = try XCTUnwrap(controller.selection(for: zcodeA))
        let invalid = ZCodeSelection(address: ZCodeTaskAddress(workspacePath: "/outside", taskId: "same-id"),
                                     instanceId: valid.instanceId, generation: valid.generation, localEpoch: valid.localEpoch)
        let before = await peer.counts()
        let result = await controller.open(invalid, admitted: { true })
        let after = await peer.counts()
        XCTAssertEqual(result, .stale)
        XCTAssertEqual(before.1, after.1)
        XCTAssertEqual(after.2, 0)
        await controller.shutdown()
    }
    func testSameTitleAndSameTaskIDRemainDistinctAcrossWorkspaces() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        XCTAssertEqual(controller.rows.count, 2)
        for address in [zcodeB, zcodeA] {
            let selection = try XCTUnwrap(controller.selection(for: address))
            let result = await controller.open(selection, admitted: { true })
            XCTAssertEqual(result, .confirmed)
            XCTAssertEqual(controller.selected, address)
        }
        let opened = await peer.opened()
        XCTAssertEqual(opened, [zcodeB, zcodeA])
        await controller.shutdown()
    }
    func testWrongSelectedCannotConfirm() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        await peer.setReply(ZCodeOpenReply(instanceId: "instance-a", generation: 1, status: .confirmed, selected: zcodeB))
        let result = await controller.open(controller.selection(for: zcodeA), admitted: { true })
        XCTAssertEqual(result, .unconfirmed)
        XCTAssertNil(controller.selected)
        await controller.shutdown()
    }
    func testGenerationChangeAndDisconnectionClearTargetsAndRejectOldSelection() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let old = try XCTUnwrap(controller.selection(for: zcodeA))
        await peer.setValue(zcodeSnapshot(generation: 2))
        await controller.refresh()
        XCTAssertEqual(controller.state, .unavailable(.stale))
        XCTAssertTrue(controller.rows.isEmpty)
        let stale = await controller.open(old, admitted: { true })
        XCTAssertEqual(stale, .stale)
        controller.setEnabled(true); await controller.refresh()
        XCTAssertEqual(controller.state, .ready)
        let rejected = await controller.open(old, admitted: { true })
        XCTAssertEqual(rejected, .stale)
        await peer.setFailed(); await controller.refresh()
        XCTAssertEqual(controller.state, .unavailable(.unavailable))
        XCTAssertTrue(controller.rows.isEmpty)
        let counts = await peer.counts()
        XCTAssertEqual(counts.2, 0)
        await controller.shutdown()
    }
    func testDeletionDoesNotOpenAnotherSameNameTask() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let selection = try XCTUnwrap(controller.selection(for: zcodeA))
        await peer.setValue(zcodeSnapshot(rows: [zcodeB]))
        let result = await controller.open(selection, admitted: { true })
        XCTAssertEqual(result, .stale)
        let opened = await peer.opened()
        XCTAssertTrue(opened.isEmpty)
        XCTAssertTrue(controller.rows.isEmpty)
        await controller.shutdown()
    }
    func testDisableDiscardsLateMetadata() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        await peer.hold()
        let pending = Task { await controller.refresh() }
        for _ in 0..<1000 {
            if await peer.hasWaiter() { break }
            await Task.yield()
        }
        let started = await peer.hasWaiter()
        XCTAssertTrue(started)
        controller.setEnabled(false)
        await peer.release(); await pending.value
        XCTAssertEqual(controller.state, .disabled)
        XCTAssertTrue(controller.rows.isEmpty)
        XCTAssertNil(controller.selected)
        await controller.shutdown()
    }
    func testScopeViolationAndDuplicateIdentityRejectWholeSnapshot() async {
        for rows in [[zcodeA, zcodeA], [ZCodeTaskAddress(workspacePath: "/outside", taskId: "same-id")]] {
            let peer = ZCodeTestPeer()
            await peer.setValue(zcodeSnapshot(rows: rows))
            let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
            controller.setEnabled(true); await controller.refresh()
            XCTAssertEqual(controller.state, .unavailable(.invalidReply))
            XCTAssertTrue(controller.rows.isEmpty)
            await controller.shutdown()
        }
    }
    func testDeniedHostAdmissionDoesNotSend() async {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let result = await controller.open(controller.selection(for: zcodeA), admitted: { false })
        XCTAssertEqual(result, .stale)
        let counts = await peer.counts()
        XCTAssertEqual(counts.2, 0)
        await controller.shutdown()
    }
    func testStoreQueueUsesLeaseAndRedactsFullIdentity() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let core = MicroInputCore.MicroInput()
        let executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core,
                          documentURL: directory.appendingPathComponent("workbench.json"),
                          actionExecutor: executor, zcodeSessions: controller)
        await store.loadWorkbench(); await store.installCodingAgentPresets()
        let profile = try XCTUnwrap(store.profile?.id.text)
        await store.setHostActiveProfile(profile)
        controller.setEnabled(true); await controller.refresh()
        let selection = try XCTUnwrap(controller.selection(for: zcodeB))
        XCTAssertTrue(store.openZCodeSession(selection))
        await store.hostExecutionTask?.value
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "confirmed")
        XCTAssertEqual(store.lastHostActionReceipt?.actionType, "zcode_open")
        XCTAssertEqual(core.hostExecutionsPending(), 0)
        XCTAssertTrue(executor.receipts.isEmpty)
        let receipt = try JSONEncoder().encode(XCTUnwrap(store.lastHostActionReceipt))
        let text = String(decoding: receipt, as: UTF8.self)
        for value in ["same-id", "Same name", "/workspace/", "instance-a"] { XCTAssertFalse(text.contains(value)) }
        controller.setEnabled(false)
        XCTAssertTrue(store.openZCodeSession(selection))
        await store.hostExecutionTask?.value
        let opened = await peer.opened()
        XCTAssertEqual(opened, [zcodeB])
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "stale")
        XCTAssertEqual(core.hostExecutionsPending(), 0)
        await store.shutdown()
    }
    func testOpenResponseFromAnotherInstanceInvalidatesConnection() async {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        await peer.setReply(ZCodeOpenReply(instanceId: "replacement", generation: 1, status: .confirmed, selected: zcodeA))
        let result = await controller.open(controller.selection(for: zcodeA), admitted: { true })
        XCTAssertEqual(result, .stale)
        XCTAssertEqual(controller.state, .unavailable(.stale))
        XCTAssertTrue(controller.rows.isEmpty)
        await controller.shutdown()
    }
    func testTwoSessionsOnlyAffectSelectedTargetAndLateStopIsNoop() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let selectionA = try XCTUnwrap(controller.selection(for: zcodeA))
        let selectionB = try XCTUnwrap(controller.selection(for: zcodeB))
        let openB = await controller.open(selectionB, admitted: { true })
        XCTAssertEqual(openB, .confirmed)
        XCTAssertEqual(controller.selected, zcodeB)

        // Attempting to send to unselected zcodeA is rejected locally without touching the peer
        let sendToUnselected = ZCodeSendIntent(selection: selectionA, commandId: "cmd-unselected", clientId: "client-1", text: "wrong-target")
        let unselectedResult = await controller.send(sendToUnselected, admitted: { true })
        XCTAssertEqual(unselectedResult, .unconfirmed)
        let statsAfterUnselected = await peer.commandStats()
        XCTAssertEqual(statsAfterUnselected.sends, 0)

        // Sending to selected zcodeB starts execution
        let sendToB = try XCTUnwrap(controller.sendIntent(for: zcodeB, text: "hello-b", commandId: "cmd-send-b", clientId: "client-1"))
        let sendResult = await controller.send(sendToB, admitted: { true })
        XCTAssertEqual(sendResult, .started)

        // Duplicate commandId does not double-send
        let dupResult = await controller.send(sendToB, admitted: { true })
        XCTAssertEqual(dupResult, .started)
        XCTAssertEqual(controller.lastCommandReply?.duplicate, true)
        let statsAfterDup = await peer.commandStats()
        XCTAssertEqual(statsAfterDup.sends, 1)

        // Execution rotates from exec-1 to exec-2; late stop carrying exec-1 is a noop and leaves exec-2 running
        await peer.setActiveExecution(zcodeB, executionId: "exec-2")
        await controller.refresh()
        let lateStop = try XCTUnwrap(controller.stopIntent(for: zcodeB, expectedExecutionId: "exec-1", commandId: "cmd-stop-old", clientId: "client-1"))
        let lateStopResult = await controller.stop(lateStop, admitted: { true })
        XCTAssertEqual(lateStopResult, .noop)
        XCTAssertEqual(controller.lastCommandReply?.reasonCode, "guard.stopTargetChanged")
        let statsAfterLateStop = await peer.commandStats()
        XCTAssertEqual(statsAfterLateStop.activeB, "exec-2")

        // Stop carrying current exec-2 stops execution
        let currentStop = try XCTUnwrap(controller.stopIntent(for: zcodeB, expectedExecutionId: "exec-2", commandId: "cmd-stop-current", clientId: "client-1"))
        let currentStopResult = await controller.stop(currentStop, admitted: { true })
        XCTAssertEqual(currentStopResult, .stopped)
        let statsAfterCurrentStop = await peer.commandStats()
        XCTAssertNil(statsAfterCurrentStop.activeB)
        await controller.shutdown()
    }
    func testCreateBindsWorkspaceAndInFlightSendSeparatesSteeringFromQueue() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()

        // Create session in workspace A and read back new ID
        let createIntent = try XCTUnwrap(controller.createIntent(for: zcodeA.scope, commandId: "cmd-create-a", clientId: "client-1", sessionId: "new-session-a", firstInputText: "init"))
        let createResult = await controller.create(createIntent, admitted: { true })
        XCTAssertEqual(createResult, .confirmed)
        let expectedCreated = ZCodeTaskAddress(workspacePath: zcodeA.workspacePath, workspaceIdentity: zcodeA.workspaceIdentity, taskId: "new-session-a")
        XCTAssertEqual(controller.selected, expectedCreated)
        XCTAssertTrue(controller.rows.contains(where: { $0.address == expectedCreated }))

        // Duplicate create with same commandId does not create a second session
        let dupCreateResult = await controller.create(createIntent, admitted: { true })
        XCTAssertEqual(dupCreateResult, .confirmed)
        XCTAssertEqual(controller.lastCommandReply?.duplicate, true)
        let statsAfterCreate = await peer.commandStats()
        XCTAssertEqual(statsAfterCreate.creates, 1)

        // While running, send with guide -> steered; send when guide falls back to queue -> queued; zero extra createSession calls
        let steerIntent = try XCTUnwrap(controller.sendIntent(for: expectedCreated, text: "steer-1", requestedDelivery: .guide, commandId: "cmd-steer-1", clientId: "client-1"))
        let steerResult = await controller.send(steerIntent, admitted: { true })
        XCTAssertEqual(steerResult, .steered)
        XCTAssertEqual(controller.lastCommandReply?.delivery, .guide)

        await peer.setForceQueueForGuide(true)
        let queuedIntent = try XCTUnwrap(controller.sendIntent(for: expectedCreated, text: "queue-1", requestedDelivery: .guide, commandId: "cmd-queue-1", clientId: "client-1"))
        let queuedResult = await controller.send(queuedIntent, admitted: { true })
        XCTAssertEqual(queuedResult, .queued)
        XCTAssertEqual(controller.lastCommandReply?.delivery, .queue)
        let statsAfterInflight = await peer.commandStats()
        XCTAssertEqual(statsAfterInflight.creates, 1)
        await controller.shutdown()
    }
    func testTimeoutOutcomeLatchesSameCommandIdAndNeverReplaysWithNewId() async throws {
        let peer = ZCodeTestPeer()
        let controller = ZCodeSessionController(client: peer, pollNanoseconds: 60_000_000_000)
        controller.setEnabled(true); await controller.refresh()
        let selectionB = try XCTUnwrap(controller.selection(for: zcodeB))
        _ = await controller.open(selectionB, admitted: { true })

        await peer.configureTimeoutNextCommand(holdUnknown: true)
        let originalSend = try XCTUnwrap(controller.sendIntent(for: zcodeB, text: "once-only", commandId: "cmd-timeout-1", clientId: "client-1"))
        let timeoutResult = await controller.send(originalSend, admitted: { true })
        XCTAssertEqual(timeoutResult, .unknown)
        XCTAssertEqual(controller.latchedCommand(forAddress: zcodeB)?.commandId, "cmd-timeout-1")

        // Attempting to build a new sendIntent with a rotated commandId while latched keeps the original commandId
        let retryCandidate = try XCTUnwrap(controller.sendIntent(for: zcodeB, text: "once-only", commandId: "cmd-rotated-should-not-use", clientId: "client-1"))
        XCTAssertEqual(retryCandidate.commandId, "cmd-timeout-1")

        await peer.settleHeldCommands()
        let reconciled = await controller.send(retryCandidate, admitted: { true })
        XCTAssertEqual(reconciled, .started)
        XCTAssertNil(controller.latchedCommand(forAddress: zcodeB))
        let stats = await peer.commandStats()
        XCTAssertEqual(stats.sends, 1)
        XCTAssertFalse(stats.commandIds.contains("cmd-rotated-should-not-use"))
        await controller.shutdown()
    }
}

final class ZCodeGrantTests: XCTestCase {
    func testGrantRequiresPrivateRegularFileAndPrivateParent() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("zcode-grant-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: folder) }
        let file = folder.appendingPathComponent("grant.json")
        let grant = ZCodeGrant(version: 1, socketPath: "/private/socket", token: String(repeating: "a", count: 32),
                               instanceId: "instance-a", generation: 1, scopes: [zcodeA.scope])
        try JSONEncoder().encode(grant).write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        XCTAssertEqual(try ZCodeGrant.load(file), grant)
        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: file.path)
        XCTAssertThrowsError(try ZCodeGrant.load(file))
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder.path)
        XCTAssertThrowsError(try ZCodeGrant.load(file))
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: folder.path)
        let link = folder.appendingPathComponent("link.json")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: file)
        XCTAssertThrowsError(try ZCodeGrant.load(link))
    }
}
