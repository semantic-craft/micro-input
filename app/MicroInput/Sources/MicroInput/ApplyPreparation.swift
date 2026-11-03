import SwiftUI
struct ApplyPreview: Decodable, Identifiable {
    var id: String { token }
    let token: String, document_id: String, revision: UInt64, document_sha256: String
    let device_identity: JSONValue, firmware: JSONValue, files: JSONValue, active_indices: JSONValue
    let active_source: String, profile_source_id: JSONValue, layer_source_id: JSONValue
    let logical_control: String, physical_position: String, before: String, after: String, planned_keymap_sha256: String
    let apply_available: Bool, restore_available: Bool, unavailable_reason: String
}
struct PreparedApplyBackup: Decodable {
    let token: String, backup_path: String, manifest_sha256: String, status: String, unavailable_reason: String
}
struct ApplyPreparationView: View {
    @ObservedObject var store: Store
    let preview: ApplyPreview
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack { Text("基础绑定应用准备").font(.title2.bold()); Spacer(); Button("关闭准备") { Task { await store.cancelApplyPreview() } } }
            Text("\(preview.before) → \(preview.after)").font(.title3.monospaced())
            Text("来源 Profile \(preview.profile_source_id.text) / Layer \(preview.layer_source_id.text) / \(preview.logical_control)")
            Text("实体位置：\(preview.physical_position) · 活动来源：\(preview.active_source)").font(.caption).foregroundStyle(.secondary)
            Text("文档 \(preview.document_id) · 修订 \(preview.revision)").font(.caption).textSelection(.enabled)
            Text("固件 \(preview.firmware.text) · 预览只使用本会话完整核验快照").font(.caption)
            DisclosureGroup("设备、活动索引与文件摘要") {
                ScrollView { Text("设备：\(preview.device_identity.text)\n活动索引：\(preview.active_indices.text)\n文件：\(preview.files.text)\n文档 SHA-256：\(preview.document_sha256)\n计划 keymap SHA-256：\(preview.planned_keymap_sha256)").font(.caption.monospaced()).textSelection(.enabled).frame(maxWidth:.infinity,alignment:.leading) }.frame(maxHeight:180)
            }
            if let prepared = store.preparedApplyBackup, prepared.token == preview.token {
                Label("本机副本已保存并读回核验 · 尚未发送", systemImage:"externaldrive.badge.checkmark")
                Text(prepared.backup_path).font(.caption).textSelection(.enabled)
                Text("Manifest SHA-256：\(prepared.manifest_sha256)").font(.caption).textSelection(.enabled)
                Text("此副本尚未经过设备恢复验收。").font(.caption).foregroundStyle(.secondary)
            } else {
                Button("保存原配置副本到本机") { Task { await store.prepareApplyBackup() } }.disabled(store.busy)
            }
            Text(preview.unavailable_reason).font(.callout)
            HStack {
                Button("应用到设备") { Task { await store.performDeviceOperation(token: preview.token) } }
                    .disabled(!preview.apply_available || store.preparedApplyBackup?.token != preview.token || store.deviceOperationRunning)
                Button("设备操作与恢复…") { Task { await store.cancelApplyPreview(); store.showingDeviceOperations = true } }
            }
            if store.deviceOperationRunning { Button("停止后续发送") { store.cancelActiveDeviceOperation() } }
            if let result = store.operationResult { Text(result.title).font(.headline) }
            Text(store.status).font(.caption).foregroundStyle(.secondary)
        }.padding(24).frame(width:680)
    }
}
