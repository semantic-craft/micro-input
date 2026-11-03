import SwiftUI

struct RecoveryPreview: Decodable, Identifiable {
    var id: String { token }
    let token: String
    let path: String
    let sha256: String
    let schema_version: UInt64?
    let source_kind: String
    let profiles: Int
    let revision: UInt64?
    let can_restore: Bool
    let message: String
}

struct DocumentRecoveryMenu: View {
    @ObservedObject var store: Store
    @State private var showing = false
    @State private var saveConfirm = false
    var body: some View {
        Menu("本地文档") {
            Button("恢复或保留文件副本…") { showing = true }
            Button("另存当前草稿为独立文档…") { saveConfirm = true }.disabled(!store.canEditDraft)
        }.disabled(store.busy || store.browsingDeviceSnapshot)
        .sheet(isPresented: $showing) { DocumentRecoveryPanel(store: store) }
        .alert("另存当前草稿", isPresented: $saveConfirm) {
            Button("取消", role: .cancel) {}
            Button("另存并继续编辑") { Task { await store.saveDocumentAs() } }
        } message: {
            Text("在本机文档目录创建独立 v2 文档并切换到它。原文件与中断记录保留；重开继续新文档。当前会话撤销历史清空，不写设备。")
        }
    }
}

private struct DocumentRecoveryPanel: View {
    @ObservedObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State private var files: [String] = []
    @State private var preview: RecoveryPreview?
    @State private var failure: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("恢复本地文档").font(.title2)
            Text("恢复为独立草稿，保留当前文件与来源副本；本地恢复不等同设备恢复。").font(.callout)
            Text("保存保护提示：\(store.status)").font(.caption).foregroundStyle(.secondary)
            Text("本机文档与恢复副本").font(.headline)
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(files, id: \.self) { path in
                        HStack {
                            VStack(alignment: .leading) {
                                Text(URL(fileURLWithPath:path).lastPathComponent).lineLimit(1)
                                Text(path).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                            }
                            Spacer()
                            Button("检查") { Task { await inspect(path) } }
                                .buttonStyle(.bordered).accessibilityLabel("检查 \(URL(fileURLWithPath:path).lastPathComponent)")
                        }
                    }
                }.padding(8)
            }.frame(height: 130)
            HStack {
                Button("选择其他来源文件…") { chooseSource() }
                Button("刷新列表") { Task { await refresh() } }
            }
            if let item = preview {
                Divider()
                Text(URL(fileURLWithPath:item.path).lastPathComponent).font(.headline)
                Text("版本：\(item.schema_version.map(String.init) ?? "未知") · 历史来源：\(item.source_kind) · 修订：\(item.revision.map(String.init) ?? "未知") · \(item.profiles) 个配置")
                Text("SHA-256：\(item.sha256)").font(.caption2).textSelection(.enabled)
                Text(item.message).foregroundStyle(item.can_restore ? Color.primary : Color.orange)
                if item.schema_version == 1 && item.can_restore { Text("复用 v1 → v2 转换；旧文件保留原字节，历史来源不作为当前设备快照。").font(.caption) }
                HStack {
                    Button("保留原始字节副本…") { Task { await copyOriginal(item) } }
                    Spacer()
                    Button("恢复为独立草稿") {
                        Task { if await store.recoverDocument(token:item.token) { dismiss() } else { failure = store.status } }
                    }.disabled(!item.can_restore)
                }
            } else { Text("选择文件后先核对来源与版本。未知格式仅可保留原始副本。").foregroundStyle(.secondary) }
            if let failure { Text(failure).foregroundStyle(.red).textSelection(.enabled) }
            HStack { Spacer(); Button("关闭") { dismiss() } }
        }.padding(24).frame(width: 620).disabled(store.busy).interactiveDismissDisabled(store.busy)
        .task { await refresh() }
    }
    private func refresh() async {
        do { files = try await store.recoveryFiles() } catch { failure = readableError(error) }
    }
    private func inspect(_ path: String) async {
        preview = nil; failure = nil
        do { preview = try await store.inspectRecovery(path:path) } catch { failure = readableError(error) }
    }
    private func chooseSource() {
        let panel = NSOpenPanel(); panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { await inspect(url.path) }
    }
    private func copyOriginal(_ item: RecoveryPreview) async {
        let panel = NSSavePanel(); panel.nameFieldStringValue = URL(fileURLWithPath:item.path).lastPathComponent + ".copy"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do { try await store.copyRecoveryOriginal(token:item.token, destination:url.path); failure = nil }
        catch { failure = readableError(error) }
    }
}
