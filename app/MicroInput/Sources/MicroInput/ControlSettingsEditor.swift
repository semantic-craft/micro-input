// Local document intent; physical control mapping and firmware modes remain unverified.
import SwiftUI

struct ControlSettingsEditor: View {
    @ObservedObject var store: Store
    @State private var mode: String
    @State private var sectors: String
    private let supported: Bool
    init(store: Store) {
        self.store = store
        let settings = store.currentControlSettings
        let currentMode = settings?.joystick_mode ?? "source"
        supported = ["source", "logical_sectors"].contains(currentMode)
        _mode = State(initialValue: currentMode)
        _sectors = State(initialValue: String(settings?.active_sectors ?? UInt32(min(32, store.layer?.joystick.sectors.count ?? 0))))
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("旋钮与摇杆 · 本地意图").font(.headline)
            Text("旋钮槽 1–3 可分别分配动作；旋转方向与按压对应关系未核实。摇杆扇区同样使用逻辑编号。").font(.caption).foregroundStyle(.secondary)
            Text("来源摇杆模式：\(store.layer?.joystick.kind.text ?? "未知")").font(.caption).textSelection(.enabled)
            if supported {
                Picker("摇杆编辑模式", selection: $mode) {
                    Text("保留来源模式").tag("source")
                    Text("逻辑扇区意图").tag("logical_sectors")
                }
                Text("活动扇区数").font(.caption)
                TextField("活动扇区数", text: $sectors).accessibilityLabel("活动扇区数")
                    .disabled(mode != "logical_sectors")
                Text("0–32 是本机编辑器限制，不代表设备容量。减少数量或切换模式会保留所有旧槽位及绑定，可继续查看和编辑。").font(.caption).foregroundStyle(.secondary)
                if let error = store.controlSettingsError { Text(error).font(.caption).foregroundStyle(.red).accessibilityLabel(error) }
                Button("保存控件意图") { Task { await store.saveControlSettings(mode: mode, sectors: sectors) } }
                    .disabled(!store.canEditDraft || store.busy)
            } else {
                Text("未知本地模式：\(mode) · 原值保留，设置只读").font(.caption).textSelection(.enabled)
            }
        }
    }
}
