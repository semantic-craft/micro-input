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
        .frame(minWidth: 1020, minHeight: 680)
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

struct CheatSheetOverlayView: View {
    let store: CoreStore
    let sheet: CheatSheetModel

    var body: some View {
        ZStack {
            Color.black.opacity(0.32)
                .ignoresSafeArea()
                .contentShape(Rectangle())
                .onTapGesture {
                    store.dispatch("overlay", "CloseCheatSheet")
                }

            VStack(alignment: .leading, spacing: 14) {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(sheet.profileName) · 按键速查面板")
                            .font(UITheme.title)
                            .foregroundStyle(UITheme.textPrimary)
                        Text(sheet.routingSummary)
                            .font(UITheme.caption)
                            .foregroundStyle(UITheme.textSecondary)
                    }
                    Spacer()
                    Button("关闭") {
                        store.dispatch("overlay", "CloseCheatSheet")
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(UITheme.accent)
                }

                LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 8)], spacing: 8) {
                    ForEach(sheet.keyCells) { cell in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text(cell.bindingId)
                                    .font(UITheme.monoBadge)
                                    .foregroundStyle(UITheme.textMuted)
                                Spacer()
                                Text(cell.controlName)
                                    .font(UITheme.micro)
                                    .foregroundStyle(UITheme.textSecondary)
                            }
                            Text(cell.label.isEmpty ? "未绑定" : cell.label)
                                .font(UITheme.bodySemibold)
                                .foregroundStyle(cell.available ? UITheme.textPrimary : UITheme.textMuted)
                                .lineLimit(1)
                        }
                        .padding(10)
                        .background(
                            RoundedRectangle(cornerRadius: UITheme.controlCornerRadius, style: .continuous)
                                .fill(UITheme.subtleSurface)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: UITheme.controlCornerRadius, style: .continuous)
                                .stroke(UITheme.border, lineWidth: 1)
                        )
                    }
                }
            }
            .padding(20)
            .frame(maxWidth: 760)
            .background(
                RoundedRectangle(cornerRadius: UITheme.cardCornerRadius, style: .continuous)
                    .fill(UITheme.surface)
                    .shadow(radius: 24)
            )
            .overlay(
                RoundedRectangle(cornerRadius: UITheme.cardCornerRadius, style: .continuous)
                    .stroke(UITheme.strongBorder, lineWidth: 1)
            )
        }
    }
}
