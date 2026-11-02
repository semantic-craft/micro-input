import AppKit

// NSWorkspace posts these on its own notification center, not NotificationCenter.default.
// Session switch notifications are not treated as screen-lock notifications.
@MainActor final class WorkspaceSleepObserver {
    private let center: NotificationCenter
    private var observers: [NSObjectProtocol] = []
    private var wakeTask: Task<Void, Never>?
    private var active = true

    init(store: Store, center: NotificationCenter = NSWorkspace.shared.notificationCenter) {
        self.center = center
        observers.append(center.addObserver(forName: NSWorkspace.willSleepNotification, object: nil, queue: .main) { [weak self, weak store] _ in
            MainActor.assumeIsolated {
                guard let self, self.active else { return }
                self.wakeTask?.cancel()
                store?.systemWillSleep()
            }
        })
        observers.append(center.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self, weak store] _ in
            MainActor.assumeIsolated {
                guard let self, self.active else { return }
                self.wakeTask?.cancel()
                self.wakeTask = Task { await store?.systemDidWake() }
            }
        })
    }

    func stop() {
        active = false
        wakeTask?.cancel(); wakeTask = nil
        observers.forEach { center.removeObserver($0) }
        observers = []
    }

    deinit {
        wakeTask?.cancel()
        observers.forEach { center.removeObserver($0) }
    }
}
