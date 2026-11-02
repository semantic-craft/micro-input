import SwiftUI
import UniformTypeIdentifiers

private struct PendingPreset: Identifiable {
    let id = UUID()
    let text: String
    let summary: PresetSummary
}

struct PresetMenu: View {
    @ObservedObject var store: Store
    @State private var pending: PendingPreset?
    @State private var failure: String?
    @State private var importFailure: String?
    @State private var exportScope = "document"
    @State private var exportConfirm = false
    var body: some View {
        Menu("导入与导出") {
            Button("导入自有预设…") { chooseImport() }
            Button("安装 Coding 起步预设…") {
                do { try prepare(store.starterPreset()) } catch { failure = readableError(error) }
            }
            Divider()
            Button("导出整个本地文档…") { exportScope = "document"; exportConfirm = true }
            Button("导出当前配置…") { exportScope = "profile"; exportConfirm = true }.disabled(store.profile == nil)
            Button("导出当前层…") { exportScope = "layer"; exportConfirm = true }.disabled(store.layer == nil)
        }
        .disabled(!store.canEditDraft || store.busy)
        .sheet(item: $pending) { item in
            VStack(alignment: .leading, spacing: 16) {
                Text("检查导入内容").font(.title2)
                Text(item.summary.name).font(.headline)
                Text("\(item.summary.profiles) 个配置 · \(item.summary.layers) 个层 · \(item.summary.actions) 个本地动作")
                Text("创建独立草稿，所有本地对象重新编号；同名配置留在各自文档中。原文档保留，重开继续新草稿。")
                Text("来源档案仅作历史记录，不作为设备快照或应用基线。不会激活设备或执行动作。")
                if let importFailure { Text(importFailure).foregroundStyle(.red) }
                HStack {
                    Button("取消") { pending = nil }.disabled(store.busy)
                    Spacer()
                    Button("创建独立草稿") {
                        Task { if await store.importPreset(item.text) { pending = nil } else { importFailure = store.status } }
                    }.disabled(store.busy)
                }
            }.padding(24).frame(width: 470).interactiveDismissDisabled(store.busy)
        }
        .alert("无法处理预设", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })) {
            Button("好") { failure = nil }
        } message: { Text(failure ?? "") }
        .alert("导出为自有格式", isPresented: $exportConfirm, presenting: exportScope) { scope in
            Button("取消", role: .cancel) {}
            Button("选择保存位置…") {
                Task { await export(scope) }
            }
        } message: { _ in
            Text("即使只导出配置或层，文件也会包含完整原始来源档案，可能含其他配置及文本参数。请检查后再分享；这不是厂商交换格式。")
        }
    }
    private func prepare(_ text: String) throws {
        importFailure = nil
        pending = PendingPreset(text: text, summary: try store.inspectPreset(text))
    }
    private func chooseImport() {
        let panel = NSOpenPanel(); panel.allowedContentTypes = [.json]; panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let handle = try FileHandle(forReadingFrom: url); defer { try? handle.close() }
            let data = try handle.read(upToCount: 4 * 1024 * 1024 + 1) ?? Data()
            guard data.count <= 4 * 1024 * 1024, let text = String(data: data, encoding: .utf8) else {
                throw CocoaError(.fileReadCorruptFile)
            }
            try prepare(text)
        } catch { failure = readableError(error) }
    }
    private func export(_ scope: String) async {
        do {
            let text = try await store.presetText(scope: scope)
            let panel = NSSavePanel(); panel.allowedContentTypes = [.json]; panel.nameFieldStringValue = "micro-input-\(scope).json"
            guard panel.runModal() == .OK, let url = panel.url else { return }
            try Data(text.utf8).write(to: url, options: .atomic)
            store.status = "已导出自有预设 · 包含完整来源档案 · 非厂商格式"
        } catch { failure = readableError(error) }
    }
}
