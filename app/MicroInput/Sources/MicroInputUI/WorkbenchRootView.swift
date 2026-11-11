import AppKit
import SwiftUI

public struct WorkbenchRootView: View {
    let store: CoreStore

    public init(store: CoreStore) {
        self.store = store
    }

    public var body: some View {
        let _ = ViewRenderTelemetry.record("WorkbenchRootView")
        let overlay = store.overlay
        let modal = store.activeModal
        ZStack {
            ClassicWorkbenchView(store: store)

            if overlay.cheatSheetVisible, let cs = overlay.cheatSheet {
                CheatSheetOverlayView(store: store, sheet: cs)
            }
        }
        .frame(minWidth: 820, minHeight: 640)
        .sheet(
            item: Binding(
                get: { modal },
                set: { newVal in
                    if newVal == nil {
                        store.dispatch("editor", "CloseModal")
                    }
                }
            )
        ) { activeModal in
            ActiveModalSheetView(store: store, modal: activeModal)
        }
    }
}

public enum CheatSheetSpatialCell: Equatable, Identifiable {
    case rotaryEncoder(ccw: CheatSheetCellModel?, cw: CheatSheetCellModel?, press: CheatSheetCellModel?)
    case joystick(north: CheatSheetCellModel?, west: CheatSheetCellModel?, south: CheatSheetCellModel?, east: CheatSheetCellModel?)
    case key(cell: CheatSheetCellModel?, bindingId: String, fallbackName: String, spanCols: Int)
    case statusCluster(profileName: String, routingSummary: String)

    public var id: String {
        switch self {
        case .rotaryEncoder: return "spatial-encoder"
        case .joystick: return "spatial-joystick"
        case .key(_, let bindingId, _, let spanCols): return "spatial-\(bindingId)-\(spanCols)u"
        case .statusCluster: return "spatial-status"
        }
    }

    public var spanCols: Int {
        switch self {
        case .key(_, _, _, let spanCols): return spanCols
        default: return 1
        }
    }

    public var bindingIds: [String] {
        switch self {
        case .rotaryEncoder: return ["AG13", "AG14", "ENC_CLK"]
        case .joystick: return ["AG15", "AG16", "AG17", "AG18"]
        case .key(_, let bindingId, _, _): return [bindingId]
        case .statusCluster: return []
        }
    }
}

public struct CheatSheetSpatialLayout: Equatable {
    public let isSplitBottomRow: Bool
    public let rows: [[CheatSheetSpatialCell]]

    public init(sheet: CheatSheetModel) {
        let lookup = Dictionary(sheet.keyCells.map { ($0.bindingId, $0) }, uniquingKeysWith: { first, _ in first })
        let hasSplit = lookup["AG11"] != nil
        self.isSplitBottomRow = hasSplit

        let row0: [CheatSheetSpatialCell] = [
            .rotaryEncoder(ccw: lookup["AG13"], cw: lookup["AG14"], press: lookup["ENC_CLK"]),
            .key(cell: lookup["AG00"], bindingId: "AG00", fallbackName: "左上键", spanCols: 1),
            .key(cell: lookup["AG01"], bindingId: "AG01", fallbackName: "右上键", spanCols: 1),
            .joystick(north: lookup["AG15"], west: lookup["AG16"], south: lookup["AG17"], east: lookup["AG18"]),
        ]

        let row1: [CheatSheetSpatialCell] = [
            .key(cell: lookup["AG02"], bindingId: "AG02", fallbackName: "按键 03", spanCols: 1),
            .key(cell: lookup["AG03"], bindingId: "AG03", fallbackName: "按键 04", spanCols: 1),
            .key(cell: lookup["AG04"], bindingId: "AG04", fallbackName: "按键 05", spanCols: 1),
            .key(cell: lookup["AG05"], bindingId: "AG05", fallbackName: "按键 06", spanCols: 1),
        ]

        let row2: [CheatSheetSpatialCell] = [
            .key(cell: lookup["AG06"], bindingId: "AG06", fallbackName: "按键 07", spanCols: 1),
            .key(cell: lookup["AG07"], bindingId: "AG07", fallbackName: "按键 08", spanCols: 1),
            .key(cell: lookup["AG08"], bindingId: "AG08", fallbackName: "按键 09", spanCols: 1),
            .key(cell: lookup["AG09"], bindingId: "AG09", fallbackName: "按键 10", spanCols: 1),
        ]

        var row3: [CheatSheetSpatialCell] = [
            .statusCluster(profileName: sheet.profileName, routingSummary: sheet.routingSummary),
        ]
        if hasSplit {
            row3.append(.key(cell: lookup["AG10"], bindingId: "AG10", fallbackName: "底行左键", spanCols: 1))
            row3.append(.key(cell: lookup["AG11"], bindingId: "AG11", fallbackName: "底行中键", spanCols: 1))
        } else {
            row3.append(.key(cell: lookup["AG10"], bindingId: "AG10", fallbackName: "底部宽键 (2U)", spanCols: 2))
        }
        row3.append(.key(cell: lookup["AG12"], bindingId: "AG12", fallbackName: "底行右键", spanCols: 1))

        self.rows = [row0, row1, row2, row3]
    }
}

struct CheatSheetOverlayView: View {
    let store: CoreStore
    let sheet: CheatSheetModel

    private let unitWidth: CGFloat = 164
    private let unitHeight: CGFloat = 104
    private let gridSpacing: CGFloat = 10

    var body: some View {
        let layout = CheatSheetSpatialLayout(sheet: sheet)
        let chassisWidth = unitWidth * 4 + gridSpacing * 3 + 28
        ZStack {
            Color.black.opacity(0.28)
                .ignoresSafeArea()
                .contentShape(Rectangle())
                .onTapGesture {
                    store.dispatch("overlay", "CloseCheatSheet")
                }

            VStack(alignment: .leading, spacing: 16) {
                HStack(alignment: .center, spacing: 12) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text("\(sheet.profileName) · 按键速查")
                            .font(.system(size: 18, weight: .semibold))
                            .foregroundStyle(UITheme.textPrimary)
                        Text(sheet.routingSummary)
                            .font(UITheme.caption)
                            .foregroundStyle(UITheme.textSecondary)
                    }
                    Spacer()
                    Text("ESC")
                        .font(UITheme.monoBadge)
                        .foregroundStyle(UITheme.textMuted)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(RoundedRectangle(cornerRadius: 6, style: .continuous).fill(UITheme.subtleSurface))
                        .overlay(RoundedRectangle(cornerRadius: 6, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))

                    Button {
                        store.dispatch("overlay", "CloseCheatSheet")
                    } label: {
                        Image(systemName: "xmark")
                    }
                    .surfaceButton(.icon)
                    .keyboardShortcut(.cancelAction)
                }
                .frame(width: chassisWidth)

                VStack(spacing: gridSpacing) {
                    ForEach(Array(layout.rows.enumerated()), id: \.offset) { _, row in
                        HStack(spacing: gridSpacing) {
                            ForEach(row) { item in
                                spatialCellView(item)
                            }
                        }
                    }
                }
                .padding(14)
                .background(
                    RoundedRectangle(cornerRadius: 20, style: .continuous)
                        .fill(UITheme.subtleSurface)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 20, style: .continuous)
                        .stroke(UITheme.hairlineBorder, lineWidth: 1)
                )
            }
            .padding(22)
            .background(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(UITheme.surface)
                    .shadow(color: Color.black.opacity(0.14), radius: 24, x: 0, y: 10)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .stroke(UITheme.hairlineBorder, lineWidth: 1)
            )
        }
        .onExitCommand {
            store.dispatch("overlay", "CloseCheatSheet")
        }
    }

    @ViewBuilder
    private func spatialCellView(_ item: CheatSheetSpatialCell) -> some View {
        switch item {
        case .rotaryEncoder(let ccw, let cw, let press):
            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 6) {
                    Image(systemName: "dial.low")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(UITheme.textSecondary)
                    Text("无极旋钮")
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textSecondary)
                    Spacer()
                }
                compactSubBindingRow(prefix: "↺", label: ccw?.label)
                compactSubBindingRow(prefix: "↻", label: cw?.label)
                compactSubBindingRow(prefix: "⏺", label: press?.label)
            }
            .padding(10)
            .frame(width: unitWidth, height: unitHeight, alignment: .topLeading)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(UITheme.surface))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(UITheme.strongBorder, lineWidth: 1))

        case .joystick(let north, let west, let south, let east):
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 5) {
                    Image(systemName: "dpad")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(UITheme.textSecondary)
                    Text("四向摇杆")
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textSecondary)
                    Spacer()
                }
                compactSubBindingRow(prefix: "↑", label: north?.label)
                compactSubBindingRow(prefix: "↓", label: south?.label)
                compactSubBindingRow(prefix: "←", label: west?.label)
                compactSubBindingRow(prefix: "→", label: east?.label)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .frame(width: unitWidth, height: unitHeight, alignment: .topLeading)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(UITheme.surface))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(UITheme.strongBorder, lineWidth: 1))

        case .key(let cell, let bindingId, let fallbackName, let spanCols):
            let cellWidth = spanCols == 2 ? (unitWidth * 2 + gridSpacing) : unitWidth
            let title = (cell?.label.isEmpty == false) ? cell!.label : "未绑定"
            let isAvailable = cell?.available ?? false
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 4) {
                    Text(bindingId)
                        .font(.system(size: 11, weight: .medium, design: .monospaced))
                        .foregroundStyle(UITheme.textMuted)
                    Spacer(minLength: 4)
                    Text(cell?.controlName ?? fallbackName)
                        .font(.system(size: 11, weight: .regular))
                        .foregroundStyle(UITheme.textSecondary)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
                Text(title)
                    .font(.system(size: 13.5, weight: isAvailable ? .medium : .regular))
                    .foregroundStyle(isAvailable ? UITheme.textPrimary : UITheme.textMuted)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(10)
            .frame(width: cellWidth, height: unitHeight, alignment: .topLeading)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(UITheme.surface))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))

        case .statusCluster(let profileName, _):
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 5) {
                    ForEach(0..<3, id: \.self) { _ in
                        Circle()
                            .fill(UITheme.textPrimary)
                            .frame(width: 5, height: 5)
                    }
                    Spacer()
                    Text("4×4")
                        .font(.system(size: 11, weight: .medium, design: .monospaced))
                        .foregroundStyle(UITheme.textMuted)
                }
                Spacer(minLength: 0)
                Text(profileName)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(UITheme.textPrimary)
                    .lineLimit(1)
                Text("物理坐标对齐")
                    .font(.system(size: 11.5))
                    .foregroundStyle(UITheme.textSecondary)
            }
            .padding(10)
            .frame(width: unitWidth, height: unitHeight, alignment: .topLeading)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(UITheme.mutedSurface.opacity(0.65)))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))
        }
    }

    private func compactSubBindingRow(prefix: String, label: String?) -> some View {
        let display = (label?.isEmpty == false) ? label! : "—"
        return HStack(spacing: 4) {
            Text(prefix)
                .font(.system(size: 11, weight: .semibold, design: .monospaced))
                .foregroundStyle(UITheme.textMuted)
            Text(display)
                .font(.system(size: 11.5, weight: .medium))
                .foregroundStyle((label?.isEmpty == false) ? UITheme.textPrimary : UITheme.textMuted)
                .lineLimit(1)
            Spacer(minLength: 0)
        }
    }
}
