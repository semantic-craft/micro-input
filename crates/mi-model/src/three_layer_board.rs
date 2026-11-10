use super::{array, digest, field, object};
use crate::{parse_device_files, DeviceConfig};
use serde_json::{json, value::RawValue, Value};
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const THREE_LAYER_PROFILE_NAME: &str = "Micro Input USB";
pub const THREE_LAYER_NAMES: [&str; 3] = ["Antigravity", "Codex", "DeepSeek Harness"];
pub const THREE_LAYER_MATRIX_BINDINGS: [[&str; 13]; 3] = [
    [
        "AG00", "AG01", "AG02", "AG03", "AG04", "AG05", "AG06", "AG07", "AG08", "AG09", "AG10",
        "AG11", "AG12",
    ],
    [
        "ACT00", "ACT01", "ACT02", "ACT03", "ACT04", "ACT05", "ACT06", "ACT07", "ACT08", "ACT09",
        "ACT10", "ACT11", "ACT12",
    ],
    [
        "AG13", "AG14", "AG15", "AG16", "AG17", "AG18", "AG19", "ACT13", "ACT14", "ACT15", "ACT16",
        "ACT17", "ACT18",
    ],
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreeLayerBoard {
    pub profile_index: u32,
    /// The firmware's layer index is one-based, independently of its JSON IDs.
    pub layer_index: u32,
}

pub enum ThreeLayerProvision {
    AlreadySelected { profile_index: u32 },
    Write { profile_index: u32, keymap: Vec<u8> },
}

fn dedicated_profile(id: u64) -> Value {
    let layers: Vec<_> = THREE_LAYER_NAMES
        .iter()
        .zip(THREE_LAYER_MATRIX_BINDINGS)
        .zip([0x00ffa6_u32, 0x726ce9, 0x4c74ff])
        .enumerate()
        .map(|(idx, ((name, matrix), color))| {
            let tokens: Vec<_> = matrix
                .into_iter()
                .map(|key| format!("KV_OAI_{key}"))
                .collect();
            let rows = [
                &tokens[0..2],
                &tokens[2..6],
                &tokens[6..10],
                &tokens[10..13],
            ];
            json!({
                "id": idx,
                "name": name,
                "color": color,
                "layout": {
                    "base": rows,
                    "keymap": rows,
                    "encoders": [["KV_OAI_ENC_CC","KV_OAI_ENC_CW","KV_OAI_ENC_CLK"]],
                    "joystick": {"type":"VENDOR","sectors":[]}
                }
            })
        })
        .collect();
    json!({"id":id,"name":THREE_LAYER_PROFILE_NAME,"layers":layers})
}

/// Recognize only the exact layer mapping; a user profile with this name is never overwritten.
pub fn recognize_three_layer_profile(config: &DeviceConfig) -> Result<Option<u32>, String> {
    let mut found = None;
    let profiles = config.raw["profiles"]
        .as_array()
        .ok_or("设备缺少 Profile 列表")?;
    for (index, profile) in profiles.iter().enumerate() {
        if profile["name"].as_str() != Some(THREE_LAYER_PROFILE_NAME) {
            continue;
        }
        let id = profile["id"].as_u64().ok_or("板载 Profile ID 不可识别")?;
        if found.is_some()
            || id != index as u64
            || profile != &dedicated_profile(id)
            || profiles
                .iter()
                .filter(|p| p["id"].as_u64() == Some(id))
                .count()
                != 1
        {
            return Err("板载 Profile 同名、重复或已修改；保留现有映射".into());
        }
        found = Some(u32::try_from(index).map_err(|_| "板载 Profile 索引超界")?);
    }
    Ok(found)
}

pub fn compile_three_layer_profile(
    keymap: &[u8],
    smart_actions: Option<&[u8]>,
) -> Result<ThreeLayerProvision, String> {
    let config = parse_device_files(keymap, smart_actions).map_err(|e| e.to_string())?;
    let active = config.raw["activeProfileId"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .filter(|&n| n < config.profiles.len())
        .ok_or("原板载活动 Profile 索引不可识别")?;
    let known = recognize_three_layer_profile(&config)?;
    if known.map(|i| i as usize) == Some(active) {
        return Ok(ThreeLayerProvision::AlreadySelected {
            profile_index: active as u32,
        });
    }

    let root: &RawValue = serde_json::from_slice(keymap).map_err(|e| e.to_string())?;
    let fields = object(root)?;
    let mut edits = Vec::new();
    let index = if let Some(index) = known {
        index
    } else {
        let profiles_raw = field(&fields, "profiles")?;
        let entries = array(profiles_raw)?;
        let index = u32::try_from(entries.len()).map_err(|_| "板载 Profile 索引超界")?;
        let id = u64::from(index);
        if config.profiles.iter().any(|p| p.id.as_u64() == Some(id)) {
            return Err("新增板载 Profile 的 ID 与原 Profile 重复；没有改动原映射".into());
        }
        let end = offset(keymap, profiles_raw)? + profiles_raw.get().len() - 1;
        let mut bytes = vec![b','];
        bytes.extend(serde_json::to_vec(&dedicated_profile(id)).map_err(|e| e.to_string())?);
        edits.push((end, end, bytes));
        index
    };

    let active_raw = field(&fields, "activeProfileId")?;
    let start = offset(keymap, active_raw)?;
    edits.push((
        start,
        start + active_raw.get().len(),
        index.to_string().into_bytes(),
    ));
    edits.sort_by_key(|(start, _, _)| *start);
    let mut cursor = 0;
    let mut next = Vec::with_capacity(keymap.len() + 2200);
    for (start, end, replacement) in edits {
        next.extend_from_slice(&keymap[cursor..start]);
        next.extend(replacement);
        cursor = end;
    }
    next.extend_from_slice(&keymap[cursor..]);

    let parsed = parse_device_files(&next, smart_actions).map_err(|e| e.to_string())?;
    if recognize_three_layer_profile(&parsed)? != Some(index) {
        return Err("板载 Profile 候选识别失败".into());
    }
    Ok(ThreeLayerProvision::Write {
        profile_index: index,
        keymap: next,
    })
}

fn offset(input: &[u8], raw: &RawValue) -> Result<usize, String> {
    let bytes = raw.get().as_bytes();
    let start = (bytes.as_ptr() as usize)
        .checked_sub(input.as_ptr() as usize)
        .ok_or("原字节范围无效")?;
    if input.get(start..start.saturating_add(bytes.len())) != Some(bytes) {
        return Err("原字节范围不一致".into());
    }
    Ok(start)
}

pub fn back_up_three_layer_write(
    parent: &Path,
    before: ThreeLayerBoard,
    profile_index: u32,
    original: &[u8],
    candidate: &[u8],
    smart_actions: Option<&[u8]>,
) -> Result<PathBuf, String> {
    let root = parent.join("three-layer-device-backups");
    match DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    let meta = fs::symlink_metadata(&root).map_err(|e| e.to_string())?;
    if !meta.is_dir()
        || meta.permissions().mode() & 0o777 != 0o700
        || meta.uid() != fs::metadata(parent).map_err(|e| e.to_string())?.uid()
    {
        return Err("板载备份目录不是当前文档所属的私有目录".into());
    }
    let path = root.join(crate::document::new_id());
    DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .map_err(|e| e.to_string())?;
    let write = |name: &str, bytes: &[u8]| -> Result<(), String> {
        let file = path.join(name);
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)
            .map_err(|e| e.to_string())?;
        f.write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        if fs::read(file).map_err(|e| e.to_string())? != bytes {
            return Err("板载备份读回不符，未发送写入".into());
        }
        Ok(())
    };
    write("keymap.json", original)?;
    if let Some(smart_actions) = smart_actions {
        write("smart_actions.json", smart_actions)?;
    }
    write("planned-keymap.json", candidate)?;
    let metadata = json!({
        "format": "micro-input-three-layer-recovery-v1",
        "previous_profile_index": before.profile_index,
        "previous_layer_index": before.layer_index,
        "planned_profile_index": profile_index,
        "keymap_sha256": digest(original),
        "planned_keymap_sha256": digest(candidate),
        "smart_actions_sha256": smart_actions.map(digest),
    });
    write(
        "manifest.json",
        &serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
    )?;
    for dir in [&path, &root, parent] {
        File::open(dir)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(path)
}
