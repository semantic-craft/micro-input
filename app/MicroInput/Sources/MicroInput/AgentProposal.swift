import SwiftUI

struct AgentProposalPreview: Decodable, Identifiable {
    var id: String { token }
    let token: String
    let changes: [String]
    let base: AgentProposalBase
}
struct AgentProposalBase: Decodable {
    let document_id: String
    let revision: UInt64
    let sha256: String
}

struct AgentProposalButton: View {
    @ObservedObject var store: Store
    @State private var preview: AgentProposalPreview?
    @State private var failure: String?
    var body: some View {
        Button("审阅 Agent 提案…") { choose() }.disabled(!store.canEditDraft)
            .sheet(item: $preview, onDismiss: { Task { await store.rejectAgentProposal() } }) { item in
                VStack(alignment: .leading, spacing: 14) {
                    Text("审阅 Agent 提案").font(.title2)
                    Text("CLI 准备的本地变化；接受后创建独立草稿，原文档保留。不会写设备或执行动作。")
                    Text("来源文档：\(item.base.document_id) · 修订 \(item.base.revision)").font(.caption).textSelection(.enabled)
                    Text("SHA-256：\(item.base.sha256)").font(.caption2).textSelection(.enabled)
                    ScrollView {
                        VStack(alignment: .leading, spacing: 10) {
                            ForEach(Array(item.changes.enumerated()), id: \.offset) { _, change in Text(change).textSelection(.enabled) }
                        }.frame(maxWidth: .infinity, alignment: .leading)
                    }.frame(maxHeight: 260)
                    Text("设备基线仅是此本地文档保存的历史来源，不代表当前设备状态。接受前会再次核对来源及修订。")
                        .font(.caption).foregroundStyle(.secondary)
                    HStack {
                        Button("拒绝提案", role: .cancel) { Task { await store.rejectAgentProposal(); preview = nil } }
                        Spacer()
                        Button("接受为独立草稿") { Task {
                            if await store.acceptAgentProposal(token: item.token) { preview = nil }
                            else { failure = store.status }
                        } }.buttonStyle(.borderedProminent)
                    }
                }.padding(24).frame(width: 650).disabled(store.busy).interactiveDismissDisabled(store.busy)
            }
            .alert("Agent 提案未接受", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })) {
                Button("好", role: .cancel) { failure = nil }
            } message: { Text(failure ?? "") }
    }
    private func choose() {
        let panel = NSOpenPanel(); panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        panel.message = "选择 mi draft patch 生成的提案 JSON，先审阅再决定是否接受。"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task {
            do {
                let handle = try FileHandle(forReadingFrom: url); defer { try? handle.close() }
                let data = try handle.read(upToCount: 4 * 1024 * 1024 + 1) ?? Data()
                guard data.count <= 4 * 1024 * 1024, let text = String(data:data,encoding:.utf8) else {
                    throw CocoaError(.fileReadCorruptFile)
                }
                preview = try await store.inspectAgentProposal(text)
            } catch { failure = readableError(error) }
        }
    }
}
