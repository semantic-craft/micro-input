import AppKit
import SwiftUI
import XCTest
@testable import MicroInputUI

final class SessionKeycapLayoutTests: XCTestCase {
    private let titles = [
        String(repeating: "Reconstructing the complete application workflow and verifying every keyboard interaction ", count: 3),
        String(repeating: "核对多语言应用中的完整会话标题、所有操作步骤与键盘反馈，确保每个状态都能清楚阅读。", count: 3)
    ]

    private func session(title: String, status: String, index: Int = 0, kind: String? = nil) -> SessionSlotModel {
        SessionSlotModel(
            slotIndex: index, keyBindingId: "AG0\(index)", mode: "Recent", status: status,
            kind: kind ?? ["running": "Working", "unread": "BlueDotCompleted", "needs_interaction": "NeedsAttention", "error": "Error", "idle": "Idle"][status]!, agentMonogram: "AG", title: title, subtitle: "",
            isUnreadBlueDot: status == "unread", isWorkingAnimated: status == "running",
            isBoundToTopKey: true, ledColorHex: "#655681", placeRaw: title.isEmpty ? nil : ["synthetic": true]
        )
    }

    @MainActor
    private func fitted<V: View>(_ content: V, width: CGFloat) -> NSHostingView<some View> {
        let host = NSHostingView(rootView: content.frame(width: width).background(UITheme.pageBackground))
        host.frame = NSRect(x: 0, y: 0, width: width, height: 1)
        host.layoutSubtreeIfNeeded()
        let height = host.fittingSize.height
        host.frame.size.height = height
        host.layoutSubtreeIfNeeded()
        return host
    }

    @MainActor
    func testLongSessionTitlesDoNotExpandKeycaps() {
        for title in titles {
            for status in ["running", "unread", "needs_interaction", "error", "idle"] {
                for showLabels in [true, false] {
                    let content = SessionKeycapContent(
                        slotIndex: 5, sessionSlot: session(title: title, status: status), showLabels: showLabels
                    )
                    let host = fitted(content, width: 56)
                    XCTAssertGreaterThan(host.fittingSize.height, 0)
                    XCTAssertLessThanOrEqual(host.fittingSize.height, 56, "\(status), labels=\(showLabels): text must fit the 68 pt key minus padding")
                }
            }
        }
    }

    func testStatusLabelsUseProjectedSessionStates() {
        for (status, expected) in [("running", "进行中"), ("unread", "已完成"), ("needs_interaction", "待回应"), ("error", "执行异常"), ("idle", "就绪")] {
            XCTAssertEqual(SessionKeyStatusView.label(for: session(title: titles[0], status: status)), expected)
        }
        XCTAssertEqual(SessionKeyStatusView.label(for: session(title: titles[0], status: "unread", kind: "Idle")), "就绪")
        XCTAssertEqual(SessionKeyStatusView.label(for: nil), "未分配")
        XCTAssertEqual(SessionKeyStatusView.label(for: session(title: "", status: "idle")), "未分配")
    }

    @MainActor
    func testEmptySlotsFromCoreProjectionAreUnassigned() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = CoreStore(isolatedDirectory: directory.path)
        let slots = store.sessionStrip.sessionSlots
        XCTAssertEqual(slots.count, 6)
        for slot in slots {
            XCTAssertTrue(slot.title.isEmpty)
            XCTAssertEqual(SessionKeyStatusView.label(for: slot), "未分配")
        }
    }

    @MainActor
    func testFullSessionTitlesWrapAtSupportedWindowWidths() throws {
        for windowWidth in [820, 1180, 1440] {
            let contentWidth = min(CGFloat(windowWidth) - UITheme.Layout.sidebarWidth, UITheme.Layout.contentMaxWidth) - 2 * UITheme.Layout.pagePadding
            for (index, title) in titles.enumerated() {
                let slot = session(title: title, status: "unread", index: index)
                let host = fitted(SessionTitleRow(slotIndex: index, title: title, sessionSlot: slot, isSelected: true, onSelect: {}), width: contentWidth)
                let short = fitted(SessionTitleRow(slotIndex: index, title: "Short title", sessionSlot: slot, isSelected: false, onSelect: {}), width: contentWidth)
                XCTAssertGreaterThan(host.fittingSize.height, short.fittingSize.height, "The full title must increase row height rather than truncate")
                let statusWidth = ("已完成" as NSString).size(withAttributes: [.font: NSFont.systemFont(ofSize: 13)]).width
                let textWidth = contentWidth - 24 - 28 - 24 - statusWidth
                let textHeight = (title as NSString).boundingRect(
                    with: NSSize(width: textWidth, height: .greatestFiniteMagnitude),
                    options: [.usesLineFragmentOrigin, .usesFontLeading],
                    attributes: [.font: NSFont.systemFont(ofSize: 14)]
                ).height
                XCTAssertGreaterThanOrEqual(host.fittingSize.height, textHeight + 24, "The row must reserve the full title's measured height")
                try capture(host, name: "session-title-\(windowWidth)-\(index)")
            }
        }
    }

    @MainActor
    private func capture(_ host: NSView, name: String) throws {
        guard let output = ProcessInfo.processInfo.environment["MI_LAYOUT_OUTPUT"] else { return }
        let directory = URL(fileURLWithPath: output, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        let data = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
        try data.write(to: directory.appendingPathComponent("\(name).png"))
    }
}
