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
        let data = try JSONSerialization.data(withJSONObject: [
            "status": "PASS", "transport": "real-unix-socket", "confirmed_targets": selected.count,
            "native_install": "NOT_RUN", "physical_key": "NOT_RUN", "transcript_api": "NOT_EXPOSED",
        ], options: [.prettyPrinted, .sortedKeys])
        try data.write(to: URL(fileURLWithPath: output))
    }
}
