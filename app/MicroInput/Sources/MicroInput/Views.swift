// R-A01/R-C05/R-H02/R-N01: original SwiftUI UI; profile selection is local browsing only.
import SwiftUI
import AppKit
struct DeviceListView: View {
    @ObservedObject var store: Store
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Micro Input").font(.title2.bold())
            Text("只读配置查看器").foregroundStyle(.secondary)
            Button("载入演示数据") { Task { await store.loadDemo() } }.disabled(store.busy)
            Divider()
            HStack { Text("USB 候选设备").font(.headline); Spacer(); Button { Task { await store.refresh() } } label: { Image(systemName: "arrow.clockwise") }.help("重新枚举") }
            if store.devices.isEmpty { Text("未发现 Creator Micro 2").foregroundStyle(.secondary) }
            ForEach(store.devices, id: \.id) { d in
                VStack(alignment: .leading) {
                    Text(d.product).font(.headline)
                    Text(String(format: "%04X:%04X · %@", d.vid, d.pid, d.transport)).font(.caption.monospaced())
                    Button(store.connectedID == d.id ? "重新读取（协议待验证）" : "连接并只读查看") { Task { await store.connect(d.id) } }.disabled(store.busy || !store.allowed || d.transport != "Usb")
                }.padding(.vertical, 6)
            }
            Spacer()
            Text("无配置写入、无 Profile 激活。\n宿主动作尚未启用。").font(.caption).foregroundStyle(.secondary)
        }.padding(20).frame(minWidth: 240, idealWidth: 270, maxWidth: 300)
    }
}
struct PermissionGuide: View {
    @ObservedObject var store: Store
    var body: some View {
        HStack {
            Image(systemName: "hand.raised").font(.title2)
            VStack(alignment: .leading) { Text("输入监控未授权").bold(); Text("连接 HID 前请在系统设置中授权，再重新检查。演示模式无需授权。").font(.caption) }
            Spacer()
            Button("打开系统设置") { NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")!) }
            Button("重新检查") { Task { await store.refresh() } }
        }.padding().background(.orange.opacity(0.10))
    }
}
struct ProfileBrowser: View {
    @ObservedObject var store: Store
    var body: some View {
        if let snapshot = store.snapshot {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Picker("Profile", selection: Binding(get: {store.profileIndex}, set: {store.selectProfile($0)})) {
                        ForEach(snapshot.profiles.indices, id: \.self) { i in Text(snapshot.profiles[i].name).tag(i) }
                    }.frame(maxWidth: 300)
                    if let profile = store.profile { Text("id \(profile.id.text) · index \(profile.index)").font(.caption.monospaced()).foregroundStyle(.secondary) }
                    Spacer()
                    Text("活动 Profile：Unknown").font(.caption)
                }
                if let profile = store.profile {
                    Picker("Layer", selection: Binding(get: {store.layerIndex}, set: {store.selectLayer($0)})) {
                        ForEach(profile.layers.indices, id: \.self) { i in Text(profile.layers[i].name).tag(i) }
                    }.pickerStyle(.segmented)
                }
                if let layer = store.layer { LayerView(layer: layer, select: {store.selectedBinding = $0}) }
                if let value = store.selectedBinding { BindingDetail(value: value, detail: snapshot.referenceDetail(value)) }
                if !snapshot.warnings.isEmpty { ForEach(snapshot.warnings, id: \.self) { Text($0).font(.caption).foregroundStyle(.orange) } }
                DisclosureGroup("原始配置与动作（只读）") {
                    Text(snapshot.raw.text).font(.caption.monospaced()).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }
            }.padding(24)
        } else {
            ContentUnavailableView("选择演示或连接设备", systemImage: "keyboard", description: Text("演示浏览由 Rust 解析驱动。设备读取会在未验证协议边界停止。"))
        }
    }
}
struct LayerView: View {
    let layer: Layer
    let select: (JSONValue) -> Void
    private let rowStarts = [0, 2, 6, 10]
    private let rowLengths = [2, 4, 4, 3]
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("13 个键位").font(.headline)
            ForEach(0..<4, id: \.self) { row in
                HStack(spacing: 8) {
                    ForEach(rowStarts[row]..<(rowStarts[row] + rowLengths[row]), id: \.self) { i in
                        if layer.keys.indices.contains(i) {
                            Button { select(layer.keys[i]) } label: { VStack(alignment: .leading, spacing: 8) { Text("\(i + 1)").font(.caption).foregroundStyle(.secondary); Text(layer.keys[i].bindingLabel).font(.caption.monospaced()).lineLimit(2).frame(maxWidth: .infinity, alignment: .leading) }.padding(10).frame(width: 138, height: 65).background(.quaternary, in: RoundedRectangle(cornerRadius: 9)) }.buttonStyle(.plain)
                        }
                    }
                }
            }
            Text("旋钮 · 3 槽").font(.headline)
            HStack { ForEach(layer.encoders.indices, id: \.self) { i in Button("\(["逆时针", "顺时针", "按下"][i]) · \(layer.encoders[i].bindingLabel)") { select(layer.encoders[i]) } } }
            Text("摇杆 · \(layer.joystick.kind.text) · \(layer.joystick.sectors.count) 个扇区").font(.headline)
            ForEach(layer.joystick.sectors.indices, id: \.self) { i in Button("扇区 \(i + 1) · \(layer.joystick.sectors[i].bindingLabel)") { select(layer.joystick.sectors[i]) } }
            DisclosureGroup("灯光原始值（不预览）") { Text(layer.lights.text).font(.caption.monospaced()) }
        }
    }
}
struct BindingDetail: View {
    let value: JSONValue
    let detail: String?
    var body: some View { VStack(alignment: .leading) { Text("绑定详情").font(.headline); Text(value.bindingLabel).font(.body.monospaced()).textSelection(.enabled); if let detail { Text(detail).font(.caption.monospaced()).textSelection(.enabled) } }.frame(maxWidth: .infinity, alignment: .leading).padding().background(.blue.opacity(0.06), in: RoundedRectangle(cornerRadius: 8)) }
}
struct StatusBar: View {
    @ObservedObject var store: Store
    var body: some View {
        HStack {
            if store.busy { ProgressView().controlSize(.small) }
            Text(store.status).lineLimit(2)
            Spacer()
            if let s = store.snapshot {
                Text(s.source == "Demo" ? "演示数据" : "设备快照")
                Text(Date(timeIntervalSince1970: Double(s.read_at)), style: .time)
                if store.stale { Text("已过期").foregroundStyle(.orange) }
            }
        }.font(.caption).padding(12).background(.bar)
    }
}
