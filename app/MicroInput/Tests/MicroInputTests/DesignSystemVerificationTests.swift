import AppKit
import Foundation
import SwiftUI
import XCTest
@testable import MicroInputUI

final class DesignSystemVerificationTests: XCTestCase {
    private func makeIsolatedTempDir(_ name: String) throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mi-design-test-\(name)-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    @MainActor
    private func makeHostedWindow<V: View>(
        _ rootView: V,
        width: CGFloat,
        fixedHeight: CGFloat? = nil
    ) -> (NSWindow, NSHostingView<some View>) {
        let wrapped = rootView
            .environment(\.colorScheme, .light)
        let host = NSHostingView(rootView: wrapped)
        host.appearance = NSAppearance(named: .aqua)
        host.frame = NSRect(x: 0, y: 0, width: width, height: fixedHeight ?? 10)
        host.layoutSubtreeIfNeeded()
        let height = fixedHeight ?? max(1, ceil(host.fittingSize.height))
        host.frame = NSRect(x: 0, y: 0, width: width, height: height)

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: width, height: height),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        window.appearance = NSAppearance(named: .aqua)
        window.backgroundColor = .clear
        window.isOpaque = false
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        host.layoutSubtreeIfNeeded()
        return (window, host)
    }

    @MainActor
    private func collectTriggerRects(in root: NSView) -> [NSRect] {
        var results: [NSRect] = []
        func visit(_ view: NSView) {
            let typeName = String(describing: type(of: view))
            if typeName == "NSView", view.subviews.isEmpty, view.bounds.width > 50, view.bounds.height >= 24, view.bounds.height <= 44 {
                let rectInRoot = view.convert(view.bounds, to: root)
                results.append(rectInRoot)
            }
            for child in view.subviews {
                visit(child)
            }
        }
        visit(root)
        return results.sorted { $0.minY < $1.minY }
    }

    @MainActor
    func testSelectDropdownTriggerStaysCompactAndRightAligned() {
        let singleOptions: [SelectOption<String>] = [
            .init(value: "65", title: "适中 65%"),
            .init(value: "100", title: "明亮 100%"),
            .init(value: "30", title: "柔和 30%"),
        ]

        let standaloneHost = NSHostingView(
            rootView: SelectDropdown(options: singleOptions, selection: "65", minTriggerWidth: 108) { _ in }
        )
        standaloneHost.layoutSubtreeIfNeeded()
        let standaloneSize = standaloneHost.fittingSize
        XCTAssertGreaterThanOrEqual(standaloneSize.width, 100, "SelectDropdown must honor minTriggerWidth")
        XCTAssertLessThan(standaloneSize.width, 200, "SelectDropdown trigger must remain a compact capsule rather than stretching across the row")
        XCTAssertGreaterThanOrEqual(standaloneSize.height, 28)
        XCTAssertLessThanOrEqual(standaloneSize.height, 40)

        for windowWidth in [CGFloat(820), CGFloat(1180)] {
            let cardWidth = min(windowWidth - UITheme.Layout.sidebarWidth - 56, 680)
            let card = VStack(spacing: 0) {
                SettingsCard {
                    SettingsRow("整体亮度", showsDivider: true) {
                        SelectDropdown(options: singleOptions, selection: "65", minTriggerWidth: 108) { _ in }
                    }
                    SettingsRow("完成与空闲时的颜色", subtitle: "有未读完成的任务时显示的主色", showsDivider: true) {
                        SelectDropdown(options: singleOptions, selection: "100", minTriggerWidth: 108) { _ in }
                    }
                    SettingsRow("仅在有活跃会话时亮起背光指示", showsDivider: false) {
                        SelectDropdown(options: singleOptions, selection: "30", minTriggerWidth: 108) { _ in }
                    }
                }
            }
            .padding(20)
            .frame(width: cardWidth)
            .background(UITheme.pageBackground)

            let (window, host) = makeHostedWindow(card, width: cardWidth)
            defer { window.orderOut(nil) }

            XCTAssertGreaterThan(host.fittingSize.height, 140)
            let rects = collectTriggerRects(in: host)
            XCTAssertEqual(rects.count, 3, "Expected one anchor probe NSView per SelectDropdown row at width \(windowWidth)")
            if let firstMaxX = rects.first?.maxX {
                for rect in rects {
                    XCTAssertLessThan(rect.width, 200, "Dropdown inside SettingsRow must stay compact")
                    XCTAssertEqual(rect.maxX, firstMaxX, accuracy: 1.0, "All SettingsRow dropdown triggers must right-align to the same trailing edge")
                }
            }
        }
    }

    @MainActor
    func testDropdownMenuCardSingleLineAndTwoLineLayoutAndSelection() {
        let singleLineOptions: [SelectOption<String>] = [
            .init(value: "1m", title: "1 分钟"),
            .init(value: "5m", title: "5 分钟"),
            .init(value: "15m", title: "15 分钟"),
            .init(value: "never", title: "从不"),
        ]
        let longSubtitle = "当后台编码代理会话正在流式执行工具与测试验证时，使用柔和低干扰的呼吸绿光持续提示运行进展。"
        let twoLineOptions: [SelectOption<String>] = [
            .init(value: "#3B82F6", title: "海洋蓝", subtitle: "默认完成与空闲蓝光"),
            .init(value: "#14C76B", title: "翡翠绿", subtitle: longSubtitle),
            .init(value: "#FFAA00", title: "琥珀金", subtitle: "温暖待确认提示暖光"),
            .init(value: "#FFFFFF", title: "纯白", subtitle: "纯净中性单色背光"),
        ]

        var pickedValue = "#3B82F6"
        let singleHost = NSHostingView(
            rootView: DropdownMenuCardView(options: singleLineOptions, selectedValue: "5m", minWidth: 176) { _ in }
        )
        singleHost.layoutSubtreeIfNeeded()

        let twoLineCard = DropdownMenuCardView(options: twoLineOptions, selectedValue: pickedValue, minWidth: 176) { chosen in
            pickedValue = chosen
        }
        let (window, twoLineHost) = makeHostedWindow(twoLineCard, width: 280)
        defer { window.orderOut(nil) }

        XCTAssertEqual(singleHost.fittingSize.width, 176, accuracy: 1.0)
        XCTAssertEqual(twoLineHost.fittingSize.width, 280, accuracy: 1.0)
        XCTAssertGreaterThan(
            twoLineHost.fittingSize.height,
            singleHost.fittingSize.height + 60,
            "Two-line DropdownMenuCardView must expand vertically to fit subtitles without clipping"
        )

        let measuredLongSubtitleHeight = (longSubtitle as NSString).boundingRect(
            with: NSSize(width: CGFloat(244), height: .greatestFiniteMagnitude),
            options: [.usesLineFragmentOrigin, .usesFontLeading],
            attributes: [.font: NSFont.systemFont(ofSize: 13)]
        ).height
        XCTAssertGreaterThan(measuredLongSubtitleHeight, 24, "Fixture subtitle must wrap onto multiple lines")
        XCTAssertGreaterThan(twoLineHost.fittingSize.height, measuredLongSubtitleHeight + 160)

        twoLineCard.onSelect("#14C76B")
        XCTAssertEqual(pickedValue, "#14C76B")
    }

    @MainActor
    func testHeaderFitsMinimumWindowWidthAndLightingPageDeduplicatesButtons() throws {
        let dir = try makeIsolatedTempDir("header-min-width")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        let headerHost = NSHostingView(rootView: WorkbenchHeaderBarView(store: store))
        headerHost.layoutSubtreeIfNeeded()
        XCTAssertLessThanOrEqual(
            headerHost.fittingSize.width,
            820,
            "WorkbenchHeaderBarView minimum intrinsic width must fit inside the 820pt minimum window width without clipping 按键速查"
        )

        let workbenchHost = NSHostingView(rootView: ClassicWorkbenchView(store: store).frame(width: 820, height: 640))
        workbenchHost.layoutSubtreeIfNeeded()
        XCTAssertLessThanOrEqual(
            workbenchHost.fittingSize.width,
            820,
            "ClassicWorkbenchView must fit inside 820x640 minimum window size without horizontal overflow"
        )

        let (window, rootHost) = makeHostedWindow(WorkbenchRootView(store: store), width: 820, fixedHeight: 640)
        defer { window.orderOut(nil) }
        XCTAssertEqual(rootHost.bounds.width, 820, accuracy: 0.5)

        let repoRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let boardSrc = try String(
            contentsOf: repoRoot.appendingPathComponent("app/MicroInput/Sources/MicroInputUI/CreatorMicroBoardView.swift"),
            encoding: .utf8
        )
        let workbenchSrc = try String(
            contentsOf: repoRoot.appendingPathComponent("app/MicroInput/Sources/MicroInputUI/ClassicWorkbenchView.swift"),
            encoding: .utf8
        )
        XCTAssertFalse(boardSrc.contains("编辑层灯光"), "SimpleLightingControlCard must not duplicate 编辑层灯光")
        XCTAssertEqual(workbenchSrc.components(separatedBy: "\"编辑层灯光\"").count - 1, 1, "Lighting settings page must expose exactly one 编辑层灯光 action")
    }

    @MainActor
    func testCheatSheetSpatialLayoutMapsAllFourPhysicalRowsInSplitAndMergedModes() throws {
        let dir = try makeIsolatedTempDir("cheatsheet-spatial")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        store.dispatch("canvas", ["SetBottomRowLayout": "split"])
        store.dispatch("overlay", "ToggleCheatSheet")
        let splitSheet = try XCTUnwrap(store.overlay.cheatSheet)
        let splitLayout = CheatSheetSpatialLayout(sheet: splitSheet)

        XCTAssertTrue(splitLayout.isSplitBottomRow)
        XCTAssertEqual(splitLayout.rows.count, 4)
        XCTAssertEqual(splitLayout.rows[0].map(\.bindingIds), [["AG13", "AG14", "ENC_CLK"], ["AG00"], ["AG01"], ["AG15", "AG16", "AG17", "AG18"]])
        XCTAssertEqual(splitLayout.rows[1].map(\.bindingIds), [["AG02"], ["AG03"], ["AG04"], ["AG05"]])
        XCTAssertEqual(splitLayout.rows[2].map(\.bindingIds), [["AG06"], ["AG07"], ["AG08"], ["AG09"]])
        XCTAssertEqual(splitLayout.rows[3].map(\.bindingIds), [[], ["AG10"], ["AG11"], ["AG12"]])
        XCTAssertEqual(splitLayout.rows[3].map(\.spanCols), [1, 1, 1, 1])

        store.dispatch("overlay", "CloseCheatSheet")
        store.dispatch("canvas", ["SetBottomRowLayout": "merged"])
        store.dispatch("overlay", "ToggleCheatSheet")
        let mergedSheet = try XCTUnwrap(store.overlay.cheatSheet)
        let mergedLayout = CheatSheetSpatialLayout(sheet: mergedSheet)

        XCTAssertFalse(mergedLayout.isSplitBottomRow)
        XCTAssertEqual(mergedLayout.rows.count, 4)
        XCTAssertEqual(mergedLayout.rows[3].map(\.bindingIds), [[], ["AG10"], ["AG12"]])
        XCTAssertEqual(mergedLayout.rows[3].map(\.spanCols), [1, 2, 1])
    }

    @MainActor
    func testDropdownPanelCapsLongOptionListsClampsScreenBoundsAndCleansUpOnWindowCloseAndDeinit() throws {
        let thirtyOptions: [SelectOption<String>] = (0..<30).map { idx in
            SelectOption(
                value: "opt-\(idx)",
                title: "分支动作 \(idx)",
                subtitle: "宏步骤与系统快捷键详情 \(idx)"
            )
        }
        let card = DropdownMenuCardView(
            options: thirtyOptions,
            selectedValue: "opt-0",
            hostedInPanel: true
        ) { _ in }
        let cardHost = NSHostingView(rootView: card)
        cardHost.layoutSubtreeIfNeeded()
        XCTAssertLessThan(cardHost.fittingSize.height, 400)

        let visible = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        let anchor = NSView(frame: NSRect(x: 20, y: 12, width: 120, height: 28))
        let parentWindow = NSWindow(
            contentRect: NSRect(x: visible.minX + 40, y: visible.minY + 12, width: 320, height: 80),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 320, height: 80))
        container.addSubview(anchor)
        parentWindow.contentView = container

        let upperTriggerWindow = NSWindow(
            contentRect: NSRect(x: visible.minX + 40, y: visible.minY + 12, width: 320, height: visible.height - 24),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        let upperAnchor = NSView(frame: NSRect(x: 20, y: visible.height - 80, width: 120, height: 28))
        let upperContainer = NSView(frame: NSRect(x: 0, y: 0, width: 320, height: visible.height - 24))
        upperContainer.addSubview(upperAnchor)
        upperTriggerWindow.contentView = upperContainer
        defer {
            parentWindow.orderOut(nil)
            upperTriggerWindow.orderOut(nil)
        }

        let controller = DropdownPanelController()
        controller.present(from: anchor, content: card)
        let popup = try XCTUnwrap(controller.panel)
        let actualVisible = parentWindow.screen?.visibleFrame ?? visible
        XCTAssertGreaterThanOrEqual(popup.frame.minY + DropdownPanelController.bottomInset, actualVisible.minY + 7.5)
        XCTAssertLessThanOrEqual(popup.frame.maxY - DropdownPanelController.topInset, actualVisible.maxY - 7.5)

        NotificationCenter.default.post(name: NSWindow.willCloseNotification, object: parentWindow)
        XCTAssertFalse(controller.isOpen)
        XCTAssertNil(controller.panel)

        var transientController: DropdownPanelController? = DropdownPanelController()
        transientController?.present(from: upperAnchor, content: card)
        let transientPopup = try XCTUnwrap(transientController?.panel)
        XCTAssertTrue(transientPopup.isVisible)
        transientController = nil
        XCTAssertFalse(transientPopup.isVisible)
    }

    @MainActor
    private func hasScrollView(in root: NSView) -> Bool {
        if root is NSScrollView {
            return true
        }
        return root.subviews.contains { hasScrollView(in: $0) }
    }

    @MainActor
    func testPopoverPreviewModeSettingsAndCheatSheetGeometryWithoutClipping() throws {
        let dir = try makeIsolatedTempDir("popover-preview-mode")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        XCTAssertEqual(store.statusMenu.popoverPreviewMode, .sessionPreview)
        XCTAssertNil(store.statusMenu.cheatSheetPreview)
        XCTAssertEqual(PopoverPreviewMode.dropdownOptions.map(\.title), ["会话预览", "按键速查"])

        ViewRenderTelemetry.isEnabled = true
        defer { ViewRenderTelemetry.isEnabled = false }
        ViewRenderTelemetry.reset()
        let (sessionWin, sessionHost) = makeHostedWindow(
            StatusMenuPopoverView(store: store, onOpenMainWindow: {}),
            width: 348
        )
        let initialSessionHeight = sessionHost.fittingSize.height
        XCTAssertEqual(sessionHost.fittingSize.width, 348, accuracy: 1.0)
        XCTAssertLessThan(initialSessionHeight, 640)
        XCTAssertFalse(hasScrollView(in: sessionHost), "StatusMenuPopoverView must never embed NSScrollView")
        XCTAssertGreaterThanOrEqual(ViewRenderTelemetry.count("PopoverSessionPreviewSection"), 1)
        XCTAssertEqual(ViewRenderTelemetry.count("PopoverMiniCheatSheetView"), 0)
        sessionWin.contentView = nil
        sessionWin.orderOut(nil)

        store.dispatch("canvas", ["SetBottomRowLayout": "split"])
        store.dispatch("status_menu", ["SetPopoverPreviewMode": PopoverPreviewMode.cheatSheet.rawValue])
        XCTAssertEqual(store.statusMenu.popoverPreviewMode, .cheatSheet)
        let splitPreview = try XCTUnwrap(store.statusMenu.cheatSheetPreview)
        XCTAssertEqual(splitPreview.keyCells.count, 20)
        XCTAssertEqual(Set(splitPreview.keyCells.map(\.bindingId)).count, 20)

        let miniHost = NSHostingView(rootView: PopoverMiniCheatSheetView(sheet: splitPreview).frame(width: 320))
        miniHost.layoutSubtreeIfNeeded()
        XCTAssertLessThanOrEqual(miniHost.fittingSize.width, 320.5)
        XCTAssertLessThan(miniHost.fittingSize.height, 300)

        ViewRenderTelemetry.reset()
        let (splitWin, splitHost) = makeHostedWindow(
            StatusMenuPopoverView(store: store, onOpenMainWindow: {}),
            width: 348
        )
        defer { splitWin.orderOut(nil) }
        XCTAssertEqual(splitHost.fittingSize.width, 348, accuracy: 1.0)
        XCTAssertLessThan(splitHost.fittingSize.height, 700)
        XCTAssertGreaterThan(splitHost.fittingSize.height, initialSessionHeight + 30)
        XCTAssertFalse(hasScrollView(in: splitHost), "StatusMenuPopoverView in cheatSheet mode must never embed NSScrollView")
        XCTAssertEqual(ViewRenderTelemetry.count("PopoverSessionPreviewSection"), 0)
        XCTAssertGreaterThanOrEqual(ViewRenderTelemetry.count("PopoverMiniCheatSheetView"), 1)

        let splitBitmap = try XCTUnwrap(splitHost.bitmapImageRepForCachingDisplay(in: splitHost.bounds))
        splitHost.cacheDisplay(in: splitHost.bounds, to: splitBitmap)
        XCTAssertNotNil(splitBitmap.representation(using: .png, properties: [:]))

        let splitLayout = CheatSheetSpatialLayout(sheet: splitPreview)
        XCTAssertTrue(splitLayout.isSplitBottomRow)
        XCTAssertEqual(splitLayout.rows.count, 4)
        for cell in splitPreview.keyCells {
            XCTAssertFalse(cell.shortLabel.isEmpty)
            XCTAssertFalse(cell.shortLabel.contains("…"), "Canonical split cheat-sheet shortLabel must not truncate with ellipsis: \(cell.shortLabel)")
            if ["AG13", "AG14", "ENC_CLK"].contains(cell.bindingId) {
                let w = (cell.shortLabel as NSString).size(withAttributes: [.font: NSFont.systemFont(ofSize: 9 * 0.75, weight: .medium)]).width
                XCTAssertLessThanOrEqual(w, 52, "Rotary shortLabel \(cell.shortLabel) must fit 1U rotary row")
            } else if ["AG15", "AG16", "AG17", "AG18"].contains(cell.bindingId) {
                let h = (cell.shortLabel as NSString).boundingRect(
                    with: NSSize(width: CGFloat(22), height: .greatestFiniteMagnitude),
                    options: [.usesLineFragmentOrigin, .usesFontLeading],
                    attributes: [.font: NSFont.systemFont(ofSize: 8 * 0.75, weight: .medium)]
                ).height
                XCTAssertLessThanOrEqual(h, 24, "Joystick shortLabel \(cell.shortLabel) must fit 2x2 quadrant height")
            } else {
                let h = (cell.shortLabel as NSString).boundingRect(
                    with: NSSize(width: CGFloat(60), height: .greatestFiniteMagnitude),
                    options: [.usesLineFragmentOrigin, .usesFontLeading],
                    attributes: [.font: NSFont.systemFont(ofSize: 10.5 * 0.82, weight: .medium)]
                ).height
                XCTAssertLessThanOrEqual(h, 34, "1U key shortLabel \(cell.shortLabel) must fit 2-line budget")
            }
        }

        store.dispatch("canvas", ["SetBottomRowLayout": "merged"])
        let mergedPreview = try XCTUnwrap(store.statusMenu.cheatSheetPreview)
        XCTAssertEqual(mergedPreview.keyCells.count, 19)
        XCTAssertFalse(mergedPreview.keyCells.map(\.bindingId).contains("AG11"), "Merged 2U layout must omit AG11")
        let mergedAg10 = try XCTUnwrap(mergedPreview.keyCells.first(where: { $0.bindingId == "AG10" }))
        XCTAssertEqual(mergedAg10.shortLabel, "按住说话")
        for cell in mergedPreview.keyCells {
            XCTAssertFalse(cell.shortLabel.contains("…"), "Canonical merged cheat-sheet shortLabel must not contain ellipsis: \(cell.shortLabel)")
        }

        let (mergedWin, mergedHost) = makeHostedWindow(
            StatusMenuPopoverView(store: store, onOpenMainWindow: {}),
            width: 348
        )
        defer { mergedWin.orderOut(nil) }
        XCTAssertEqual(mergedHost.fittingSize.width, 348, accuracy: 1.0)
        XCTAssertLessThan(mergedHost.fittingSize.height, 700)

        let repoRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let statusMenuSrc = try String(
            contentsOf: repoRoot.appendingPathComponent("app/MicroInput/Sources/MicroInputUI/ModalsAndStatusMenuView.swift"),
            encoding: .utf8
        )
        let workbenchSrc = try String(
            contentsOf: repoRoot.appendingPathComponent("app/MicroInput/Sources/MicroInputUI/ClassicWorkbenchView.swift"),
            encoding: .utf8
        )
        XCTAssertFalse(statusMenuSrc.contains("ToggleCheatSheet"), "Broken bottom 按键速查 action row must be removed from StatusMenuPopoverView")
        XCTAssertFalse(statusMenuSrc.contains("隐藏按键速查"))
        XCTAssertTrue(workbenchSrc.contains("菜单栏弹层内容"))
        XCTAssertTrue(workbenchSrc.contains("popover-preview-mode-dropdown"))
        XCTAssertTrue(workbenchSrc.contains("SetPopoverPreviewMode"))

        store.dispatch("overlay", "ToggleCheatSheet")
        XCTAssertTrue(store.overlay.cheatSheetVisible)
        store.dispatch("overlay", "CloseCheatSheet")
        XCTAssertFalse(store.overlay.cheatSheetVisible)
        XCTAssertEqual(store.statusMenu.popoverPreviewMode, .cheatSheet)

        let anchorWindow = NSWindow(
            contentRect: NSRect(x: 100, y: 500, width: 80, height: 28),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        anchorWindow.isReleasedWhenClosed = false
        let anchorButton = NSButton(frame: NSRect(x: 0, y: 0, width: 40, height: 24))
        anchorWindow.contentView = anchorButton
        anchorWindow.orderFrontRegardless()
        defer {
            anchorWindow.orderOut(nil)
            anchorWindow.close()
        }

        let persistentPopover = NSPopover()
        persistentPopover.animates = false
        let hc = NSHostingController(
            rootView: StatusMenuPopoverView(store: store, onOpenMainWindow: {})
        )
        hc.sizingOptions = [.preferredContentSize]
        persistentPopover.contentViewController = hc
        defer {
            if persistentPopover.isShown {
                persistentPopover.performClose(nil)
            }
        }

        store.dispatch("status_menu", ["SetPopoverPreviewMode": PopoverPreviewMode.sessionPreview.rawValue])
        hc.view.layoutSubtreeIfNeeded()
        let size1 = hc.sizeThatFits(in: NSSize(width: CGFloat(348), height: .greatestFiniteMagnitude))
        persistentPopover.contentSize = size1
        persistentPopover.show(relativeTo: anchorButton.bounds, of: anchorButton, preferredEdge: .minY)
        let h1 = persistentPopover.contentSize.height
        XCTAssertEqual(size1.width, 348, accuracy: 1.0)
        XCTAssertLessThan(h1, 640)
        persistentPopover.performClose(nil)

        store.dispatch("canvas", ["SetBottomRowLayout": "split"])
        store.dispatch("status_menu", ["SetPopoverPreviewMode": PopoverPreviewMode.cheatSheet.rawValue])
        hc.view.layoutSubtreeIfNeeded()
        let size2 = hc.sizeThatFits(in: NSSize(width: CGFloat(348), height: .greatestFiniteMagnitude))
        persistentPopover.contentSize = size2
        persistentPopover.show(relativeTo: anchorButton.bounds, of: anchorButton, preferredEdge: .minY)
        let h2 = persistentPopover.contentSize.height
        XCTAssertEqual(size2.width, 348, accuracy: 1.0)
        XCTAssertGreaterThan(h2, h1 + 30)
        XCTAssertEqual(hc.view.frame.height, size2.height, accuracy: 2.0)
        persistentPopover.performClose(nil)
    }

    @MainActor
    func testCheatSheetSpatialLayoutDeduplicatesRepeatedBindingIdsKeepingFirstEntry() {
        let sheet = CheatSheetModel(
            profileName: "Antigravity",
            routingSummary: "自动跟随",
            keyCells: [
                CheatSheetCellModel(bindingId: "AG00", controlName: "左上键", label: "首选动作", available: true),
                CheatSheetCellModel(bindingId: "AG00", controlName: "左上键", label: "重复动作", available: false),
                CheatSheetCellModel(bindingId: "AG11", controlName: "底中键", label: "拆分布局", available: true),
            ]
        )
        let layout = CheatSheetSpatialLayout(sheet: sheet)
        XCTAssertTrue(layout.isSplitBottomRow)
        if case let .key(cell, bindingId, _, _) = layout.rows[0][1] {
            XCTAssertEqual(bindingId, "AG00")
            XCTAssertEqual(cell?.label, "首选动作")
        } else {
            XCTFail("Expected key cell at row 0 col 1")
        }
    }

    @MainActor
    func testWheelMenuCopyUnificationAndToggleContract() throws {
        XCTAssertEqual(WorkbenchDestination.radial.rawValue, "轮盘菜单")

        let dir = try makeIsolatedTempDir("wheel-menu")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        XCTAssertFalse(store.statusMenu.radialMenuEnabled)
        XCTAssertEqual(store.statusMenu.radialSlots.count, 8)

        store.dispatch("status_menu", ["SetRadialMenuEnabled": true])
        XCTAssertTrue(store.statusMenu.radialMenuEnabled)
        store.dispatch("status_menu", ["SetRadialMenuEnabled": false])
        XCTAssertFalse(store.statusMenu.radialMenuEnabled)

        let (workbenchWindow, workbenchHost) = makeHostedWindow(
            ClassicWorkbenchView(store: store),
            width: 960,
            fixedHeight: 680
        )
        defer { workbenchWindow.orderOut(nil) }
        XCTAssertGreaterThan(workbenchHost.bounds.height, 0)

        let (popoverWindow, popoverHost) = makeHostedWindow(
            StatusMenuPopoverView(store: store, onOpenMainWindow: {}),
            width: 348
        )
        defer { popoverWindow.orderOut(nil) }
        XCTAssertGreaterThan(popoverHost.bounds.height, 0)
    }

    @MainActor
    func testExportVisualDesignSystemSnapshots() throws {
        guard let outputPath = ProcessInfo.processInfo.environment["MI_LAYOUT_OUTPUT"], !outputPath.isEmpty else {
            return
        }
        let outputDir = URL(fileURLWithPath: outputPath, isDirectory: true)
        try FileManager.default.createDirectory(at: outputDir, withIntermediateDirectories: true)

        let dir = try makeIsolatedTempDir("visual-snapshots")
        defer { try? FileManager.default.removeItem(at: dir) }
        let store = CoreStore(isolatedDirectory: dir.path)

        let lightingPage = VStack(alignment: .leading, spacing: 20) {
            VStack(alignment: .leading, spacing: 4) {
                Text("灯效设置")
                    .font(UITheme.title)
                    .foregroundStyle(UITheme.textPrimary)
                Text("配置会话状态背光反馈、自动熄灯与当前按键层硬件灯效。")
                    .font(UITheme.caption)
                    .foregroundStyle(UITheme.textSecondary)
            }
            SimpleLightingControlCard(store: store)
            VStack(alignment: .leading, spacing: 10) {
                SettingsSectionHeader("按键层灯光")
                SettingsCard {
                    SettingsRow("双区 RGB 灯效", subtitle: "配置当前按键层的静态背光、底灯颜色与亮度。") {
                        Button("编辑层灯光") {}
                            .surfaceButton(.secondary)
                    }
                }
            }
        }
        .padding(28)
        .frame(width: 720)
        .background(UITheme.pageBackground)
        try saveSnapshot(lightingPage, width: 720, to: outputDir.appendingPathComponent("lighting-settings-card.png"))

        let twoLineMenu = VStack {
            DropdownMenuCardView(
                options: [
                    .init(value: "#3B82F6", title: "海洋蓝", subtitle: "默认完成与空闲蓝光"),
                    .init(value: "#14C76B", title: "翡翠绿", subtitle: "柔和醒目的运行态绿光"),
                    .init(value: "#FFAA00", title: "琥珀金", subtitle: "温暖待确认提示暖光"),
                    .init(value: "#00B8A9", title: "青碧", subtitle: "冷静低干扰青绿光"),
                    .init(value: "#FFFFFF", title: "纯白", subtitle: "纯净中性单色背光"),
                ],
                selectedValue: "#3B82F6",
                hostedInPanel: true
            ) { _ in }
        }
        .padding(8)
        .background(UITheme.pageBackground)
        try saveSnapshot(twoLineMenu, width: 340, to: outputDir.appendingPathComponent("dropdown-open-twoline.png"))

        let singleLineMenu = VStack {
            DropdownMenuCardView(
                options: [
                    .init(value: "1m", title: "1 分钟"),
                    .init(value: "5m", title: "5 分钟"),
                    .init(value: "15m", title: "15 分钟"),
                    .init(value: "30m", title: "30 分钟"),
                    .init(value: "never", title: "从不"),
                ],
                selectedValue: "5m",
                minWidth: 196,
                hostedInPanel: true
            ) { _ in }
        }
        .padding(8)
        .background(UITheme.pageBackground)
        try saveSnapshot(singleLineMenu, width: 260, to: outputDir.appendingPathComponent("dropdown-open-singleline.png"))

        let editorModel = LightingEditorModel(
            profileId: "antigravity",
            layerId: "layer-0",
            layerName: "默认层",
            applyToAllLayers: false,
            canAddLighting: true,
            hasLighting: true,
            backlight: LightingZoneModel(
                zone: "backlight",
                zoneTitle: "键帽背光 (Backlight)",
                effect: "solid",
                brightnessPercent: 68,
                speedPercent: 45,
                magicPercent: 25,
                colorHex: "#3B82F6",
                canEditColor: true,
                canEditBrightness: true,
                canEditSpeed: true,
                canEditMagic: false
            ),
            underglow: LightingZoneModel(
                zone: "underglow",
                zoneTitle: "底盘环绕光 (Underglow)",
                effect: "breath",
                brightnessPercent: 52,
                speedPercent: 35,
                magicPercent: 40,
                colorHex: "#FFFFFF",
                canEditColor: true,
                canEditBrightness: true,
                canEditSpeed: true,
                canEditMagic: true
            ),
            previewActive: false,
            previewRemainingSecs: 0,
            validationError: nil
        )
        try saveSnapshot(
            LightingEditorSheet(store: store, model: editorModel),
            width: 640,
            to: outputDir.appendingPathComponent("lighting-editor-slider.png")
        )

        let makePopoverCard = {
            VStack {
                StatusMenuPopoverView(store: store, onOpenMainWindow: {})
                    .background(
                        RoundedRectangle(cornerRadius: 14, style: .continuous)
                            .fill(UITheme.surface)
                            .shadow(color: Color.black.opacity(0.10), radius: 14, x: 0, y: 6)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 14, style: .continuous)
                            .stroke(UITheme.hairlineBorder, lineWidth: 1)
                    )
            }
            .padding(20)
            .background(UITheme.pageBackground)
        }
        store.dispatch("status_menu", ["SetPopoverPreviewMode": PopoverPreviewMode.sessionPreview.rawValue])
        try saveSnapshot(makePopoverCard(), width: 388, to: outputDir.appendingPathComponent("status-menu-popover.png"))
        try saveSnapshot(makePopoverCard(), width: 388, to: outputDir.appendingPathComponent("status-menu-popover-sessions.png"))

        store.dispatch("canvas", ["SetBottomRowLayout": "split"])
        store.dispatch("status_menu", ["SetPopoverPreviewMode": PopoverPreviewMode.cheatSheet.rawValue])
        try saveSnapshot(makePopoverCard(), width: 388, to: outputDir.appendingPathComponent("status-menu-popover-cheatsheet.png"))

        store.dispatch("canvas", ["SetBottomRowLayout": "merged"])
        try saveSnapshot(makePopoverCard(), width: 388, to: outputDir.appendingPathComponent("status-menu-popover-cheatsheet-merged.png"))

        let previewModeDropdownMenu = VStack {
            DropdownMenuCardView(
                options: PopoverPreviewMode.dropdownOptions,
                selectedValue: .cheatSheet,
                minWidth: 280,
                hostedInPanel: true
            ) { _ in }
        }
        .padding(12)
        .background(UITheme.pageBackground)
        try saveSnapshot(
            previewModeDropdownMenu,
            width: 340,
            to: outputDir.appendingPathComponent("popover-preview-mode-dropdown.png")
        )

        store.dispatch("canvas", ["SetBottomRowLayout": "split"])
        store.dispatch("overlay", "ToggleCheatSheet")
        let liveSheet = try XCTUnwrap(store.overlay.cheatSheet)
        let cheatSheetContainer = ZStack {
            UITheme.pageBackground
            CheatSheetOverlayView(store: store, sheet: liveSheet)
        }
        .frame(width: 860, height: 640)
        try saveSnapshot(
            cheatSheetContainer,
            width: 860,
            fixedHeight: 640,
            to: outputDir.appendingPathComponent("cheat-sheet-4x4-spatial.png")
        )
    }

    @MainActor
    private func saveSnapshot<V: View>(
        _ view: V,
        width: CGFloat,
        fixedHeight: CGFloat? = nil,
        to url: URL
    ) throws {
        let (window, host) = makeHostedWindow(view, width: width, fixedHeight: fixedHeight)
        defer { window.orderOut(nil) }
        let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: bitmap)
        let data = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
        try data.write(to: url)
    }
}
