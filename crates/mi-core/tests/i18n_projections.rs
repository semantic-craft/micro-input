use mi_core::{
    ActiveHudContent, CanvasAction, DeviceWorkflowAction, HeaderAction, HostOverlayAction,
    InspectorAction, InspectorBodyProjection, JoystickControlMode, KnobJoystickPreset,
    ProfileCommand, ResolvedLocale, SidebarAction, UiLanguagePreference, WorkbenchCommand,
    WorkbenchStore, WorkbenchStoreConfig,
};
use mi_model::draft::Action;
use serde_json::json;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn contains_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c as u32,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x3000..=0x303F | 0xFF00..=0xFFEF
        )
    })
}

fn open_persisted_isolated_store(dir: &Path) -> WorkbenchStore {
    let wb_path = dir.join("workbench.json");
    if !wb_path.exists() {
        let seed = WorkbenchStore::open_isolated(dir).unwrap();
        let bytes = serde_json::to_vec(&seed.document_store.draft.document).unwrap();
        fs::write(&wb_path, bytes).unwrap();
    }
    WorkbenchStore::open_isolated(dir).unwrap()
}

#[test]
fn live_language_switching_system_locale_and_host_preferences_roundtrip() {
    let temp = TempDir::new().unwrap();
    let mut store = open_persisted_isolated_store(temp.path());

    assert_eq!(
        store.projection().header.ui_language,
        UiLanguagePreference::System
    );
    assert_eq!(
        store.projection().header.resolved_locale,
        ResolvedLocale::ZhHans
    );
    assert!(store.projection().header.silent_at_login);
    assert_eq!(store.projection().status_message, "已就绪");
    assert!(store.projection().last_exported_diagnostics_path.is_none());

    let gen_before_silent = store.projection_generation();
    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetSilentAtLogin {
            silent: false,
        }))
        .unwrap();
    assert!(store.projection_generation() > gen_before_silent);
    assert!(!store.projection().header.silent_at_login);

    let gen_before_sys_en = store.projection_generation();
    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SyncSystemLocale {
            preferred_languages: vec!["en-US".to_string(), "zh-Hans-CN".to_string()],
        }))
        .unwrap();
    assert!(store.projection_generation() > gen_before_sys_en);
    assert_eq!(
        store.projection().header.ui_language,
        UiLanguagePreference::System
    );
    assert_eq!(
        store.projection().header.resolved_locale,
        ResolvedLocale::En
    );
    assert_eq!(store.projection().status_message, "Ready");
    assert_eq!(
        store.projection().sidebar.section_title,
        "Software Profiles"
    );

    let other_row = store
        .projection()
        .sidebar
        .rows
        .iter()
        .find(|r| r.badge_monogram == "APP")
        .expect("Other Apps row must project APP monogram in English");
    assert_eq!(other_row.display_name, "Other Apps");

    let layer_tab = store
        .projection()
        .canvas
        .layer_tabs
        .first()
        .expect("starter base layer tab");
    assert_eq!(layer_tab.name, "Base Layer");
    assert_eq!(
        mi_model::i18n::localize_layer_name("默认层", ResolvedLocale::En),
        "Default Layer"
    );
    assert_eq!(store.projection().canvas.joystick.mode, "logical_sectors");
    for sector in &store.projection().canvas.joystick.sectors {
        assert!(
            !contains_cjk(&sector.concise_label) && !sector.concise_label.contains('…'),
            "English Antigravity joystick sector {} must fit without CJK/ellipsis: {}",
            sector.logical_slot,
            sector.concise_label
        );
    }
    let codex_row = store
        .projection()
        .sidebar
        .rows
        .iter()
        .find(|r| r.display_name == "Codex")
        .expect("Codex row");
    let codex_id = codex_row.profile_id.clone();
    store.select_and_activate_profile(&codex_id).unwrap();
    assert_eq!(store.projection().canvas.joystick.mode, "mouse");
    let ag_id = store
        .projection()
        .sidebar
        .rows
        .iter()
        .find(|r| r.display_name == "Antigravity")
        .expect("Antigravity row")
        .profile_id
        .clone();
    store.select_and_activate_profile(&ag_id).unwrap();

    match &store.projection().inspector.body {
        InspectorBodyProjection::Editable(body) => {
            assert_eq!(body.action_category_filter, "All");
            assert_eq!(
                body.available_categories.first().map(String::as_str),
                Some("All")
            );
            for cat in &body.available_categories {
                assert!(
                    !contains_cjk(cat),
                    "English Inspector category must not contain CJK: {cat}"
                );
            }
            assert!(!body.filtered_actions.is_empty());
            for row in &body.filtered_actions {
                assert!(
                    !contains_cjk(&row.name),
                    "English action row name must not contain CJK: {} ({})",
                    row.name,
                    row.action_id
                );
                assert!(
                    !contains_cjk(&row.category),
                    "English action row category must not contain CJK: {}",
                    row.category
                );
                assert!(
                    !contains_cjk(&row.detail),
                    "English action row detail must not contain CJK: {} ({})",
                    row.detail,
                    row.action_id
                );
            }
            for kc in &body.filtered_keycaps {
                assert!(
                    !contains_cjk(&kc.chinese_hint),
                    "English keycap hint must not contain CJK: {}",
                    kc.chinese_hint
                );
            }
        }
        InspectorBodyProjection::ProtectedLocked(_) => {
            panic!("starter layer must be editable");
        }
    }

    store
        .dispatch_command(WorkbenchCommand::Device(
            DeviceWorkflowAction::ExportDiagnostics,
        ))
        .unwrap();
    let exported_diag = store
        .projection()
        .last_exported_diagnostics_path
        .clone()
        .expect("ExportDiagnostics must populate last_exported_diagnostics_path");
    assert!(Path::new(&exported_diag).exists());
    assert!(
        store
            .projection()
            .status_message
            .starts_with("Exported diagnostic report: "),
        "unexpected English diagnostics status: {}",
        store.projection().status_message
    );

    let gen_before_zh = store.projection_generation();
    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::ZhHans,
        }))
        .unwrap();
    assert!(store.projection_generation() > gen_before_zh);
    assert_eq!(
        store.projection().header.ui_language,
        UiLanguagePreference::ZhHans
    );
    assert_eq!(
        store.projection().header.resolved_locale,
        ResolvedLocale::ZhHans
    );
    assert!(
        store
            .projection()
            .status_message
            .starts_with("已导出诊断报告："),
        "switching back to ZhHans must re-localize diagnostics status: {}",
        store.projection().status_message
    );

    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::En,
        }))
        .unwrap();
    assert_eq!(
        store.projection().header.resolved_locale,
        ResolvedLocale::En
    );

    drop(store);
    let reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert_eq!(
        reopened.projection().header.ui_language,
        UiLanguagePreference::En
    );
    assert_eq!(
        reopened.projection().header.resolved_locale,
        ResolvedLocale::En
    );
    assert!(!reopened.projection().header.silent_at_login);
    assert_eq!(reopened.projection().status_message, "Ready");
    match &reopened.projection().inspector.body {
        InspectorBodyProjection::Editable(body) => {
            assert_eq!(body.action_category_filter, "All");
        }
        InspectorBodyProjection::ProtectedLocked(_) => panic!("expected editable inspector"),
    }
}

#[test]
fn canonical_disk_names_and_custom_user_names_remain_untouched_while_category_filter_normalizes() {
    let temp = TempDir::new().unwrap();
    let mut store = open_persisted_isolated_store(temp.path());

    store
        .dispatch_command(WorkbenchCommand::Inspector(
            InspectorAction::SetActionCategoryFilter("导航".to_string()),
        ))
        .unwrap();

    store
        .apply_profile_command(ProfileCommand::Create {
            name: "我的专属工作流".to_string(),
        })
        .unwrap();
    let custom_pid = store.selected_profile_id().unwrap().to_string();
    let base_layer_id = store.selected_layer_id().unwrap().to_string();

    store
        .dispatch_command(WorkbenchCommand::Inspector(
            InspectorAction::CreateCustomLayer {
                name: "默认层".to_string(),
            },
        ))
        .unwrap();
    let default_layer_id = store.selected_layer_id().unwrap().to_string();

    store
        .dispatch_command(WorkbenchCommand::Inspector(
            InspectorAction::CreateCustomLayer {
                name: "冲刺层 Alpha".to_string(),
            },
        ))
        .unwrap();
    let custom_lid = store.selected_layer_id().unwrap().to_string();

    store
        .document_store
        .update(|draft| {
            draft.assign(
                &json!(custom_pid),
                &json!(custom_lid),
                0,
                Action {
                    id: "custom-user-prompt-1".to_string(),
                    name: "自定义提示词".to_string(),
                    category: "文本".to_string(),
                    detail: "Run targeted cargo test".to_string(),
                    payload: json!({
                        "type": "text",
                        "text": "Run targeted cargo test"
                    }),
                },
            )
        })
        .unwrap();

    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::En,
        }))
        .unwrap();

    let custom_row_en = store
        .projection()
        .sidebar
        .rows
        .iter()
        .find(|r| r.profile_id == custom_pid)
        .expect("custom profile visible");
    assert_eq!(custom_row_en.display_name, "我的专属工作流");

    let base_layer_en = store
        .projection()
        .canvas
        .layer_tabs
        .iter()
        .find(|l| l.layer_id == base_layer_id)
        .expect("base layer on custom profile");
    assert_eq!(base_layer_en.name, "Base Layer");

    let default_layer_en = store
        .projection()
        .canvas
        .layer_tabs
        .iter()
        .find(|l| l.layer_id == default_layer_id)
        .expect("default layer on custom profile");
    assert_eq!(default_layer_en.name, "Default Layer");

    let custom_layer_en = store
        .projection()
        .canvas
        .layer_tabs
        .iter()
        .find(|l| l.is_selected)
        .expect("selected custom layer");
    assert_eq!(custom_layer_en.name, "冲刺层 Alpha");

    match &store.projection().inspector.body {
        InspectorBodyProjection::Editable(body) => {
            assert_eq!(body.action_category_filter, "Navigation");
            assert_eq!(body.bound_action_name, "自定义提示词");
            assert_eq!(body.bound_action_category, "Text");
            assert!(!body.filtered_actions.is_empty());
            for row in &body.filtered_actions {
                assert_eq!(row.category, "Navigation");
                assert!(!contains_cjk(&row.name));
            }
        }
        InspectorBodyProjection::ProtectedLocked(_) => panic!("expected editable inspector"),
    }

    let raw_disk_json = fs::read_to_string(temp.path().join("workbench.json")).unwrap();
    assert!(
        raw_disk_json.contains("\"其他软件\""),
        "workbench.json must retain canonical 其他软件 on disk"
    );
    assert!(
        raw_disk_json.contains("\"基础层\""),
        "workbench.json must retain canonical 基础层 on disk"
    );
    assert!(
        raw_disk_json.contains("\"默认层\""),
        "workbench.json must retain canonical 默认层 on disk"
    );
    assert!(
        !raw_disk_json.contains("\"Other Apps\"")
            && !raw_disk_json.contains("\"Base Layer\"")
            && !raw_disk_json.contains("\"Default Layer\""),
        "localized display labels must never be written into workbench.json"
    );

    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::ZhHans,
        }))
        .unwrap();

    match &store.projection().inspector.body {
        InspectorBodyProjection::Editable(body) => {
            assert_eq!(body.action_category_filter, "导航");
            assert!(!body.filtered_actions.is_empty());
            for row in &body.filtered_actions {
                assert_eq!(row.category, "导航");
            }
        }
        InspectorBodyProjection::ProtectedLocked(_) => panic!("expected editable inspector"),
    }
}

#[test]
fn english_cheat_sheet_and_joystick_modes_fit_spatial_budgets_without_cjk_or_ellipsis() {
    let temp = TempDir::new().unwrap();
    let mut cfg = WorkbenchStoreConfig::isolated(temp.path());
    cfg.system_locale = ResolvedLocale::En;
    let mut store = WorkbenchStore::open(cfg).unwrap();

    assert_eq!(
        store.projection().header.resolved_locale,
        ResolvedLocale::En
    );

    store
        .dispatch_command(WorkbenchCommand::Overlay(
            HostOverlayAction::ToggleCheatSheet,
        ))
        .unwrap();

    let profile_ids: Vec<String> = store
        .projection()
        .sidebar
        .rows
        .iter()
        .map(|r| r.profile_id.clone())
        .collect();
    assert_eq!(profile_ids.len(), 5);

    for pid in &profile_ids {
        store
            .dispatch_command(WorkbenchCommand::Sidebar(SidebarAction::SelectProfile(
                pid.clone(),
            )))
            .unwrap();

        let ActiveHudContent::InteractiveCheatSheet(sheet) = &store.projection().overlay.content
        else {
            panic!("expected interactive cheat sheet to remain visible");
        };
        assert_cheat_sheet_english_budgets(sheet);
    }

    let ag_pid = profile_ids[0].clone();
    store
        .dispatch_command(WorkbenchCommand::Sidebar(SidebarAction::SelectProfile(
            ag_pid,
        )))
        .unwrap();

    for preset in [
        KnobJoystickPreset::ModelSelectionAndMouse,
        KnobJoystickPreset::CycleWorkingAndBlueDot,
        KnobJoystickPreset::DualScrollProjectsAndChat,
        KnobJoystickPreset::MousePointerAndScroll,
    ] {
        store
            .dispatch_command(WorkbenchCommand::Canvas(
                CanvasAction::ApplyKnobJoystickPreset(preset),
            ))
            .unwrap();
        let ActiveHudContent::InteractiveCheatSheet(sheet) = &store.projection().overlay.content
        else {
            panic!("expected cheat sheet");
        };
        assert_cheat_sheet_english_budgets(sheet);
    }

    for (mode, expected_wire) in [
        (JoystickControlMode::Mouse, "mouse"),
        (JoystickControlMode::Scroll, "scroll"),
        (JoystickControlMode::LogicalSectors, "logical_sectors"),
    ] {
        store
            .dispatch_command(WorkbenchCommand::Inspector(
                InspectorAction::SetJoystickMode(mode),
            ))
            .unwrap();
        assert_eq!(store.projection().canvas.joystick.mode, expected_wire);
        let ActiveHudContent::InteractiveCheatSheet(sheet) = &store.projection().overlay.content
        else {
            panic!("expected cheat sheet");
        };
        assert_eq!(sheet.joystick_mode, expected_wire);
        assert_cheat_sheet_english_budgets(sheet);
    }
}

fn assert_cheat_sheet_english_budgets(sheet: &mi_core::CheatSheetProjection) {
    assert!(
        !contains_cjk(&sheet.profile_name),
        "CJK in profile_name: {}",
        sheet.profile_name
    );
    assert!(
        !contains_cjk(&sheet.routing_summary),
        "CJK in routing_summary: {}",
        sheet.routing_summary
    );
    if let Some(card) = &sheet.target_agent_card {
        assert!(
            !contains_cjk(&card.agent_name),
            "CJK in target_agent_card.agent_name: {}",
            card.agent_name
        );
        assert!(
            !contains_cjk(&card.phase_label),
            "CJK in target_agent_card.phase_label: {}",
            card.phase_label
        );
    }

    for cell in &sheet.key_cells {
        assert!(
            !contains_cjk(&cell.control_name),
            "CJK in control_name for {}: {}",
            cell.binding_id,
            cell.control_name
        );
        assert!(
            !contains_cjk(&cell.label),
            "CJK in label for {}: {}",
            cell.binding_id,
            cell.label
        );
        assert!(
            !contains_cjk(&cell.short_label),
            "CJK in short_label for {}: {}",
            cell.binding_id,
            cell.short_label
        );
        assert!(
            !cell.short_label.contains('…'),
            "ellipsis forbidden in English Cheat Sheet short_label for {}: {}",
            cell.binding_id,
            cell.short_label
        );

        let char_len = cell.short_label.chars().count();
        match cell.binding_id.as_str() {
            "AG13" | "AG14" | "ENC_CLK" => {
                assert!(
                    char_len <= 12,
                    "rotary cell {} exceeds 12 chars ({}): {:?}",
                    cell.binding_id,
                    char_len,
                    cell.short_label
                );
            }
            "AG15" | "AG16" | "AG17" | "AG18" => {
                assert!(
                    char_len <= 12,
                    "quadrant cell {} exceeds 12 chars ({}): {:?}",
                    cell.binding_id,
                    char_len,
                    cell.short_label
                );
                let words: Vec<&str> = cell.short_label.split_whitespace().collect();
                assert!(
                    words.len() <= 2,
                    "quadrant cell {} exceeds 2 words: {:?}",
                    cell.binding_id,
                    cell.short_label
                );
                for word in words {
                    assert!(
                        word.chars().count() <= 6,
                        "quadrant cell {} has word > 6 chars ({:?}) in {:?}",
                        cell.binding_id,
                        word,
                        cell.short_label
                    );
                }
            }
            _ => {
                assert!(
                    char_len <= 14,
                    "1U/2U cell {} exceeds 14 chars ({}): {:?}",
                    cell.binding_id,
                    char_len,
                    cell.short_label
                );
            }
        }
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn extract_tr_string_literals(source: &str) -> Vec<String> {
    let prod_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    let bytes = prod_source.as_bytes();
    let mut results = Vec::new();
    let mut i = 0;

    while i + 4 < bytes.len() {
        if bytes[i..].starts_with(b"i18n::tr(") {
            let mut cursor = i + "i18n::tr(".len();
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'"' {
                cursor += 1;
                let mut lit = String::new();
                while cursor < bytes.len() {
                    let ch = prod_source[cursor..].chars().next().unwrap();
                    cursor += ch.len_utf8();
                    if ch == '\\' && cursor < bytes.len() {
                        let esc = prod_source[cursor..].chars().next().unwrap();
                        cursor += esc.len_utf8();
                        match esc {
                            'n' => lit.push('\n'),
                            't' => lit.push('\t'),
                            '"' => lit.push('"'),
                            '\\' => lit.push('\\'),
                            other => lit.push(other),
                        }
                    } else if ch == '"' {
                        results.push(lit);
                        break;
                    } else {
                        lit.push(ch);
                    }
                }
                i = cursor;
                continue;
            }
        }
        i += 1;
    }
    results
}

#[test]
fn i18n_literal_coverage_across_mi_model_and_mi_core() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    let mut files = Vec::new();
    collect_rs_files(&workspace_root.join("crates/mi-model/src"), &mut files);
    collect_rs_files(&workspace_root.join("crates/mi-core/src"), &mut files);
    files.sort();

    let mut total_literals = 0usize;
    let mut missing = Vec::new();

    for file in &files {
        let content = fs::read_to_string(file).unwrap();
        for lit in extract_tr_string_literals(&content) {
            total_literals += 1;
            match mi_core::i18n::en_lookup(&lit) {
                Some(en) => {
                    assert!(
                        !en.trim().is_empty(),
                        "empty English translation for {:?} in {}",
                        lit,
                        file.display()
                    );
                    assert!(
                        !contains_cjk(en),
                        "English translation contains CJK for {:?} -> {:?} in {}",
                        lit,
                        en,
                        file.display()
                    );
                }
                None => {
                    missing.push(format!("{}: {:?}", file.display(), lit));
                }
            }
        }
    }

    assert!(
        total_literals >= 40,
        "expected scanner to discover i18n::tr literals, found only {total_literals}"
    );
    assert!(
        missing.is_empty(),
        "missing English table entries for {} i18n::tr literals:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

#[test]
fn dynamic_status_message_preserves_canonical_chinese_across_en_roundtrip() {
    let temp = TempDir::new().unwrap();
    let mut store = open_persisted_isolated_store(temp.path());

    let other_pid = store
        .projection()
        .sidebar
        .rows
        .iter()
        .find(|r| r.raw_name == "其他软件")
        .map(|r| r.profile_id.clone())
        .expect("Other Apps profile exists");
    store
        .dispatch_command(WorkbenchCommand::Sidebar(SidebarAction::SelectProfile(
            other_pid.clone(),
        )))
        .unwrap();
    store
        .dispatch_command(WorkbenchCommand::Inspector(InspectorAction::SelectSlot(9)))
        .unwrap();
    store
        .dispatch_command(WorkbenchCommand::Inspector(
            InspectorAction::UnassignSelectedSlot,
        ))
        .unwrap();
    store.set_profile_locked(true, true).unwrap();
    assert_eq!(store.projection().status_message, "已锁定方案「其他软件」");

    store
        .dispatch_command(WorkbenchCommand::Device(
            DeviceWorkflowAction::SimulateKeyPress {
                binding_id: "AG09".to_string(),
            },
        ))
        .unwrap();
    assert_eq!(
        store.projection().status_message,
        "已按下 AG09（已暂停软件按键映射）"
    );
    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::En,
        }))
        .unwrap();
    assert_eq!(
        store.projection().status_message,
        "Pressed AG09 (software key mapping paused)"
    );
    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::ZhHans,
        }))
        .unwrap();
    assert_eq!(
        store.projection().status_message,
        "已按下 AG09（已暂停软件按键映射）"
    );

    store.set_host_execution_enabled(true).unwrap();
    store
        .dispatch_command(WorkbenchCommand::Device(
            DeviceWorkflowAction::SimulateKeyPress {
                binding_id: "AG09".to_string(),
            },
        ))
        .unwrap();
    assert_eq!(
        store.projection().status_message,
        "已按下 AG09（当前槽位未绑定动作）"
    );

    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::En,
        }))
        .unwrap();
    assert_eq!(
        store.projection().status_message,
        "Pressed AG09 (slot unbound)"
    );

    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::ZhHans,
        }))
        .unwrap();
    assert_eq!(
        store.projection().status_message,
        "已按下 AG09（当前槽位未绑定动作）"
    );

    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::En,
        }))
        .unwrap();
    store.set_profile_locked(true, true).unwrap();
    assert_eq!(
        store.projection().status_message,
        "Locked profile \"Other Apps\""
    );
    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::ZhHans,
        }))
        .unwrap();
    assert_eq!(store.projection().status_message, "已锁定方案「其他软件」");
}

#[test]
fn canonical_duplicate_and_macro_multi_modals_comparison_apply_and_v070_errors_localize_cleanly() {
    use mi_core::{
        EditorAction, EditorModalProjection, FirmwareAction, MultiBranchKind, MultiBranchTarget,
        V070HardwareAction,
    };

    let temp = TempDir::new().unwrap();
    let mut store = open_persisted_isolated_store(temp.path());

    assert_eq!(
        mi_model::i18n::localize_profile_name("自定义方案", ResolvedLocale::En),
        "Custom Profile"
    );

    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::En,
        }))
        .unwrap();

    let other_pid = store
        .projection()
        .sidebar
        .rows
        .iter()
        .find(|r| r.raw_name == "其他软件")
        .map(|r| r.profile_id.clone())
        .expect("Other Apps profile exists");

    store
        .apply_profile_command(ProfileCommand::Duplicate {
            profile_id: other_pid,
            name: String::new(),
        })
        .unwrap();
    let dup_pid = store.selected_profile_id().unwrap().to_string();
    assert_eq!(
        store.projection().status_message,
        "Duplicated profile \"Other Apps Copy\""
    );
    let dup_row_en = store
        .projection()
        .sidebar
        .rows
        .iter()
        .find(|r| r.profile_id == dup_pid)
        .unwrap();
    assert_eq!(dup_row_en.raw_name, "其他软件 副本");
    assert_eq!(dup_row_en.display_name, "Other Apps Copy");

    store
        .dispatch_command(WorkbenchCommand::Inspector(
            InspectorAction::CreateCustomLayer {
                name: "默认层".to_string(),
            },
        ))
        .unwrap();
    store
        .dispatch_command(WorkbenchCommand::Inspector(
            InspectorAction::DuplicateSelectedLayer {
                name: String::new(),
            },
        ))
        .unwrap();
    let dup_lid = store.selected_layer_id().unwrap().to_string();
    let dup_layer_en = store
        .projection()
        .canvas
        .layer_tabs
        .iter()
        .find(|l| l.layer_id == dup_lid)
        .unwrap();
    assert_eq!(dup_layer_en.name, "Default Layer Copy");
    assert_eq!(
        store.document_store.draft.document.editable.graph.layers[&dup_lid].name,
        "默认层 副本"
    );

    store
        .dispatch_command(WorkbenchCommand::Inspector(InspectorAction::SelectSlot(8)))
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::OpenMacroEditor { macro_id: None })
        .unwrap();
    if let Some(EditorModalProjection::MacroEditor(m)) = &store.projection().active_modal {
        assert_eq!(m.name, "New Macro");
    } else {
        panic!("expected MacroEditor modal");
    }
    store
        .dispatch_editor_action(EditorAction::MacroSetName("New Macro".to_string()))
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::MacroAddTapKey {
            key: "a".to_string(),
            delay_ms: 20,
        })
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::MacroSave {
            bind_to_selected_slot: true,
        })
        .unwrap();

    let saved_macro_en = &store.projection().inspector.editable().unwrap().macros[0];
    let saved_macro_id = saved_macro_en.id.clone();
    assert_eq!(saved_macro_en.name, "New Macro");
    assert_eq!(
        saved_macro_en.reference_summary,
        "Other Apps Copy / Default Layer Copy · logical-8"
    );
    assert!(
        !contains_cjk(&saved_macro_en.reference_summary),
        "English macro reference_summary must not contain CJK: {}",
        saved_macro_en.reference_summary
    );
    assert_eq!(
        store.document_store.draft.document.editable.graph.actions[&saved_macro_id].name,
        "新建宏"
    );

    store
        .dispatch_editor_action(EditorAction::OpenMacroEditor {
            macro_id: Some(saved_macro_id.clone()),
        })
        .unwrap();
    if let Some(EditorModalProjection::MacroEditor(m)) = &store.projection().active_modal {
        assert_eq!(m.name, "New Macro");
        assert_eq!(
            m.reference_summaries,
            vec!["Other Apps Copy / Default Layer Copy · logical-8".to_string()]
        );
    } else {
        panic!("expected reopened MacroEditor");
    }
    store
        .dispatch_editor_action(EditorAction::CloseModal)
        .unwrap();

    store
        .dispatch_command(WorkbenchCommand::Inspector(InspectorAction::SelectSlot(9)))
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::OpenMultiEditor { multi_id: None })
        .unwrap();
    if let Some(EditorModalProjection::MultiEditor(m)) = &store.projection().active_modal {
        assert_eq!(m.name, "New Multi");
        assert!(!m.available_actions.is_empty());
        for opt in &m.available_actions {
            assert!(
                !contains_cjk(&opt.label),
                "MultiEditor available_actions label must not contain CJK: {} ({})",
                opt.label,
                opt.id
            );
        }
        for opt in &m.available_macros {
            assert!(
                !contains_cjk(&opt.label),
                "MultiEditor available_macros label must not contain CJK: {} ({})",
                opt.label,
                opt.id
            );
        }
    } else {
        panic!("expected MultiEditor modal");
    }
    store
        .dispatch_editor_action(EditorAction::MultiSetName("New Multi".to_string()))
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::MultiSetBranch {
            branch: MultiBranchKind::Tap,
            target: Some(MultiBranchTarget::Action {
                action_id: "save".to_string(),
            }),
        })
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::MultiSetBranch {
            branch: MultiBranchKind::Hold,
            target: Some(MultiBranchTarget::Macro {
                action_id: saved_macro_id.clone(),
            }),
        })
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::MultiSave {
            bind_to_selected_slot: true,
        })
        .unwrap();

    let saved_multi_en = &store.projection().inspector.editable().unwrap().multis[0];
    let saved_multi_id = saved_multi_en.id.clone();
    assert_eq!(saved_multi_en.name, "New Multi");
    assert_eq!(
        saved_multi_en.reference_summary,
        "Other Apps Copy / Default Layer Copy · logical-9"
    );
    assert_eq!(
        store.document_store.draft.document.editable.graph.actions[&saved_multi_id].name,
        "新建 Multi"
    );

    store
        .dispatch_editor_action(EditorAction::OpenMultiEditor {
            multi_id: Some(saved_multi_id.clone()),
        })
        .unwrap();
    if let Some(EditorModalProjection::MultiEditor(m)) = &store.projection().active_modal {
        assert_eq!(m.name, "New Multi");
        for b in &m.branches {
            assert!(
                !contains_cjk(&b.target_display_name),
                "MultiEditor branch target_display_name must not contain CJK: {}",
                b.target_display_name
            );
        }
        assert_eq!(
            m.reference_summaries,
            vec!["Other Apps Copy / Default Layer Copy · logical-9".to_string()]
        );
    } else {
        panic!("expected reopened MultiEditor");
    }
    store
        .dispatch_editor_action(EditorAction::CloseModal)
        .unwrap();

    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::ZhHans,
        }))
        .unwrap();
    let dup_row_zh = store
        .projection()
        .sidebar
        .rows
        .iter()
        .find(|r| r.profile_id == dup_pid)
        .unwrap();
    assert_eq!(dup_row_zh.display_name, "其他软件 副本");
    let dup_layer_zh = store
        .projection()
        .canvas
        .layer_tabs
        .iter()
        .find(|l| l.layer_id == dup_lid)
        .unwrap();
    assert_eq!(dup_layer_zh.name, "默认层 副本");
    assert_eq!(
        store.projection().inspector.editable().unwrap().macros[0].name,
        "新建宏"
    );
    assert_eq!(
        store.projection().inspector.editable().unwrap().multis[0].name,
        "新建 Multi"
    );

    store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::En,
        }))
        .unwrap();
    let _ = store.dispatch_command(WorkbenchCommand::V070Hardware(V070HardwareAction::Refresh));
    let v070_err = store
        .projection()
        .v070_hardware
        .last_error
        .clone()
        .expect("v070 RefreshAll while disconnected sets last_error");
    assert!(
        !contains_cjk(&v070_err),
        "English v070_hardware.last_error must not contain CJK: {v070_err}"
    );
    assert!(
        !contains_cjk(&store.projection().v070_hardware.status_message),
        "English v070_hardware.status_message must not contain CJK: {}",
        store.projection().v070_hardware.status_message
    );
    let _ = store.dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::PrepareDevice));
    assert!(
        !contains_cjk(&store.projection().firmware.message),
        "English firmware.message must not contain CJK: {}",
        store.projection().firmware.message
    );

    assert_eq!(
        mi_core::i18n::localize_v070_status_message(
            "当前固件 0.6.3 低于 0.7.0-rc.16，不支持硬件能力接口",
            ResolvedLocale::En
        ),
        "Firmware 0.6.3 is older than 0.7.0-rc.16; hardware capability API unsupported"
    );
    assert_eq!(
        mi_core::i18n::localize_v070_status_message(
            "板载层索引 9 超出有效范围（仅支持第 1–6 层）",
            ResolvedLocale::En
        ),
        "Onboard layer index 9 out of range (only Layers 1–6 supported)"
    );
    assert_eq!(
        mi_core::i18n::localize_firmware_message("请先下载并校验固件", ResolvedLocale::En),
        "Download and verify firmware first"
    );

    let comp_temp = TempDir::new().unwrap();
    let mut comp_store = WorkbenchStore::open_isolated(comp_temp.path()).unwrap();
    comp_store
        .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::En,
        }))
        .unwrap();
    comp_store
        .dispatch_command(WorkbenchCommand::Device(DeviceWorkflowAction::ReadSnapshot))
        .unwrap();
    comp_store
        .dispatch_command(WorkbenchCommand::Device(
            DeviceWorkflowAction::OpenApplyPreparation,
        ))
        .unwrap();
    if let Some(EditorModalProjection::ApplyPreparation(apply_modal)) =
        &comp_store.projection().active_modal
    {
        assert!(
            !contains_cjk(&apply_modal.unavailable_reason),
            "English ApplyPreparation unavailable_reason must not contain CJK: {}",
            apply_modal.unavailable_reason
        );
        assert!(
            !contains_cjk(&apply_modal.before_summary),
            "English ApplyPreparation before_summary must not contain CJK: {}",
            apply_modal.before_summary
        );
        assert!(
            !contains_cjk(&apply_modal.after_summary),
            "English ApplyPreparation after_summary must not contain CJK: {}",
            apply_modal.after_summary
        );
    } else {
        panic!("expected ApplyPreparation modal");
    }
    comp_store
        .dispatch_editor_action(EditorAction::ApplyCancelPreview)
        .unwrap();

    comp_store
        .dispatch_command(WorkbenchCommand::Device(
            DeviceWorkflowAction::OpenReconnectComparison,
        ))
        .unwrap();
    if let Some(EditorModalProjection::ReconnectComparison(cmp_modal)) =
        &comp_store.projection().active_modal
    {
        assert!(!cmp_modal.rows.is_empty());
        for row in &cmp_modal.rows {
            assert!(
                !contains_cjk(&row.result_badge),
                "English ReconnectComparison result_badge must not contain CJK: {}",
                row.result_badge
            );
            assert!(
                !contains_cjk(&row.label),
                "English ReconnectComparison row.label must not contain CJK: {}",
                row.label
            );
            assert!(
                !contains_cjk(&row.base_summary),
                "English ReconnectComparison base_summary must not contain CJK: {}",
                row.base_summary
            );
            assert!(
                !contains_cjk(&row.local_summary),
                "English ReconnectComparison local_summary must not contain CJK: {}",
                row.local_summary
            );
            assert!(
                !contains_cjk(&row.device_summary),
                "English ReconnectComparison device_summary must not contain CJK: {}",
                row.device_summary
            );
        }
        for c in &cmp_modal.conflicts {
            assert!(
                !contains_cjk(c),
                "English ReconnectComparison conflict must not contain CJK: {c}"
            );
        }
    } else {
        panic!("expected ReconnectComparison modal");
    }
}
