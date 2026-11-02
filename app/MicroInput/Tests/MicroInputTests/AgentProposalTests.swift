import XCTest
@testable import MicroInput
import MicroInputCore

final class AgentProposalTests: XCTestCase {
    private func runCLI(_ command: String, _ request: [String:Any]) throws -> [String:Any] {
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let process = Process(); process.executableURL = root.appendingPathComponent("target/debug/mi")
        process.arguments = ["draft", command]
        let input = Pipe(), output = Pipe(); process.standardInput = input; process.standardOutput = output
        try process.run(); try input.fileHandleForWriting.write(contentsOf: JSONSerialization.data(withJSONObject: request)); try input.fileHandleForWriting.close()
        let data = try output.fileHandleForReading.readToEnd()!; process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, String(decoding:data,as:UTF8.self))
        let envelope = try JSONSerialization.jsonObject(with:data) as! [String:Any]
        return envelope["result"] as! [String:Any]
    }
    @MainActor func testSubprocessProposalCanBeRejectedThenAcceptedAndReopenedThroughRealUniFFI() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at:dir) }
        let url = dir.appendingPathComponent("workbench.json")
        let store = Store(backend:MicroInputCore.MicroInput(),documentURL:url)
        await store.loadWorkbench(); await store.assign("save")
        let original = try Data(contentsOf:url), originalID = store.profile?.id.text
        let read = try runCLI("read", ["api_version":1,"workbench":url.path])
        let document = read["document"] as! [String:Any], graph = document["graph"] as! [String:Any]
        let pid = (graph["profile_order"] as! [String])[0], output = dir.appendingPathComponent("agent-proposal.json")
        let result = try runCLI("patch",["api_version":1,"workbench":url.path,"expected":read["stamp"]!,"operations":[["op":"rename_profile","profile_id":pid,"name":"CLI 审阅结果"]],"output":output.path])
        let text = try String(contentsOf:output,encoding:.utf8)
        let preview = try await store.inspectAgentProposal(text)
        XCTAssertTrue(preview.changes.contains(where:{$0.contains("CLI 审阅结果")}))
        await store.rejectAgentProposal()
        XCTAssertEqual(store.profile?.id.text,originalID); XCTAssertEqual(try Data(contentsOf:url),original)
        let second = try await store.inspectAgentProposal(text)
        let accepted = await store.acceptAgentProposal(token:second.token)
        XCTAssertTrue(accepted); XCTAssertEqual(store.profile?.name,"CLI 审阅结果")
        let proposed = result["document"] as! [String:Any]
        XCTAssertEqual(store.draft.document_id,proposed["document_id"] as? String)
        XCTAssertEqual(try Data(contentsOf:url),original)
        let reopened = Store(backend:MicroInputCore.MicroInput(),documentURL:url); await reopened.loadWorkbench()
        XCTAssertEqual(reopened.profile?.name,"CLI 审阅结果"); XCTAssertEqual(reopened.draft.document_id,store.draft.document_id)
        XCTAssertEqual(reopened.snapshot?.source,"LocalDraft"); XCTAssertTrue(reopened.devices.isEmpty)
        let stale = await reopened.acceptAgentProposal(token:second.token); XCTAssertFalse(stale)
    }
}
