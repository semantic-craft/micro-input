import XCTest
import AppKit
import SwiftUI
@testable import MicroInput
import MicroInputCore

private struct ActionLibrarySheetHost: View {
    let sheet: ActionLibrarySheet
    @State private var presented = true

    var body: some View {
        Color.clear
            .frame(width: 700, height: 560)
            .sheet(isPresented: $presented) { sheet }
    }
}

@MainActor
final class ActionLibraryFocusTests: XCTestCase {
    func testActionLibrarySearchStartsFocusedInASheet() async throws {
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput())
        await store.loadDemo()
        let host = ActionLibrarySheetHost(
            sheet: ActionLibrarySheet(store: store) {}
        )
        let container = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 700, height: 560),
            styleMask: [.titled],
            backing: .buffered,
            defer: false
        )
        container.contentView = NSHostingView(rootView: host)
        container.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)

        func textFields(in view: NSView) -> [NSTextField] {
            var fields = view.subviews.compactMap { $0 as? NSTextField }
            for child in view.subviews { fields.append(contentsOf: textFields(in: child)) }
            return fields
        }

        var searchField: NSTextField?
        var sheetWindow: NSWindow?
        for _ in 0..<100 {
            sheetWindow = NSApp.windows.first { $0.sheetParent === container }
            searchField = sheetWindow?.contentView.flatMap { textFields(in: $0).first }
            if let searchField, searchField.currentEditor() != nil { break }
            try await Task.sleep(nanoseconds: 20_000_000)
        }

        let presentedSheet = try XCTUnwrap(sheetWindow, "action library sheet was not presented")
        let field = try XCTUnwrap(searchField, "action library search field was not presented")
        XCTAssertEqual(presentedSheet.firstResponder, field.currentEditor())
        container.close()
    }
}
