#if MI_SYNTHETIC_BACKEND
import Foundation
import MicroInputCore

enum CindyOpenOverride: Sendable {
    case normal
    case deliveredOnly
    case wrongSelected(CindyTaskAddress)
    case missingSelected
    case rotateGeneration(UInt64)
    case rotateOwner(String, UInt64)
}

actor CindyNavigationFixture: CindySessionServing {
    static let shared = CindyNavigationFixture()

    private var instanceId = "cindy-instance-1"
    private var generation: UInt64 = 1
    private var ownerEpoch = "owner-account-1:1"
    private var scopes: [CindyScope] = [
        CindyScope(workspacePath: "/Users/example/Projects/alpha"),
        CindyScope(workspacePath: "/Users/example/Projects/beta"),
    ]
    private var allRows: [CindyTaskRow] = [
        CindyTaskRow(
            address: CindyTaskAddress(
                workspacePath: "/Users/example/Projects/alpha",
                sessionId: "sess-alpha-1"
            ),
            title: "Daily Standup",
            phase: "running",
            attention: false,
            pinned: true
        ),
        CindyTaskRow(
            address: CindyTaskAddress(
                workspacePath: "/Users/example/Projects/alpha",
                sessionId: "sess-alpha-2"
            ),
            title: "Daily Standup",
            phase: "needs-interaction",
            attention: true,
            pinned: false
        ),
        CindyTaskRow(
            address: CindyTaskAddress(
                workspacePath: "/Users/example/Projects/beta",
                sessionId: "sess-beta-1"
            ),
            title: "Daily Standup",
            phase: "idle",
            attention: false,
            pinned: false
        ),
        CindyTaskRow(
            address: CindyTaskAddress(
                workspacePath: "/Users/example/Projects/secret",
                sessionId: "sess-secret-9"
            ),
            title: "Secret Workspace Task",
            phase: "running",
            attention: true,
            pinned: false
        ),
    ]
    private var delivered: CindyTaskAddress?
    private var selected: CindyTaskAddress? = CindyTaskAddress(
        workspacePath: "/Users/example/Projects/alpha",
        sessionId: "sess-alpha-1"
    )
    private var openOverride: CindyOpenOverride = .normal
    private var authorizedMetadataReads = 0
    private var unauthorizedMetadataReads = 0
    private var openHistory: [CindyTaskAddress] = []

    func reset() {
        instanceId = "cindy-instance-1"
        generation = 1
        ownerEpoch = "owner-account-1:1"
        scopes = [
            CindyScope(workspacePath: "/Users/example/Projects/alpha"),
            CindyScope(workspacePath: "/Users/example/Projects/beta"),
        ]
        allRows = [
            CindyTaskRow(
                address: CindyTaskAddress(
                    workspacePath: "/Users/example/Projects/alpha",
                    sessionId: "sess-alpha-1"
                ),
                title: "Daily Standup",
                phase: "running",
                attention: false,
                pinned: true
            ),
            CindyTaskRow(
                address: CindyTaskAddress(
                    workspacePath: "/Users/example/Projects/alpha",
                    sessionId: "sess-alpha-2"
                ),
                title: "Daily Standup",
                phase: "needs-interaction",
                attention: true,
                pinned: false
            ),
            CindyTaskRow(
                address: CindyTaskAddress(
                    workspacePath: "/Users/example/Projects/beta",
                    sessionId: "sess-beta-1"
                ),
                title: "Daily Standup",
                phase: "idle",
                attention: false,
                pinned: false
            ),
            CindyTaskRow(
                address: CindyTaskAddress(
                    workspacePath: "/Users/example/Projects/secret",
                    sessionId: "sess-secret-9"
                ),
                title: "Secret Workspace Task",
                phase: "running",
                attention: true,
                pinned: false
            ),
        ]
        delivered = nil
        selected = CindyTaskAddress(
            workspacePath: "/Users/example/Projects/alpha",
            sessionId: "sess-alpha-1"
        )
        openOverride = .normal
        authorizedMetadataReads = 0
        unauthorizedMetadataReads = 0
        openHistory = []
    }

    func setOpenOverride(_ overrideMode: CindyOpenOverride) {
        openOverride = overrideMode
    }

    func removeTask(_ address: CindyTaskAddress) {
        allRows.removeAll { $0.address == address }
        if delivered == address {
            delivered = nil
        }
        if selected == address {
            selected = nil
        }
    }

    func stats() -> (authorizedReads: Int, unauthorizedReads: Int, openHistory: [CindyTaskAddress]) {
        (authorizedMetadataReads, unauthorizedMetadataReads, openHistory)
    }

    func connect() async -> Result<CindySessionSnapshot, CindyFailure> {
        await read()
    }

    func read() async -> Result<CindySessionSnapshot, CindyFailure> {
        authorizedMetadataReads += 1
        let allowedPaths = Set(scopes.map(\.workspacePath))
        let scopedRows = allRows.filter { allowedPaths.contains($0.address.workspacePath) }
        let scopedSelected = selected.flatMap { current in
            scopedRows.contains(where: { $0.address == current }) ? current : nil
        }
        return .success(
            CindySessionSnapshot(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                scopes: scopes,
                rows: scopedRows,
                selected: scopedSelected
            )
        )
    }

    func open(_ selection: CindySelection) async -> CindyOpenReply {
        let allowedPaths = Set(scopes.map(\.workspacePath))
        guard allowedPaths.contains(selection.address.workspacePath) else {
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .denied,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }

        switch openOverride {
        case .rotateGeneration(let nextGeneration):
            generation = nextGeneration
            openOverride = .normal
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        case .rotateOwner(let nextOwnerEpoch, let nextGeneration):
            ownerEpoch = nextOwnerEpoch
            generation = nextGeneration
            openOverride = .normal
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        case .wrongSelected(let wrongAddress):
            openOverride = .normal
            delivered = selection.address
            selected = wrongAddress
            openHistory.append(selection.address)
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .confirmed,
                delivered: selection.address,
                selected: wrongAddress,
                deepLink: selection.address.deepLink
            )
        case .missingSelected:
            openOverride = .normal
            delivered = selection.address
            selected = nil
            openHistory.append(selection.address)
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .unconfirmed,
                delivered: selection.address,
                selected: nil,
                deepLink: selection.address.deepLink
            )
        case .deliveredOnly:
            openOverride = .normal
            guard selection.instanceId == instanceId,
                  selection.generation == generation,
                  selection.ownerEpoch == ownerEpoch else {
                return CindyOpenReply(
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .stale,
                    delivered: nil,
                    selected: nil,
                    deepLink: nil
                )
            }
            authorizedMetadataReads += 1
            guard allRows.contains(where: { $0.address == selection.address }) else {
                return CindyOpenReply(
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .stale,
                    delivered: nil,
                    selected: nil,
                    deepLink: nil
                )
            }
            delivered = selection.address
            selected = nil
            openHistory.append(selection.address)
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .delivered,
                delivered: selection.address,
                selected: nil,
                deepLink: selection.address.deepLink
            )
        case .normal:
            break
        }

        guard selection.instanceId == instanceId,
              selection.generation == generation,
              selection.ownerEpoch == ownerEpoch else {
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }

        authorizedMetadataReads += 1
        guard allRows.contains(where: { $0.address == selection.address }) else {
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }

        delivered = selection.address
        selected = selection.address
        openHistory.append(selection.address)
        return CindyOpenReply(
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            status: .confirmed,
            delivered: selection.address,
            selected: selection.address,
            deepLink: selection.address.deepLink
        )
    }

    func close() async {}
}

struct CindyScenarioCheckReceipt: Codable {
    var name: String
    var status: String
    var detail: String
}

struct CindyNavigationScenarioReceipt: Codable {
    var scenario: String
    var status: String
    var unauthorized_metadata_reads: Int
    var authorized_metadata_reads: Int
    var local_agent_api_relaxed: Bool
    var checks: [CindyScenarioCheckReceipt]
    var native_app: String
    var physical_key: String
}

extension Store {
    func prepareCindyNavigationScenario() async {
        let fixture = CindyNavigationFixture.shared
        await fixture.reset()

        func queuedOpen(_ selection: CindySelection) async {
            _ = openCindySession(selection)
            await hostExecutionTask?.value
        }

        var checks: [CindyScenarioCheckReceipt] = []

        await installCodingAgentPresets()
        if let profile = hostProfiles.first(where: { $0.name == "Cindy" }) {
            await setHostActiveProfile(profile.id.text)
        }

        let initialStats = await fixture.stats()
        let disabledZeroReadsPass =
            !cindySessions.enabled &&
            cindySessions.state == .disabled &&
            initialStats.authorizedReads == 0 &&
            initialStats.unauthorizedReads == 0
        checks.append(
            CindyScenarioCheckReceipt(
                name: "disabled_zero_reads",
                status: disabledZeroReadsPass ? "PASS" : "FAIL",
                detail: "enabled=\(cindySessions.enabled) authorized_reads=\(initialStats.authorizedReads)"
            )
        )

        cindySessions.setEnabled(true)
        await cindySessions.refreshNow()

        let alpha1 = CindyTaskAddress(
            workspacePath: "/Users/example/Projects/alpha",
            sessionId: "sess-alpha-1"
        )
        let alpha2 = CindyTaskAddress(
            workspacePath: "/Users/example/Projects/alpha",
            sessionId: "sess-alpha-2"
        )
        let beta1 = CindyTaskAddress(
            workspacePath: "/Users/example/Projects/beta",
            sessionId: "sess-beta-1"
        )
        let secret = CindyTaskAddress(
            workspacePath: "/Users/example/Projects/secret",
            sessionId: "sess-secret-9"
        )

        let scopedCatalogPass =
            cindySessions.state == .ready &&
            cindySessions.rows.map(\.address) == [alpha1, alpha2, beta1] &&
            !cindySessions.rows.contains(where: { $0.address == secret })
        checks.append(
            CindyScenarioCheckReceipt(
                name: "scoped_catalog_only",
                status: scopedCatalogPass ? "PASS" : "FAIL",
                detail: "rows=\(cindySessions.rows.map(\.id).joined(separator: ","))"
            )
        )

        await fixture.setOpenOverride(.deliveredOnly)
        if let deliveredSelection = cindySessions.selection(for: alpha2) {
            await queuedOpen(deliveredSelection)
        }
        let deliveredOnlyPass =
            cindySessions.lastResult == .delivered &&
            lastHostActionReceipt?.outcome == "delivered" &&
            cindySessions.delivered == alpha2 &&
            cindySessions.selected == nil &&
            cindySessions.lastDeepLink == "cindy://session/sess-alpha-2" &&
            navigationHistory.current != .session(adapter: "cindy", session: "sess-alpha-2")

        if let alpha2Selection = cindySessions.selection(for: alpha2) {
            await queuedOpen(alpha2Selection)
        }
        let confirmedAfterDeliveredPass =
            deliveredOnlyPass &&
            cindySessions.lastResult == .confirmed &&
            lastHostActionReceipt?.outcome == "confirmed" &&
            cindySessions.delivered == alpha2 &&
            cindySessions.selected == alpha2 &&
            cindySessions.lastDeepLink == "cindy://session/sess-alpha-2" &&
            navigationHistory.current == .session(adapter: "cindy", session: "sess-alpha-2")
        checks.append(
            CindyScenarioCheckReceipt(
                name: "delivered_vs_confirmed_separated",
                status: confirmedAfterDeliveredPass ? "PASS" : "FAIL",
                detail: "delivered=\(cindySessions.delivered?.id ?? "nil") selected=\(cindySessions.selected?.id ?? "nil") deepLink=\(cindySessions.lastDeepLink ?? "nil")"
            )
        )

        let sameNamePass =
            cindySessions.lastResult == .confirmed &&
            cindySessions.selected == alpha2 &&
            cindySessions.selected != alpha1 &&
            cindySessions.selected != beta1
        checks.append(
            CindyScenarioCheckReceipt(
                name: "same_name_exact_identity",
                status: sameNamePass ? "PASS" : "FAIL",
                detail: "selected=\(cindySessions.selected?.id ?? "nil")"
            )
        )

        if let beta1Selection = cindySessions.selection(for: beta1) {
            await queuedOpen(beta1Selection)
        }
        let readbackPass =
            cindySessions.lastResult == .confirmed &&
            cindySessions.selected == beta1 &&
            navigationHistory.current == .session(adapter: "cindy", session: "sess-beta-1")
        checks.append(
            CindyScenarioCheckReceipt(
                name: "selected_readback_matches",
                status: readbackPass ? "PASS" : "FAIL",
                detail: "selected=\(cindySessions.selected?.id ?? "nil")"
            )
        )

        await fixture.setOpenOverride(.wrongSelected(alpha1))
        if let mismatchSelection = cindySessions.selection(for: beta1) {
            await queuedOpen(mismatchSelection)
        }
        let unconfirmedPass =
            cindySessions.lastResult == .unconfirmed &&
            lastHostActionReceipt?.outcome == "unconfirmed" &&
            cindySessions.delivered == beta1 &&
            cindySessions.selected == nil
        checks.append(
            CindyScenarioCheckReceipt(
                name: "wrong_selected_unconfirmed",
                status: unconfirmedPass ? "PASS" : "FAIL",
                detail: "result=\(cindySessions.lastResult?.rawValue ?? "nil") selected=\(cindySessions.selected?.id ?? "nil")"
            )
        )

        let readsBeforeOutOfScope = await fixture.stats().authorizedReads
        let forgedReply = await fixture.open(
            CindySelection(
                address: secret,
                instanceId: "cindy-instance-1",
                generation: 1,
                ownerEpoch: "owner-account-1:1",
                localEpoch: 1
            )
        )
        let readsAfterOutOfScope = await fixture.stats()
        let outOfScopeZeroReadsPass =
            forgedReply.status == .denied &&
            readsAfterOutOfScope.authorizedReads == readsBeforeOutOfScope &&
            readsAfterOutOfScope.unauthorizedReads == 0
        checks.append(
            CindyScenarioCheckReceipt(
                name: "out_of_scope_zero_reads",
                status: outOfScopeZeroReadsPass ? "PASS" : "FAIL",
                detail: "status=\(forgedReply.status.rawValue) delta_reads=\(readsAfterOutOfScope.authorizedReads - readsBeforeOutOfScope)"
            )
        )

        await cindySessions.refreshNow()
        if let staleDeletedSelection = cindySessions.selection(for: alpha2) {
            await fixture.removeTask(alpha2)
            let historyBeforeDeletedOpen = await fixture.stats().openHistory.count
            await queuedOpen(staleDeletedSelection)
            let historyAfterDeletedOpen = await fixture.stats().openHistory.count
            let deletedNoRedirectPass =
                cindySessions.lastResult == .stale &&
                lastHostActionReceipt?.outcome == "stale" &&
                cindySessions.state == .unavailable(.stale) &&
                cindySessions.selected == nil &&
                cindySessions.rows.isEmpty &&
                historyAfterDeletedOpen == historyBeforeDeletedOpen
            checks.append(
                CindyScenarioCheckReceipt(
                    name: "deleted_target_no_redirect",
                    status: deletedNoRedirectPass ? "PASS" : "FAIL",
                    detail: "result=\(cindySessions.lastResult?.rawValue ?? "nil") extra_opens=\(historyAfterDeletedOpen - historyBeforeDeletedOpen)"
                )
            )
        }

        await cindySessions.refreshNow()
        if let preRotationSelection = cindySessions.selection(for: alpha1) {
            await fixture.setOpenOverride(.rotateOwner("owner-account-2:2", 2))
            await queuedOpen(preRotationSelection)
            let rotationPass =
                cindySessions.lastResult == .stale &&
                lastHostActionReceipt?.outcome == "stale" &&
                cindySessions.state == .unavailable(.stale) &&
                cindySessions.rows.isEmpty &&
                cindySessions.selected == nil &&
                cindySessions.delivered == nil
            checks.append(
                CindyScenarioCheckReceipt(
                    name: "account_or_generation_change_blocks_old_selection",
                    status: rotationPass ? "PASS" : "FAIL",
                    detail: "state=\(cindySessions.state.label) rows=\(cindySessions.rows.count)"
                )
            )
        }

        await cindySessions.refreshNow()
        if let currentAlpha1 = cindySessions.selection(for: alpha1) {
            await queuedOpen(currentAlpha1)
        }

        let finalStats = await fixture.stats()
        let allPass = checks.count == 9 && checks.allSatisfy { $0.status == "PASS" } && finalStats.unauthorizedReads == 0
        let receipt = CindyNavigationScenarioReceipt(
            scenario: "cindy-navigation",
            status: allPass ? "PASS" : "FAIL",
            unauthorized_metadata_reads: finalStats.unauthorizedReads,
            authorized_metadata_reads: finalStats.authorizedReads,
            local_agent_api_relaxed: false,
            checks: checks,
            native_app: "NOT_RUN",
            physical_key: "NOT_RUN"
        )
        writeCindyScenarioReceipt(receipt)
    }

    private func writeCindyScenarioReceipt(_ receipt: CindyNavigationScenarioReceipt) {
        let env = ProcessInfo.processInfo.environment
        guard let auditPath = env["MI_HOST_AUDIT_LOG"] ?? env["MI_AUDIT_LOG"], !auditPath.isEmpty else {
            return
        }
        let directory = (auditPath as NSString).deletingLastPathComponent
        let outputPath = (directory as NSString).appendingPathComponent("cindy-navigation.json")
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(receipt) else { return }
        try? data.write(to: URL(fileURLWithPath: outputPath), options: .atomic)
    }
}
#endif
