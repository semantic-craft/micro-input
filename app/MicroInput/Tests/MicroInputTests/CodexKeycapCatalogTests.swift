import XCTest
@testable import MicroInput

@MainActor
final class CodexKeycapCatalogTests: XCTestCase {
    func testTopSixDefaultRoleMatchesUserSpecificationPerAgent() {
        let prefs = KeycapVisualPreferences()

        for slot in 0..<6 {
            XCTAssertEqual(
                prefs.role(for: "prof-herdr", slot: slot, agent: .herdr, action: nil),
                .actionKey,
                "Herdr top key \(slot) must default to actionKey"
            )
            XCTAssertEqual(
                prefs.role(for: "prof-qwen", slot: slot, agent: .qwenwork, action: nil),
                .actionKey,
                "QwenWork top key \(slot) must default to actionKey"
            )
        }

        for agent in [AgentProfileKind.cindy, .codex, .antigravity, .zcode, .dsh] {
            for slot in 0..<6 {
                XCTAssertEqual(
                    prefs.role(for: "prof-\(agent.rawValue)", slot: slot, agent: agent, action: nil),
                    .taskKey,
                    "\(agent.rawValue) top key \(slot) must default to taskKey"
                )
            }
            for slot in 6..<13 {
                XCTAssertEqual(
                    prefs.role(for: "prof-\(agent.rawValue)", slot: slot, agent: agent, action: nil),
                    .actionKey,
                    "\(agent.rawValue) bottom/middle key \(slot) must default to actionKey"
                )
            }
        }
    }

    func testBoundNonSlotActionOnTopSixAlwaysResolvesToActionKey() {
        let prefs = KeycapVisualPreferences()
        let searchFilesAction = CodexKeycapCatalog.makeShortcutAction(
            id: "search-files",
            name: "搜索文件",
            detail: "Cmd+P",
            key: "p",
            modifiers: ["command"],
            target: "frontmost-editor"
        )

        for agent in [AgentProfileKind.antigravity, .cindy, .zcode, .dsh] {
            let profileId = "prof-\(agent.rawValue)"
            for slot in 0..<6 {
                prefs.setRole(.taskKey, for: profileId, slot: slot)

                let resolved = KeycapPresentationResolver.resolveRole(
                    slot: slot,
                    agent: agent,
                    boundAction: searchFilesAction
                )
                XCTAssertEqual(resolved, .actionKey, "Non-slot action on slot \(slot) for \(agent.rawValue) must resolve to actionKey")

                let prefRole = prefs.role(
                    for: profileId,
                    slot: slot,
                    agent: agent,
                    action: searchFilesAction
                )
                XCTAssertEqual(prefRole, .actionKey, "KeycapVisualPreferences.role must return actionKey when boundAction is non-slot")

                let caption = KeycapPresentationResolver.resolveCaption(
                    slot: slot,
                    role: prefRole,
                    actionDisplayName: searchFilesAction.name,
                    liveSessionTitle: "Some Live Session"
                )
                XCTAssertEqual(caption, "搜索文件")
                XCTAssertFalse(caption.contains("·"))
            }
        }
    }

    func testAllResolvedCaptionsContainZeroMiddleDotsAndNoLeadingNumbers() {
        let allAgents: [AgentProfileKind] = [.antigravity, .cindy, .codex, .zcode, .dsh, .qwenwork, .herdr, .custom]
        for agent in allAgents {
            for slot in 0..<20 {
                for cap in CodexKeycapId.allCases {
                    let act = CodexKeycapCatalog.defaultAction(for: cap, agent: agent, slot: slot)
                    let role = KeycapPresentationResolver.resolveRole(slot: slot, agent: agent, boundAction: act)
                    let caption = KeycapPresentationResolver.resolveCaption(slot: slot, role: role, actionDisplayName: act.name)
                    XCTAssertFalse(caption.contains("·"), "Caption '\(caption)' for \(cap.rawValue) on \(agent.rawValue) slot \(slot) must not contain middle dot")
                }

                for group in CodexKeycapCatalog.actionGroups(for: agent, slot: slot) {
                    for item in group.items {
                        let role = KeycapPresentationResolver.resolveRole(slot: slot, agent: agent, boundAction: item.action)
                        let caption = KeycapPresentationResolver.resolveCaption(slot: slot, role: role, actionDisplayName: item.action.name)
                        XCTAssertFalse(caption.contains("·"), "Caption '\(caption)' for preset \(item.id) must not contain middle dot")
                    }
                }
            }
        }

        for slot in 0..<6 {
            let taskCaption = KeycapPresentationResolver.resolveCaption(slot: slot, role: .taskKey, actionDisplayName: "未分配", liveSessionTitle: nil)
            XCTAssertEqual(taskCaption, "会话 \(slot + 1)")
            XCTAssertFalse(taskCaption.contains("·"))

            let liveCaption = KeycapPresentationResolver.resolveCaption(slot: slot, role: .taskKey, actionDisplayName: "未分配", liveSessionTitle: "1·重构界面方案")
            XCTAssertEqual(liveCaption, "重构界面")
            XCTAssertFalse(liveCaption.contains("·"))
        }
    }

    func testControlSlotClassification() {
        for s in 0..<6 {
            let cs = ControlSlot(rawSlot: s)
            XCTAssertEqual(cs, .topRowKey(s))
            XCTAssertEqual(cs.rawSlot, s)
            XCTAssertTrue(cs.isKeycap)
            XCTAssertTrue(cs.supportsTaskRole)
        }
        for s in [6, 7, 8, 9, 12] {
            let cs = ControlSlot(rawSlot: s)
            XCTAssertEqual(cs, .actionRowKey(s))
            XCTAssertEqual(cs.rawSlot, s)
            XCTAssertTrue(cs.isKeycap)
            XCTAssertFalse(cs.supportsTaskRole)
        }
        for s in [10, 11] {
            let cs = ControlSlot(rawSlot: s)
            XCTAssertEqual(cs, .bottomWideKey(s))
            XCTAssertEqual(cs.rawSlot, s)
            XCTAssertTrue(cs.isKeycap)
            XCTAssertFalse(cs.supportsTaskRole)
        }
        for s in 13..<16 {
            let cs = ControlSlot(rawSlot: s)
            XCTAssertEqual(cs, .rotaryEncoder(s))
            XCTAssertEqual(cs.rawSlot, s)
            XCTAssertFalse(cs.isKeycap)
            XCTAssertFalse(cs.supportsTaskRole)
        }
        for s in 16..<20 {
            let cs = ControlSlot(rawSlot: s)
            XCTAssertEqual(cs, .joystickSector(s))
            XCTAssertEqual(cs.rawSlot, s)
            XCTAssertFalse(cs.isKeycap)
            XCTAssertFalse(cs.supportsTaskRole)
        }
    }

    func testAgentActionGroupsExposeDistinctCapabilitiesPerAgent() {
        let agItems = CodexKeycapCatalog.actionGroups(for: .antigravity, slot: 6).flatMap(\.items)
        XCTAssertTrue(agItems.contains(where: { $0.action.payload["command"]?.text == "live-voice" }), "Antigravity must expose Gemini Live")
        XCTAssertTrue(agItems.contains(where: { $0.action.payload["command"]?.text == "project" }), "Antigravity must expose Select Project")
        XCTAssertFalse(agItems.contains(where: { $0.action.payload["command"]?.text == "toggle-voice-reply" }), "Antigravity 2.29 must not expose unsupported TTS voice reply")

        let zcodeItems = CodexKeycapCatalog.actionGroups(for: .zcode, slot: 6).flatMap(\.items)
        XCTAssertTrue(zcodeItems.contains(where: { $0.action.payload["command"]?.text == "toggle-voice-reply" }), "ZCode must expose voice reply toggle")

        let dshItems = CodexKeycapCatalog.actionGroups(for: .dsh, slot: 6).flatMap(\.items)
        XCTAssertTrue(dshItems.contains(where: { $0.action.payload["command"]?.text == "toggle-voice-reply" }), "DSH must expose voice reply toggle")

        let qwenItems = CodexKeycapCatalog.actionGroups(for: .qwenwork, slot: 0).flatMap(\.items)
        XCTAssertTrue(qwenItems.contains(where: { $0.action.name.contains("搜索全部任务") }), "QwenWork must expose global task search")
        XCTAssertTrue(qwenItems.contains(where: { $0.action.name.contains("新建任务") }), "QwenWork must expose new task")
        XCTAssertFalse(qwenItems.contains(where: { $0.action.payload["command"]?.text == "effort" }), "QwenWork combines tier into model selector and must not expose standalone effort")

        let herdrItems = CodexKeycapCatalog.actionGroups(for: .herdr, slot: 1).flatMap(\.items)
        XCTAssertTrue(herdrItems.contains(where: { $0.action.name.contains("选择 Space / Tab") }), "Herdr must expose Space/Tab picker")
        XCTAssertTrue(herdrItems.contains(where: { $0.action.name.contains("向右分屏") }), "Herdr must expose split right")
        XCTAssertFalse(herdrItems.contains(where: { $0.action.payload["command"]?.text == "push-to-talk" }), "Herdr is a terminal multiplexer and must not expose voice dictation")
    }

    func testKeycapCatalogSearchAndDefaultActionResolution() {
        XCTAssertEqual(CodexKeycapId.allCases.count, 34)
        for cap in CodexKeycapId.allCases {
            XCTAssertFalse(cap.shortLabel.contains("·"))
            XCTAssertNil(cap.shortLabel.range(of: #"^[A-Z0-9+-]+$"#, options: .regularExpression), "shortLabel should be natural Chinese, not all-caps ASCII")
        }
        XCTAssertTrue(CodexKeycapId.approve.matchesSearch("APPR"))
        XCTAssertTrue(CodexKeycapId.approve.matchesSearch("批准"))
        XCTAssertTrue(CodexKeycapId.mindPlus.matchesSearch("MIND+"))

        let agApprove = CodexKeycapCatalog.defaultAction(for: .approve, agent: .antigravity, slot: 7)
        XCTAssertEqual(agApprove.payload["command"]?.text, "approve")

        let herdrSplit = CodexKeycapCatalog.defaultAction(for: .split, agent: .herdr, slot: 5)
        XCTAssertEqual(herdrSplit.name, "向右分屏")
    }
}
