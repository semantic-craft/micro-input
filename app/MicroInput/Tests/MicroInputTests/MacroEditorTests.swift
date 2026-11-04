import XCTest
import MicroInputCore
@testable import MicroInput

final class MacroEditorTests: XCTestCase {
    @MainActor func testStructuredMacroEditsPersistThroughRealBridgeAndUndoAsOneChange() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("workbench.json")
        let core = MicroInputCore.MicroInput()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: url)
        await store.loadWorkbench()
        let template = try XCTUnwrap(store.catalog.first { $0.id == "local-macro" })
        var editor = try XCTUnwrap(MacroEditorDraft(action: template))
        editor.insert(kind: "delay", at: 1)
        editor.update(index: 1, milliseconds: "125")
        editor.duplicate(index: 1)
        editor.move(index: 2, by: -1)
        editor.remove(index: 2)
        let edited = try editor.action(named: "My local macro")
        await store.saveAction(edited)
        XCTAssertEqual(store.draft.revision, 1)
        XCTAssertEqual(store.draft.edits[store.slotKey]?.name, "My local macro")
        let reopened = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await reopened.loadWorkbench()
        let saved = try XCTUnwrap(reopened.draft.edits[reopened.slotKey])
        let restored = try XCTUnwrap(MacroEditorDraft(action: saved))
        XCTAssertEqual(restored.steps.map(\.kind), ["press", "delay", "release"])
        XCTAssertEqual(restored.steps[1].milliseconds, "125")
        await store.undoDraft()
        XCTAssertNil(store.draft.edits[store.slotKey])
        await store.undoDraft(redo: true)
        XCTAssertEqual(store.draft.edits[store.slotKey]?.name, "My local macro")
        await core.shutdown()
    }
}

extension MacroEditorTests {
    @MainActor func testVerifiedMacroRoutesThroughRealBridgeAndRecordingExecutorReportsSequence() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("workbench.json")
        let executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()
        await store.setHostExecutionEnabled(true)
        await store.setHostActiveProfile(store.profile?.id.text)
        let template = try XCTUnwrap(store.catalog.first { $0.id == "local-macro" })
        await store.saveAction(template)
        XCTAssertTrue(store.draft.edits.keys.contains(store.slotKey))
        XCTAssertEqual(store.actionCapabilities[store.slotKey]?.execution_available, true)

        let rawAction = store.resolveInputActionForTesting(bindingId: "AG00")
        let actionJSON = try XCTUnwrap(rawAction)
        let payload = try JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(actionJSON.utf8))
        XCTAssertEqual(payload.action_type, "macro_sequence")
        XCTAssertEqual(payload.events?.map(\.event_type), ["press", "release"])
        XCTAssertEqual(payload.events?.compactMap(\.key_code), [0x00, 0x00])

        let executed = await store.executeHostAction(
            actionJSON: actionJSON,
            bindingId: "AG00",
            profileId: try XCTUnwrap(store.hostActiveProfileId)
        )
        let receipt = try XCTUnwrap(executed)
        XCTAssertTrue(receipt.succeeded)
        XCTAssertEqual(receipt.actionType, "macro_sequence")
        XCTAssertEqual(receipt.detail, "steps=2 keys=2 delays=0")
        XCTAssertEqual(executor.receipts.last, receipt)
    }

    @MainActor func testCancelledMacroSequenceReleasesPressedKeys() async throws {
        let executor = LiveHostActionExecutor()
        var posted: [(keyCode: UInt16, down: Bool)] = []
        executor.macroAccessibilityChecker = { true }
        executor.macroKeyEventPoster = { keyCode, down in
            posted.append((keyCode, down))
            return true
        }
        let actionJSON = """
        {"action_type":"macro_sequence","events":[
          {"event_type":"press","key":"a","key_code":1},
          {"event_type":"delay","ms":300},
          {"event_type":"release","key":"a","key_code":1}
        ]}
        """
        let execution = Task { await executor.execute(actionJSON: actionJSON, bindingId: "AG12", profileId: "macro-profile") }
        try await Task.sleep(nanoseconds: 20_000_000)
        execution.cancel()
        let receipt = await execution.value

        XCTAssertFalse(receipt.succeeded)
        XCTAssertTrue(receipt.detail.contains("已释放"))
        XCTAssertEqual(posted.map(\.keyCode), [1, 1])
        XCTAssertEqual(posted.map(\.down), [true, false])
    }

    @MainActor func testStoreSleepCancelsRunningMacroAndReleasesHeldKeys() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("workbench.json")
        let executor = LiveHostActionExecutor()
        var posted: [(keyCode: UInt16, down: Bool)] = []
        executor.macroAccessibilityChecker = { true }
        executor.macroKeyEventPoster = { keyCode, down in
            posted.append((keyCode, down))
            return true
        }
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()
        await store.setHostExecutionEnabled(true)
        await store.setHostActiveProfile(store.profile?.id.text)
        let template = try XCTUnwrap(store.catalog.first { $0.id == "local-macro" })
        var editor = try XCTUnwrap(MacroEditorDraft(action: template))
        editor.insert(kind: "delay", at: 1)
        editor.update(index: 1, milliseconds: "500")
        await store.saveAction(try editor.action(named: "Sleep-cancelled macro"))

        let actionJSON = try XCTUnwrap(store.resolveInputActionForTesting(bindingId: "AG00"))
        let state = try XCTUnwrap(store.routing)
        XCTAssertTrue(store.enqueueHostAction(
            json: actionJSON,
            binding: "AG00",
            profile: try XCTUnwrap(store.hostActiveProfileId),
            state: state
        ))

        let started = Date()
        while posted.isEmpty, Date().timeIntervalSince(started) < 1 {
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertEqual(posted.map(\.down), [true])

        store.systemWillSleep()
        let released = Date()
        while posted.count < 2, Date().timeIntervalSince(released) < 0.1 {
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertEqual(posted.map(\.keyCode), [0x00, 0x00])
        XCTAssertEqual(posted.map(\.down), [true, false])
        await store.shutdown()
    }
}

extension MacroEditorTests {
    @MainActor func testSharedMacroBindingEditUnlinkAndGuardedDeleteThroughRealBridge() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("workbench.json")
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await store.loadWorkbench()
        let template = try XCTUnwrap(store.catalog.first { $0.id == "local-macro" })
        let saved = await store.saveMacro(template)
        XCTAssertTrue(saved)
        let id = try XCTUnwrap(store.macros.first?.id)
        store.slot = 1; await store.bindMacro(id)
        XCTAssertEqual(store.macros.first?.references.count, 2)
        var changed = template; changed.name = "Shared revision"
        let result = await store.saveMacro(changed, id: id)
        XCTAssertTrue(result)
        XCTAssertEqual(store.draft.revision, 3)
        XCTAssertEqual(store.draft.edits.values.filter { $0.name == "Shared revision" }.count, 2)
        await store.deleteMacro(id)
        XCTAssertEqual(store.draft.revision, 3)
        XCTAssertTrue(store.status.contains("logical-0"))
        let re = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await re.loadWorkbench()
        XCTAssertEqual(re.macros.first?.id, id)
        XCTAssertEqual(re.macros.first?.references.count, 2)
        await re.bindMacro(nil); re.slot = 1; await re.bindMacro(nil)
        XCTAssertEqual(re.macros.first?.references.count, 0)
        await re.deleteMacro(id)
        XCTAssertTrue(re.macros.isEmpty)
        await re.undoDraft()
        XCTAssertEqual(re.macros.first?.id, id)
        await re.undoDraft()
        XCTAssertEqual(re.macros.first?.references.count, 1)
    }
    func testEditorLocalUndoRestoresRemovedAndReorderedSteps() throws {
        let data = Data(#"{"id":"macro","name":"Local","category":"普通宏","detail":"local","payload":{"type":"local_macro","macro_sequence":{"format_version":1,"events":[{"type":"press","key":"a"},{"type":"release","key":"a"}]}}}"#.utf8)
        let action = try JSONDecoder().decode(CatalogAction.self, from: data)
        var editor = try XCTUnwrap(MacroEditorDraft(action: action))
        editor.insert(kind: "delay", at: 1)
        editor.move(index: 1, by: 1)
        XCTAssertEqual(editor.steps.map(\.kind), ["press", "release", "delay"])
        editor.restore()
        XCTAssertEqual(editor.steps.map(\.kind), ["press", "delay", "release"])
        editor.restore()
        XCTAssertEqual(editor.steps.map(\.kind), ["press", "release"])
        editor.restore(redo: true)
        XCTAssertEqual(editor.steps.map(\.kind), ["press", "delay", "release"])
        editor.update(index: 1, milliseconds: "-1")
        XCTAssertThrowsError(try editor.action(named: "Local"))
    }
}
