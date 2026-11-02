// R-P03/R-H11: offline first-round workbench; no discovery, permission request or device open on launch.
import SwiftUI
import MicroInputCore
@main struct MicroInputApp: App {
    @StateObject private var store = Store(backend: MicroInput())
    var body: some Scene {
        Window("Micro Input", id: "main") { Workbench(store: store) }.defaultSize(width: 1180, height: 730)
        MenuBarExtra("Micro Input", systemImage: "keyboard") { MenuContent(store: store) }
    }
}
struct MenuContent: View {
    @ObservedObject var store: Store
    @Environment(\.openWindow) private var openWindow
    var body: some View {
        Text("离线演示 · 不激活设备配置")
        if let snapshot = store.snapshot {
            ForEach(snapshot.profiles.indices, id: \.self) { i in Button(snapshot.profiles[i].name) { store.selectProfile(i); openWindow(id: "main") } }
        }
        Button("打开工作台") {openWindow(id: "main"); NSApp.activate(ignoringOtherApps: true)}
        Divider()
        Button("退出 Micro Input") {NSApp.terminate(nil)}
    }
}
