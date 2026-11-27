use mi_model::{
    direct_controls::{CONTROL_BINDINGS, DIRECT_CONTROLS},
    host_router::BottomRowLayout,
};
use mi_transport::{ActivationPayload, TargetKeyboard, UserCommandReceiver};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const RULE_MARKER: &str = "Micro Input — six layers v1";
const CLI_CANDIDATES: &[&str] = &[
    "/Applications/Karabiner-Elements.app/Contents/Helpers/karabiner_cli",
    "/Library/Application Support/org.pqrs/Karabiner-Elements/bin/karabiner_cli",
];
const LOG: &str = "/var/log/karabiner/core_service.log";
const OUTPUT_LIMIT: u64 = 2 * 1024 * 1024;
const MAX_LOG_READ_BYTES: u64 = 1024 * 1024;

pub fn resolve_karabiner_cli_from(candidates: &[&Path]) -> Result<PathBuf, String> {
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.to_path_buf())
        .ok_or_else(|| "未找到 karabiner_cli 可执行文件".into())
}

pub fn resolve_karabiner_cli() -> Result<PathBuf, String> {
    let candidates: Vec<&Path> = CLI_CANDIDATES.iter().map(Path::new).collect();
    resolve_karabiner_cli_from(&candidates)
}

pub fn rules(
    target: &TargetKeyboard,
    receiver: &UserCommandReceiver,
    layout: BottomRowLayout,
) -> Result<Value, String> {
    if target.vendor_id != 0x303a
        || target.product_id != 0x8298
        || target.location_id == 0
        || target.usb_parent == 0
    {
        return Err("键盘身份未确认".into());
    }
    let condition = json!({"type":"device_if","identifiers":[{"vendor_id":target.vendor_id,"product_id":target.product_id,"is_keyboard":true,"location_id":target.location_id}]});
    let manipulators: Vec<_> = DIRECT_CONTROLS.iter().map(|control| {
        let merged = layout.is_merged() && matches!(control.slot, 10 | 11);
        let id = if merged { control.id - control.slot + 10 } else { control.id };
        let mut send = json!({"send_user_command":{"endpoint":receiver.path(),"payload":ActivationPayload::new(receiver.generation(),id)},"repeat":false});
        let to = if merged {
            let prefix = format!("mi_{}_l{}", receiver.generation(), control.layer);
            let held = format!("{prefix}_s{}", control.slot);
            let other = format!("{prefix}_s{}", if control.slot == 10 { 11 } else { 10 });
            let latch = format!("{prefix}_latch");
            send["conditions"] = json!([{"type":"variable_if","name":latch,"value":0}]);
            json!([
                {"set_variable":{"name":held,"value":1,"key_up_value":0},"repeat":false},
                send,
                {"set_variable":{"name":latch,"value":1,"key_up_expression":other},"repeat":false}
            ])
        } else { json!([send]) };
        let mut manipulator = json!({"type":"basic", "from":{"key_code":control.key_code,"modifiers":{"optional":["any"]}}, "conditions":[condition.clone()], "to":to});
        if CONTROL_BINDINGS[control.slot as usize] == "ENC_CLK" {
            manipulator["to_after_key_up"] = json!([{
                "send_user_command": {
                    "endpoint": receiver.path(),
                    "payload": ActivationPayload::new_release(receiver.generation(), id),
                }
            }]);
        }
        manipulator
    }).collect();
    Ok(json!({"description":RULE_MARKER,"manipulators":manipulators}))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigPatch {
    pub path: PathBuf,
    pub original: Vec<u8>,
    pub installed: Vec<u8>,
    pub profile_name: String,
    pub original_device: Option<Value>,
    pub installed_device: Value,
}

fn selected_profile(value: &mut Value) -> Result<&mut Value, String> {
    let profiles = value["profiles"]
        .as_array_mut()
        .ok_or("Karabiner profiles 缺失")?;
    let indices: Vec<_> = profiles
        .iter()
        .enumerate()
        .filter(|(_, p)| p["selected"] == true)
        .map(|(i, _)| i)
        .collect();
    if indices.len() != 1 {
        return Err("Karabiner 当前方案不唯一".into());
    }
    Ok(&mut profiles[indices[0]])
}

fn is_target_device(device: &Value, target: &TargetKeyboard) -> bool {
    let id = &device["identifiers"];
    id["vendor_id"] == target.vendor_id
        && id["product_id"] == target.product_id
        && id["is_keyboard"] == true
}

pub fn prepare_config(
    path: PathBuf,
    original: Vec<u8>,
    target: &TargetKeyboard,
    rule: Value,
) -> Result<ConfigPatch, String> {
    let mut value: Value =
        serde_json::from_slice(&original).map_err(|e| format!("Karabiner 配置解析失败：{e}"))?;
    let profile = selected_profile(&mut value)?;
    let profile_name = profile["name"]
        .as_str()
        .ok_or("Karabiner 方案名称缺失")?
        .to_owned();
    if profile.get("complex_modifications").is_none() {
        profile["complex_modifications"] = json!({"rules":[]});
    }
    if profile["complex_modifications"].get("rules").is_none() {
        profile["complex_modifications"]["rules"] = json!([]);
    }
    let existing = profile["complex_modifications"]["rules"]
        .as_array_mut()
        .ok_or("Karabiner rules 不是数组")?;
    existing.retain(|rule| rule["description"] != RULE_MARKER);
    existing.insert(0, rule);
    if profile.get("devices").is_none() {
        profile["devices"] = json!([]);
    }
    let devices = profile["devices"]
        .as_array_mut()
        .ok_or("Karabiner devices 不是数组")?;
    if devices
        .iter()
        .filter(|device| is_target_device(device, target))
        .count()
        > 1
    {
        return Err("Karabiner 中的目标键盘设置重复".into());
    }
    let original_device = devices
        .iter()
        .find(|device| is_target_device(device, target))
        .cloned();
    let mut installed_device = original_device.clone().unwrap_or_else(|| json!({"identifiers":{"vendor_id":target.vendor_id,"product_id":target.product_id,"is_keyboard":true}}));
    installed_device["ignore"] = json!(false);
    devices.retain(|device| !is_target_device(device, target));
    devices.push(installed_device.clone());
    let installed = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    Ok(ConfigPatch {
        path,
        original,
        installed,
        profile_name,
        original_device,
        installed_device,
    })
}

pub fn replace_if_unchanged(
    path: &Path,
    expected: &[u8],
    replacement: &[u8],
) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("配置文件不是普通文件".into());
    }
    if fs::read(path).map_err(|e| e.to_string())? != expected {
        return Err("配置已被其他程序修改，未覆盖".into());
    }
    let pending = path.with_extension(format!("{}.pending", mi_model::document::new_id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        file.write_all(replacement)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        if fs::read(path).map_err(|e| e.to_string())? != expected {
            return Err("配置在保存期间发生变化，未覆盖".into());
        }
        fs::rename(&pending, path).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            File::open(parent)
                .and_then(|file| file.sync_all())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    let _ = fs::remove_file(pending);
    result
}

impl ConfigPatch {
    pub fn install(&self, backup_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(backup_dir).map_err(|e| e.to_string())?;
        let backup = backup_dir.join(format!("karabiner-{}.json", mi_model::document::new_id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&backup)
            .map_err(|e| e.to_string())?;
        file.write_all(&self.original)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        File::open(backup_dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        replace_if_unchanged(&self.path, &self.original, &self.installed)
    }

    pub fn rollback_exact(&self) -> Result<(), String> {
        replace_if_unchanged(&self.path, &self.installed, &self.original)
    }

    pub fn restore_owned(&self, target: &TargetKeyboard) -> Result<(), String> {
        let current = fs::read(&self.path).map_err(|e| e.to_string())?;
        if current == self.installed {
            return self.rollback_exact();
        }
        let mut value: Value = serde_json::from_slice(&current).map_err(|e| e.to_string())?;
        let profiles = value["profiles"]
            .as_array_mut()
            .ok_or("Karabiner profiles 缺失")?;
        if profiles
            .iter()
            .filter(|profile| profile["name"] == self.profile_name)
            .count()
            != 1
        {
            return Err("原 Karabiner 方案身份已变化".into());
        }
        let profile = profiles
            .iter_mut()
            .find(|profile| profile["name"] == self.profile_name)
            .unwrap();
        let rules = profile["complex_modifications"]["rules"]
            .as_array_mut()
            .ok_or("Karabiner rules 已变化")?;
        let installed: Value =
            serde_json::from_slice(&self.installed).map_err(|e| e.to_string())?;
        let owned = installed["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == self.profile_name)
            .unwrap()["complex_modifications"]["rules"][0]
            .clone();
        if rules
            .iter()
            .any(|rule| rule["description"] == RULE_MARKER && rule != &owned)
        {
            return Err("自有规则已被修改，未覆盖".into());
        }
        rules.retain(|rule| rule["description"] != RULE_MARKER);
        let devices = profile["devices"]
            .as_array_mut()
            .ok_or("Karabiner devices 已变化")?;
        if devices
            .iter()
            .filter(|device| is_target_device(device, target))
            .count()
            != 1
            || devices
                .iter()
                .find(|device| is_target_device(device, target))
                != Some(&self.installed_device)
        {
            return Err("目标键盘设置已被修改，未覆盖".into());
        }
        devices.retain(|device| !is_target_device(device, target));
        if let Some(original) = &self.original_device {
            devices.push(original.clone());
        }
        replace_if_unchanged(
            &self.path,
            &current,
            &serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
        )
    }
}

fn run(program: impl AsRef<OsStr>, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().unwrap();
    let output = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(2) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = output.join();
            return Err("Karabiner 公开接口超时".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let bytes = output
        .join()
        .map_err(|_| "接口输出线程失败")?
        .map_err(|e| e.to_string())?;
    if !status.success() || bytes.len() as u64 > OUTPUT_LIMIT {
        return Err("Karabiner 公开接口返回失败或超长数据".into());
    }
    Ok(bytes)
}

pub fn config_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or("未找到用户配置目录")?;
    Ok(base.join("karabiner/karabiner.json"))
}

fn service_generation() -> Result<String, String> {
    let bytes = run("/bin/ps", &["-axo", "uid=,pid=,lstart=,comm="])?;
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    let uid = String::from_utf8(run("/usr/bin/id", &["-u"])?)
        .map_err(|e| e.to_string())?
        .trim()
        .parse()
        .map_err(|_| "当前用户身份未确认")?;
    service_generation_from_ps(&text, uid)
}

fn service_generation_from_ps(text: &str, uid: u32) -> Result<String, String> {
    if uid == 0 {
        return Err("六层输入必须由当前桌面用户运行".into());
    }
    let mut roles = [0; 3];
    let mut lines = Vec::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 8 {
            continue;
        }
        let Ok(owner) = fields[0].parse::<u32>() else {
            continue;
        };
        if owner != 0 && owner != uid {
            continue;
        }
        let executable = fields[7..].join(" ");
        let name = Path::new(&executable)
            .file_name()
            .and_then(|name| name.to_str());
        let role = match (name, owner) {
            (Some("Karabiner-Core-Service"), 0) => 0,
            (Some("Karabiner-Core-Service"), owner) if owner == uid => 1,
            (Some("Karabiner-Console-User-Server"), owner) if owner == uid => 2,
            _ => continue,
        };
        if fields[1].parse::<u32>().is_err() {
            return Err("Karabiner 服务进程身份无效".into());
        }
        roles[role] += 1;
        lines.push(line.trim());
    }
    if roles != [1, 1, 1] {
        return Err("Karabiner 系统服务、用户服务或桌面服务的当前代次未确认".into());
    }
    lines.sort_unstable();
    Ok(lines.join("\n"))
}

fn field_values<'a>(value: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            for (name, value) in map {
                if name == key {
                    out.push(value);
                }
                field_values(value, key, out);
            }
        }
        Value::Array(values) => {
            for value in values {
                field_values(value, key, out);
            }
        }
        _ => {}
    }
}

pub fn validate_guidance(value: &Value) -> Result<(), String> {
    for (key, expected) in [
        ("driver_activated", true),
        ("driver_connected", true),
        ("driver_version_mismatched", false),
        ("virtual_hid_device_service_client_connected", true),
        ("virtual_hid_keyboard_ready", true),
        ("karabiner_json_permission_error", false),
    ] {
        let mut values = Vec::new();
        field_values(value, key, &mut values);
        if values.is_empty() || values.iter().any(|value| value.as_bool() != Some(expected)) {
            return Err(format!("Karabiner 未就绪：{key}"));
        }
    }
    let mut parse_errors = Vec::new();
    field_values(
        value,
        "karabiner_json_parse_error_message",
        &mut parse_errors,
    );
    if parse_errors.is_empty() || parse_errors.iter().any(|value| value.as_str() != Some("")) {
        return Err("Karabiner 配置解析失败".into());
    }
    Ok(())
}

pub fn keyboard_device_id(devices: &Value, target: &TargetKeyboard) -> Result<u64, String> {
    let rows = devices.as_array().ok_or("Karabiner 设备列表格式不符")?;
    let matches: Vec<_> = rows
        .iter()
        .filter(|row| {
            is_target_device(&json!({"identifiers":row["device_identifiers"]}), target)
                && row["location_id"] == target.location_id
                && row["transport"] == "USB"
        })
        .collect();
    if matches.len() != 1 {
        return Err("Karabiner 未识别当前 USB 位置的标准键盘".into());
    }
    matches[0]["device_id"]
        .as_u64()
        .ok_or_else(|| "Karabiner 键盘身份缺失".into())
}

#[derive(Clone, Debug)]
pub struct ReloadBaseline {
    generation: String,
    inode: u64,
    offset: u64,
}

#[derive(Clone, Debug)]
pub struct Readiness {
    baseline: ReloadBaseline,
    pub configuration_observed: bool,
    pub keyboard_grabbed: bool,
}

#[cfg(test)]
impl Readiness {
    pub(crate) fn isolated() -> Self {
        Self {
            baseline: ReloadBaseline {
                generation: "isolated".into(),
                inode: 0,
                offset: 0,
            },
            configuration_observed: false,
            keyboard_grabbed: false,
        }
    }
}

impl ReloadBaseline {
    pub fn capture() -> Result<Self, String> {
        let generation = service_generation()?;
        let meta = fs::metadata(LOG).map_err(|e| e.to_string())?;
        Ok(Self {
            generation,
            inode: meta.ino(),
            offset: meta.len(),
        })
    }
    pub fn check(&self, patch: &ConfigPatch, target: &TargetKeyboard) -> Result<Readiness, String> {
        if service_generation()? != self.generation {
            return Err("Karabiner 服务已换代".into());
        }
        if fs::read(&patch.path).map_err(|e| e.to_string())? != patch.installed {
            return Err("Karabiner 配置已变化".into());
        }
        let cli = resolve_karabiner_cli()?;
        let guidance: Value =
            serde_json::from_slice(&run(&cli, &["--show-settings-window-guidance"])?)
                .map_err(|e| e.to_string())?;
        validate_guidance(&guidance)?;
        let devices: Value = serde_json::from_slice(&run(&cli, &["--list-connected-devices"])?)
            .map_err(|e| e.to_string())?;
        let id = keyboard_device_id(&devices, target)?;
        check_log_readiness(Path::new(LOG), self.inode, self.offset, &patch.path, id)?;
        Ok(Readiness {
            baseline: self.clone(),
            configuration_observed: true,
            keyboard_grabbed: true,
        })
    }
}

fn read_log_tail(path: &Path, start_offset: u64) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let tail_start = len.saturating_sub(MAX_LOG_READ_BYTES).max(start_offset);
    file.seek(SeekFrom::Start(tail_start))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_LOG_READ_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn rotated_log_paths(log_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(stem) = log_path.file_stem().and_then(OsStr::to_str) {
        let parent = log_path.parent().unwrap_or_else(|| Path::new(""));
        let ext = log_path.extension().and_then(OsStr::to_str);
        for generation in [3_u8, 2, 1] {
            let name = match ext {
                Some(ext) => format!("{stem}.{generation}.{ext}"),
                None => format!("{stem}.{generation}"),
            };
            let sibling = parent.join(name);
            if sibling.is_file() {
                paths.push(sibling);
            }
        }
    }
    paths.push(log_path.to_path_buf());
    paths
}

fn read_rotated_log_history(log_path: &Path, current_tail: &str) -> (String, bool) {
    let paths = rotated_log_paths(log_path);
    let has_rotated_siblings = paths.len() > 1;
    let mut history = String::new();
    for sibling in &paths[..paths.len().saturating_sub(1)] {
        if let Ok(bytes) = read_log_tail(sibling, 0) {
            history.push_str(&String::from_utf8_lossy(&bytes));
            if !history.ends_with('\n') {
                history.push('\n');
            }
        }
    }
    history.push_str(current_tail);
    (history, has_rotated_siblings)
}

fn check_log_readiness(
    log_path: &Path,
    expected_inode: u64,
    baseline_offset: u64,
    patch_path: &Path,
    device_id: u64,
) -> Result<(), String> {
    let meta = fs::metadata(log_path).map_err(|e| e.to_string())?;
    let tail_bytes = read_log_tail(log_path, 0)?;
    let tail_log = String::from_utf8_lossy(&tail_bytes);
    if tail_log.contains("karabiner.json error:") {
        return Err("尚未观测到 Karabiner 成功载入本次配置".into());
    }
    let rotated_since_baseline = meta.ino() != expected_inode || meta.len() < baseline_offset;
    if !rotated_since_baseline {
        let new_bytes = read_log_tail(log_path, baseline_offset)?;
        let new = String::from_utf8_lossy(&new_bytes);
        if !new.lines().any(|line| {
            line.contains("Load ") && line.contains(patch_path.to_string_lossy().as_ref())
        }) || !new.contains("core_configuration is updated.")
            || new.contains("karabiner.json error:")
        {
            return Err("尚未观测到 Karabiner 成功载入本次配置".into());
        }
    }
    let (history, has_rotated_siblings) = read_rotated_log_history(log_path, &tail_log);
    validate_grabbed_with_rotation(
        &history,
        device_id,
        has_rotated_siblings || rotated_since_baseline,
    )
}

pub fn validate_grabbed(log: &str, id: u64) -> Result<(), String> {
    validate_grabbed_with_rotation(log, id, false)
}

pub fn validate_grabbed_with_rotation(
    history: &str,
    id: u64,
    has_rotated_siblings: bool,
) -> Result<(), String> {
    let marker = format!("(device_id:{id})");
    let last = history.lines().rfind(|line| {
        line.contains(&marker)
            && (line.contains("hid device events monitor") || line.contains("terminated"))
    });
    match last {
        Some(line) if line.contains("hid device events monitor is started (grabbed).") => Ok(()),
        Some(_) => Err("目标标准键盘未被 Karabiner 抓取".into()),
        None if has_rotated_siblings => Ok(()),
        None => Err("未找到目标键盘的抓取记录".into()),
    }
}

impl Readiness {
    pub fn recheck(&self, patch: &ConfigPatch, target: &TargetKeyboard) -> Result<(), String> {
        self.recheck_preferred(patch, target, None)
    }

    pub fn recheck_preferred(
        &self,
        patch: &ConfigPatch,
        target: &TargetKeyboard,
        preferred_serial: Option<&str>,
    ) -> Result<(), String> {
        if mi_transport::discover_target_keyboard_preferred(preferred_serial)
            .map_err(|e| e.to_string())?
            != *target
        {
            return Err("目标 USB 设备已变化".into());
        }
        self.baseline.check(patch, target).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> TargetKeyboard {
        TargetKeyboard {
            vendor_id: 0x303a,
            product_id: 0x8298,
            location_id: 42,
            usb_parent: 8,
            vendor_path: "synthetic-vendor".into(),
        }
    }

    #[test]
    fn generated_rules_cover_all_usages_and_merged_switches_latch_until_both_release() {
        let receiver = UserCommandReceiver::bind().unwrap();
        let rule = rules(&target(), &receiver, BottomRowLayout::Merged).unwrap();
        let manipulators = rule["manipulators"].as_array().unwrap();
        assert_eq!(manipulators.len(), 60);
        let mut keys = std::collections::BTreeSet::new();
        for manipulator in manipulators {
            assert!(keys.insert(manipulator["from"]["key_code"].as_str().unwrap()));
            assert_eq!(manipulator["from"]["modifiers"]["optional"], json!(["any"]));
            assert_eq!(
                manipulator["conditions"][0]["identifiers"][0],
                json!({"vendor_id":12346,"product_id":33432,"is_keyboard":true,"location_id":42})
            );
            for event in manipulator["to"].as_array().unwrap() {
                assert_eq!(event["repeat"], false);
                assert!(event.get("key_code").is_none());
                assert!(event.get("shell_command").is_none());
            }
        }
        let mut variables = std::collections::BTreeMap::<String, i64>::new();
        let mut held = std::collections::BTreeSet::new();
        let mut activations = Vec::new();
        for (slot, down) in [
            (10, true),
            (10, true),
            (11, true),
            (10, false),
            (10, true),
            (11, false),
            (10, false),
            (11, true),
            (11, false),
            (10, true),
            (10, false),
        ] {
            let to = manipulators[slot]["to"].as_array().unwrap();
            if down {
                if !held.insert(slot) {
                    continue;
                }
                let before = variables.clone();
                for event in to {
                    if let Some(variable) = event.get("set_variable") {
                        variables.insert(
                            variable["name"].as_str().unwrap().into(),
                            variable["value"].as_i64().unwrap(),
                        );
                    }
                    if let Some(command) = event.get("send_user_command") {
                        let allowed = event["conditions"].as_array().is_none_or(|conditions| {
                            conditions.iter().all(|condition| {
                                before
                                    .get(condition["name"].as_str().unwrap())
                                    .copied()
                                    .unwrap_or(0)
                                    == condition["value"].as_i64().unwrap()
                            })
                        });
                        if allowed {
                            activations.push(command["payload"]["control"].as_u64().unwrap());
                        }
                    }
                }
            } else {
                held.remove(&slot);
                for event in to {
                    if let Some(variable) = event.get("set_variable") {
                        let value = variable["key_up_value"].as_i64().unwrap_or_else(|| {
                            variables
                                .get(variable["key_up_expression"].as_str().unwrap())
                                .copied()
                                .unwrap_or(0)
                        });
                        variables.insert(variable["name"].as_str().unwrap().into(), value);
                    }
                }
            }
        }
        assert_eq!(activations, vec![10, 10, 10]);
        let next = UserCommandReceiver::bind().unwrap();
        assert_ne!(
            rules(&target(), &next, BottomRowLayout::Merged).unwrap(),
            rule
        );

        let mut release_controls = Vec::new();
        for (idx, manipulator) in manipulators.iter().enumerate() {
            if let Some(after_up) = manipulator.get("to_after_key_up") {
                let events = after_up.as_array().unwrap();
                assert_eq!(events.len(), 1);
                let payload = &events[0]["send_user_command"]["payload"];
                assert_eq!(payload["released"], json!(true));
                release_controls.push((idx, payload["control"].as_u64().unwrap()));
            }
        }
        assert_eq!(release_controls, vec![(15, 15), (35, 35), (55, 55)]);
    }

    #[test]
    fn configuration_preserves_user_rules_and_restores_without_overwriting_concurrent_edits() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("karabiner.json");
        let original = br#"{ "global":{"unknown":42},"profiles":[{"name":"User","selected":true,"devices":[{"identifiers":{"vendor_id":12346,"product_id":33432,"is_keyboard":true},"ignore":true,"user_field":7}],"complex_modifications":{"rules":[{"description":"User rule","manipulators":[]}]}},{"name":"Other","selected":false,"private_field":[1,2]}] }"#.to_vec();
        fs::write(&path, &original).unwrap();
        let receiver = UserCommandReceiver::bind().unwrap();
        let patch = prepare_config(
            path.clone(),
            original.clone(),
            &target(),
            rules(&target(), &receiver, BottomRowLayout::Split).unwrap(),
        )
        .unwrap();
        patch.install(&temp.path().join("backups")).unwrap();
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            value["profiles"][0]["complex_modifications"]["rules"][1]["description"],
            "User rule"
        );
        assert_eq!(value["profiles"][0]["devices"][0]["user_field"], 7);
        assert_eq!(value["profiles"][1]["private_field"], json!([1, 2]));
        value["global"]["newUserEdit"] = json!(true);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(patch.rollback_exact().is_err());
        patch.restore_owned(&target()).unwrap();
        let restored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(restored["global"]["newUserEdit"], true);
        assert_eq!(restored["profiles"][0]["devices"][0]["ignore"], true);
        assert_eq!(
            restored["profiles"][0]["complex_modifications"]["rules"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            fs::read_dir(temp.path().join("backups")).unwrap().count(),
            1
        );
        let backup = fs::read_dir(temp.path().join("backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(fs::read(backup).unwrap(), original);
        assert!(patch.install(&temp.path().join("backups")).is_err());
    }

    #[test]
    fn readiness_requires_real_keyboard_location_grab_and_error_free_guidance() {
        let guidance = json!({"daemon":{"driver_activated":true,"driver_connected":true,"driver_version_mismatched":false,"virtual_hid_device_service_client_connected":true,"virtual_hid_keyboard_ready":true,"karabiner_json_permission_error":false,"karabiner_json_parse_error_message":""},"console":{"karabiner_json_permission_error":false}});
        validate_guidance(&guidance).unwrap();
        let mut bad = guidance.clone();
        bad["console"]["karabiner_json_permission_error"] = json!(true);
        assert!(validate_guidance(&bad).is_err());
        let devices = json!([{"device_id":17,"location_id":42,"transport":"USB","device_identifiers":{"vendor_id":12346,"product_id":33432,"is_keyboard":true}},{"device_id":18,"location_id":42,"transport":"USB","device_identifiers":{"vendor_id":12346,"product_id":33432,"is_consumer":true}}]);
        assert_eq!(keyboard_device_id(&devices, &target()).unwrap(), 17);
        let mut other = target();
        other.location_id = 43;
        assert!(keyboard_device_id(&devices, &other).is_err());
        validate_grabbed(
            "keyboard (device_id:17) hid device events monitor is started (grabbed).",
            17,
        )
        .unwrap();
        assert!(validate_grabbed(
            "consumer (device_id:18) hid device events monitor is started (grabbed).",
            17
        )
        .is_err());
        assert!(validate_grabbed("keyboard (device_id:17) hid device events monitor is started (grabbed).\nkeyboard (device_id:17) hid device events monitor is stopped.",17).is_err());
        assert!(validate_grabbed(
            "keyboard (device_id:17) hid device events monitor is started (observed).",
            17
        )
        .is_err());
    }

    #[test]
    fn check_log_readiness_handles_logs_larger_than_one_megabyte_and_log_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("core_service.log");
        let patch_path = dir.path().join("karabiner.json");
        fs::write(&patch_path, b"{}").unwrap();

        let padding = "x".repeat(1_200_000) + "\n";
        fs::write(&log_path, &padding).unwrap();
        let meta = fs::metadata(&log_path).unwrap();
        let baseline_inode = meta.ino();
        let baseline_offset = meta.len();

        let post_reload = format!(
            "keyboard (device_id:17) hid device events monitor is started (grabbed).\nLoad {}\ncore_configuration is updated.\n",
            patch_path.display()
        );
        let mut f = OpenOptions::new().append(true).open(&log_path).unwrap();
        f.write_all(post_reload.as_bytes()).unwrap();
        f.sync_all().unwrap();

        check_log_readiness(&log_path, baseline_inode, baseline_offset, &patch_path, 17).unwrap();

        let rotated = "keyboard (device_id:17) hid device events monitor is started (grabbed).\n";
        fs::write(&log_path, rotated).unwrap();
        check_log_readiness(&log_path, baseline_inode, baseline_offset, &patch_path, 17).unwrap();
    }

    #[test]
    fn resolve_karabiner_cli_prefers_v17_app_helper_and_falls_back_to_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let v17 = dir
            .path()
            .join("Karabiner-Elements.app/Contents/Helpers/karabiner_cli");
        let legacy = dir
            .path()
            .join("org.pqrs/Karabiner-Elements/bin/karabiner_cli");
        let candidates = [v17.as_path(), legacy.as_path()];

        assert!(resolve_karabiner_cli_from(&candidates).is_err());

        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, b"#!/bin/sh\n").unwrap();
        assert_eq!(resolve_karabiner_cli_from(&candidates).unwrap(), legacy);

        fs::create_dir_all(&v17).unwrap();
        assert_eq!(resolve_karabiner_cli_from(&candidates).unwrap(), legacy);
        fs::remove_dir(&v17).unwrap();

        fs::write(&v17, b"#!/bin/sh\n").unwrap();
        assert_eq!(resolve_karabiner_cli_from(&candidates).unwrap(), v17);
    }

    #[test]
    fn check_log_readiness_finds_grabbed_device_in_rotated_sibling_and_rejects_later_stop() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("core_service.log");
        let sibling_1 = dir.path().join("core_service.1.log");
        let sibling_2 = dir.path().join("core_service.2.log");
        let patch_path = dir.path().join("karabiner.json");
        fs::write(&patch_path, b"{}").unwrap();

        fs::write(
            &sibling_2,
            "Creator Micro 2 (device_id:17) hid device events monitor is stopped.\n",
        )
        .unwrap();
        fs::write(
            &sibling_1,
            "Creator Micro 2 (device_id:17) hid device events monitor is started (grabbed).\n",
        )
        .unwrap();
        fs::write(&log_path, "unrelated line\n").unwrap();
        let meta = fs::metadata(&log_path).unwrap();
        let baseline_inode = meta.ino();
        let baseline_offset = meta.len();

        let reload = format!(
            "Load {}\ncore_configuration is updated.\n",
            patch_path.display()
        );
        let mut f = OpenOptions::new().append(true).open(&log_path).unwrap();
        f.write_all(reload.as_bytes()).unwrap();
        f.sync_all().unwrap();

        check_log_readiness(&log_path, baseline_inode, baseline_offset, &patch_path, 17).unwrap();

        f.write_all(b"Creator Micro 2 (device_id:17) hid device events monitor is stopped.\n")
            .unwrap();
        f.sync_all().unwrap();
        assert!(
            check_log_readiness(&log_path, baseline_inode, baseline_offset, &patch_path, 17)
                .is_err()
        );
    }

    #[test]
    fn check_log_readiness_tolerates_aged_out_grab_only_when_rotated_siblings_exist_and_reload_succeeded(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("core_service.log");
        let sibling_1 = dir.path().join("core_service.1.log");
        let patch_path = dir.path().join("karabiner.json");
        fs::write(&patch_path, b"{}").unwrap();

        fs::write(&log_path, "earlier unrelated traffic\n").unwrap();
        let meta = fs::metadata(&log_path).unwrap();
        let baseline_inode = meta.ino();
        let baseline_offset = meta.len();

        let reload = format!(
            "Load {}\ncore_configuration is updated.\n",
            patch_path.display()
        );
        let mut f = OpenOptions::new().append(true).open(&log_path).unwrap();
        f.write_all(reload.as_bytes()).unwrap();
        f.sync_all().unwrap();

        assert_eq!(
            check_log_readiness(&log_path, baseline_inode, baseline_offset, &patch_path, 17)
                .unwrap_err(),
            "未找到目标键盘的抓取记录"
        );

        fs::write(
            &sibling_1,
            "older unrelated traffic without device mention\n",
        )
        .unwrap();
        check_log_readiness(&log_path, baseline_inode, baseline_offset, &patch_path, 17).unwrap();

        let after_meta = fs::metadata(&log_path).unwrap();
        assert_eq!(
            check_log_readiness(
                &log_path,
                after_meta.ino(),
                after_meta.len(),
                &patch_path,
                17,
            )
            .unwrap_err(),
            "尚未观测到 Karabiner 成功载入本次配置"
        );
    }
}
