import SwiftUI
import MicroInputCore
@main struct MicroInputApp: App {
    private let core: MicroInput
    @StateObject private var store: Store
    init() { let core = MicroInput(); self.core = core; _store = StateObject(wrappedValue: Store(backend: core)) }
    var body: some Scene {
        Window("Micro Input", id: "main") {
            VStack(spacing: 0) {
                if !store.allowed { PermissionGuide(store: store) }
                HSplitView { DeviceListView(store: store); ScrollView { ProfileBrowser(store: store) }.frame(maxWidth: .infinity, maxHeight: .infinity) }
                StatusBar(store: store)
            }.frame(minWidth: 1050, minHeight: 740)
            .task { await core.subscribeDeviceEvents(cb: Events(store)); await store.refresh() }
            .onReceive(DistributedNotificationCenter.default().publisher(for: Notification.Name("com.apple.screenIsLocked"))) { _ in Task { await store.suspend() } }
            .onReceive(DistributedNotificationCenter.default().publisher(for: Notification.Name("com.apple.screenIsUnlocked"))) { _ in Task { await store.refresh() } }
            .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in Task { await store.suspend() } }
        }.defaultSize(width: 1150, height: 850)
    }
}
