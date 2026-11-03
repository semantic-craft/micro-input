import AppKit
import Foundation

protocol CommandRunning: Sendable {
    func run(executable: String, arguments: [String], timeoutSeconds: TimeInterval) async -> (Bool, String)
}

struct SubprocessCommandRunner: CommandRunning {
    func run(executable: String, arguments: [String], timeoutSeconds: TimeInterval = 2.0) async -> (Bool, String) {
        await AgentAdapters.runProcess(executable: executable, arguments: arguments, timeoutSeconds: timeoutSeconds)
    }
}

enum AgentAdapters {
    static func resolveExecutable(
        envVar: String,
        binaryName: String,
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> String? {
        if let custom = environment[envVar], !custom.isEmpty,
           FileManager.default.isExecutableFile(atPath: custom) {
            return custom
        }
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let candidates = [
            "/opt/homebrew/bin/\(binaryName)",
            "/usr/local/bin/\(binaryName)",
            "\(home)/.cargo/bin/\(binaryName)",
            "\(home)/.local/bin/\(binaryName)",
        ]
        for path in candidates where FileManager.default.isExecutableFile(atPath: path) {
            return path
        }
        if let pathEnv = environment["PATH"] {
            for dir in pathEnv.split(separator: ":") {
                let candidate = "\(dir)/\(binaryName)"
                if FileManager.default.isExecutableFile(atPath: candidate) {
                    return candidate
                }
            }
        }
        return nil
    }

    static func splitArguments(_ command: String) -> [String] {
        command
            .replacingOccurrences(of: #"[\r\n]+"#, with: " ", options: .regularExpression)
            .split(whereSeparator: \.isWhitespace)
            .map(String.init)
    }

    static func stripTrailingNewlines(_ text: String) -> String {
        text.replacingOccurrences(of: #"[\r\n]+$"#, with: "", options: .regularExpression)
    }

    static func runProcess(executable: String, arguments: [String], timeoutSeconds: TimeInterval = 2.0) async -> (Bool, String) {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let process = Process()
                process.executableURL = URL(fileURLWithPath: executable)
                process.arguments = arguments
                let pipe = Pipe()
                process.standardOutput = pipe
                process.standardError = pipe
                do {
                    try process.run()
                } catch {
                    continuation.resume(returning: (false, "进程启动失败: \(error.localizedDescription)"))
                    return
                }
                let readGroup = DispatchGroup()
                readGroup.enter()
                var capturedData = Data()
                let dataLock = NSLock()
                DispatchQueue.global(qos: .userInitiated).async {
                    let data = pipe.fileHandleForReading.readDataToEndOfFile()
                    dataLock.lock()
                    capturedData = data
                    dataLock.unlock()
                    readGroup.leave()
                }
                let deadline = Date().addingTimeInterval(timeoutSeconds)
                while process.isRunning && Date() < deadline {
                    Thread.sleep(forTimeInterval: 0.02)
                }
                if process.isRunning {
                    process.terminate()
                    let grace = Date().addingTimeInterval(0.1)
                    while process.isRunning && Date() < grace {
                        Thread.sleep(forTimeInterval: 0.01)
                    }
                    if process.isRunning {
                        kill(process.processIdentifier, SIGKILL)
                    }
                    process.waitUntilExit()
                    _ = readGroup.wait(timeout: .now() + 0.2)
                    continuation.resume(returning: (false, "CLI 执行超时 (\(Int(timeoutSeconds))s)"))
                    return
                }
                process.waitUntilExit()
                _ = readGroup.wait(timeout: .now() + 1.0)
                dataLock.lock()
                let data = capturedData
                dataLock.unlock()
                let output = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                let ok = process.terminationStatus == 0
                continuation.resume(returning: (ok, ok ? (output.isEmpty ? "ok" : output) : "退出码 \(process.terminationStatus): \(output)"))
            }
        }
    }

    struct AdapterSpec: Sendable {
        let envVar: String
        let binaryName: String
        let focusSubcommand: String
        let executableOverride: String?

        func resolveBin() -> String? {
            if let executableOverride, !executableOverride.isEmpty {
                return executableOverride
            }
            return AgentAdapters.resolveExecutable(envVar: envVar, binaryName: binaryName)
        }

        func executeCommand(_ command: String, runner: CommandRunning) async -> (Bool, String) {
            let args = AgentAdapters.splitArguments(command)
            guard !args.isEmpty else {
                return (false, "缺失 \(binaryName) 子命令")
            }
            guard let bin = resolveBin() else {
                return (false, "未找到 \(binaryName) 可执行文件（可设置 \(envVar)）")
            }
            return await runner.run(executable: bin, arguments: args, timeoutSeconds: 2.0)
        }

        func focusSession(_ session: String, runner: CommandRunning) async -> (Bool, String) {
            let trimmed = session.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty, trimmed != "unselected" else {
                return (false, "缺失目标会话标识")
            }
            guard let bin = resolveBin() else {
                return (false, "未找到 \(binaryName) 可执行文件（可设置 \(envVar)）")
            }
            return await runner.run(executable: bin, arguments: [focusSubcommand, trimmed], timeoutSeconds: 2.0)
        }

        func sendTextNoNewline(session: String, text: String, runner: CommandRunning) async -> (Bool, String) {
            let trimmed = session.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty, trimmed != "unselected" else {
                return (false, "缺失目标会话标识")
            }
            guard let bin = resolveBin() else {
                return (false, "未找到 \(binaryName) 可执行文件（可设置 \(envVar)）")
            }
            let sanitized = AgentAdapters.stripTrailingNewlines(text)
            return await runner.run(executable: bin, arguments: ["pane", "send-text", trimmed, sanitized], timeoutSeconds: 2.0)
        }
    }
}

actor HerdrAdapter {
    static let shared = HerdrAdapter()
    private let runner: CommandRunning
    private let spec: AgentAdapters.AdapterSpec
    private var tail: Task<Void, Never>?

    init(runner: CommandRunning = SubprocessCommandRunner(), executableOverride: String? = nil) {
        self.runner = runner
        self.spec = AgentAdapters.AdapterSpec(
            envVar: "MI_HERDR_BIN",
            binaryName: "herdr",
            focusSubcommand: "focus-session",
            executableOverride: executableOverride
        )
    }

    private func serialized<T: Sendable>(_ operation: @Sendable @escaping () async -> T) async -> T {
        let previous = tail
        let (stream, continuation) = AsyncStream<Void>.makeStream()
        tail = Task {
            await previous?.value
            for await _ in stream { break }
        }
        await previous?.value
        defer { continuation.finish() }
        return await operation()
    }

    func executeCommand(_ command: String) async -> (Bool, String) {
        let spec = self.spec
        let runner = self.runner
        return await serialized {
            await spec.executeCommand(command, runner: runner)
        }
    }

    func focusSession(_ session: String) async -> (Bool, String) {
        let spec = self.spec
        let runner = self.runner
        return await serialized {
            await spec.focusSession(session, runner: runner)
        }
    }

    func sendTextNoNewline(session: String, text: String) async -> (Bool, String) {
        let spec = self.spec
        let runner = self.runner
        return await serialized {
            await spec.sendTextNoNewline(session: session, text: text, runner: runner)
        }
    }
}

actor DshAdapter {
    static let shared = DshAdapter()
    private let runner: CommandRunning
    private let spec: AgentAdapters.AdapterSpec
    private var tail: Task<Void, Never>?

    init(runner: CommandRunning = SubprocessCommandRunner(), executableOverride: String? = nil) {
        self.runner = runner
        self.spec = AgentAdapters.AdapterSpec(
            envVar: "MI_DSH_BIN",
            binaryName: "dsh",
            focusSubcommand: "session",
            executableOverride: executableOverride
        )
    }

    private func serialized<T: Sendable>(_ operation: @Sendable @escaping () async -> T) async -> T {
        let previous = tail
        let (stream, continuation) = AsyncStream<Void>.makeStream()
        tail = Task {
            await previous?.value
            for await _ in stream { break }
        }
        await previous?.value
        defer { continuation.finish() }
        return await operation()
    }

    func executeCommand(_ command: String) async -> (Bool, String) {
        let spec = self.spec
        let runner = self.runner
        return await serialized {
            await spec.executeCommand(command, runner: runner)
        }
    }

    func focusSession(_ session: String) async -> (Bool, String) {
        let spec = self.spec
        let runner = self.runner
        return await serialized {
            await spec.focusSession(session, runner: runner)
        }
    }

    func sendTextNoNewline(session: String, text: String) async -> (Bool, String) {
        let spec = self.spec
        let runner = self.runner
        return await serialized {
            await spec.sendTextNoNewline(session: session, text: text, runner: runner)
        }
    }
}
