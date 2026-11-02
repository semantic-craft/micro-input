import SwiftUI
import MicroInputCore

@MainActor final class InputMonitorState: ObservableObject {
    @Published private(set) var count: UInt64 = 0
    @Published private(set) var lost: UInt64 = 0
    @Published private(set) var lastEvent: InputEventInfo?
    @Published private(set) var status = "连接设备后持续监听"
    @Published private(set) var paused = false
    private var subscription: InputSubscription?
    private var generation = 0

    func stop() {
        generation += 1
        subscription?.close(); subscription = nil
    }
    func listen(store: Store) async {
        stop()
        let token = generation
        count = 0; lost = 0; lastEvent = nil; paused = false
        guard store.connectedID != nil else { status = "连接设备后持续监听"; return }
        do {
            let stream = try await store.subscribeInput()
            guard token == generation, !Task.isCancelled else { stream.close(); return }
            subscription = stream
            status = "正在监听 · 控件输入格式待核实"
            await withTaskCancellationHandler(operation: {
                while !Task.isCancelled, let event = await stream.nextEvent() {
                    guard token == generation else { break }
                    lastEvent = event
                    if event.kind == .inputStreamGap {
                        lost += event.lostCount; paused = true
                        status = "输入中断 · 已清除按住状态，请重建监听"
                        break
                    }
                    count += 1
                }
            }, onCancel: { stream.close() })
            stream.close()
            if token == generation {
                subscription = nil
                if !paused { status = "监听已停止 · 可重新连接或重建监听" }
            }
        } catch {
            if token == generation { status = "监听不可用 · \(readableError(error))" }
        }
    }
}
struct InputMonitorPanel: View {
    @ObservedObject var store: Store
    @StateObject private var monitor = InputMonitorState()
    @State private var revision = 0
    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            Label("输入监视", systemImage: "waveform.path").font(.caption.bold())
            VStack(alignment: .leading, spacing: 4) {
                Text(monitor.status)
                Text("已收 \(monitor.count) 条通知 · 丢失 \(monitor.lost) 条 · 不执行动作")
                    .foregroundStyle(.secondary)
                if let event = monitor.lastEvent {
                    Text(event.kind == .inputStreamGap ? "事件流有缺口" : "未知通知 · 物理控件、键帽覆盖和动作角色尚未核实")
                    Text("会话 \(event.sessionId) · 本地序号 \(event.sequence) · 接收时间 \(Date(timeIntervalSince1970: Double(event.timestampMs) / 1000).formatted(date: .omitted, time: .standard))")
                        .foregroundStyle(.secondary)
                }
            }.font(.caption)
            Spacer()
            Button("重建监听") { revision += 1 }
                .disabled(store.connectedID == nil)
        }.padding(.horizontal, 20).padding(.vertical, 8)
        .task(id: "\(store.connectedID ?? "")/\(revision)") { await monitor.listen(store: store) }
        .onDisappear { monitor.stop() }
    }
}
