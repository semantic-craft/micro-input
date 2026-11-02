//! Candidate read-only device projection, independently implemented from the evidence summary.
//! Exact source bytes belong to the session snapshot; this model never normalizes a file for writing.
use super::*;

fn device_binding(v: &Value) -> Binding {
    match v.as_str() {
        Some("KC_A" | "KC_B" | "KC_ENTER" | "KC_ESC" | "KC_SPACE" | "KC_TAB") => {
            Binding::Basic(Token(v.as_str().unwrap().into()))
        }
        _ if v.is_null() => Binding::Empty,
        _ => Binding::Unknown(v.clone()),
    }
}
/// Device files are not the demo/import interchange format. Missing optional values remain unknown.
pub fn parse_device_files(keymap: &[u8], smart: Option<&[u8]>) -> Result<DeviceConfig, ParseError> {
    let raw: Value = serde_json::from_slice(keymap).map_err(|_| error("keymap JSON 无效"))?;
    if !raw.is_object() {
        return Err(error("keymap 根必须为对象"));
    }
    let smart_actions = match smart {
        None => Value::Null,
        Some([]) => return Err(error("smart_actions.json 存在但为空文件")),
        Some(bytes) => {
            let v: Value =
                serde_json::from_slice(bytes).map_err(|_| error("smart_actions JSON 无效"))?;
            if !v.is_object() || !v.get("smartActions").is_some_and(Value::is_object) {
                return Err(error("smartActions 必须为对象"));
            }
            v
        }
    };
    let mut warnings = vec!["设备投影为只读；活动索引与物理方向语义未核实".into()];
    if smart.is_none() {
        warnings.push("smart_actions.json 文件不存在".into());
    }
    if raw.get("macros").is_some() || raw.get("multiActions").is_some() {
        warnings.push("宏及 Multi Action 保留原始记录；尚未转换为可编辑事件".into());
    }
    if raw.get("multiActionsGroups").is_some()
        && raw.get("multiActionGroups").is_some()
        && raw["multiActionsGroups"] != raw["multiActionGroups"]
    {
        warnings.push("两种 Multi Action 分组字段不同；均保留原值".into());
    }
    let mut profiles = Vec::new();
    let mut profile_ids = Vec::new();
    for (index, p) in array(&raw, "profiles")?.iter().enumerate() {
        if !p.is_object() {
            return Err(error("Profile 必须为对象"));
        }
        let id = p.get("id").cloned().unwrap_or(Value::Null);
        if id.is_null() || profile_ids.contains(&id) {
            warnings.push(format!("Profile {index} ID 缺失或重复；仅按本快照位置浏览"));
        }
        profile_ids.push(id.clone());
        let mut layers = Vec::new();
        let mut layer_ids = Vec::new();
        for (li, l) in array(p, "layers")?.iter().enumerate() {
            let lid = l.get("id").cloned().unwrap_or(Value::Null);
            if lid.is_null() || layer_ids.contains(&lid) {
                warnings.push(format!(
                    "Profile {index} Layer {li} ID 缺失或重复；仅按位置浏览"
                ));
            }
            layer_ids.push(lid.clone());
            let rows = array(&l["layout"], "keymap")?;
            if rows.len() != 4
                || rows
                    .iter()
                    .zip([2, 4, 4, 3])
                    .any(|(r, n)| r.as_array().map(Vec::len) != Some(n))
            {
                return Err(error("设备 keymap 行长必须为 2/4/4/3"));
            }
            let keys: Vec<_> = rows
                .iter()
                .flat_map(|r| r.as_array().unwrap())
                .map(device_binding)
                .collect();
            let encoders = match l["layout"].get("encoders").and_then(Value::as_array) {
                Some(a) if a.len() == 3 => std::array::from_fn(|i| device_binding(&a[i])),
                _ => {
                    warnings.push(format!("Profile {index} Layer {li} 旋钮槽缺失或形状未知"));
                    std::array::from_fn(|_| Binding::Unknown(Value::Null))
                }
            };
            let sectors = l["layout"]["joystick"]
                .get("sectors")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|s| {
                            s.get("k")
                                .map(device_binding)
                                .unwrap_or_else(|| Binding::Unknown(s.clone()))
                        })
                        .collect()
                })
                .unwrap_or_else(|| {
                    warnings.push(format!("Profile {index} Layer {li} 摇杆字段缺失或未知"));
                    vec![]
                });
            let color = l.get("color").cloned().unwrap_or(Value::Null);
            if !color.as_u64().is_some_and(|n| n <= 0xffffff) {
                warnings.push(format!("Profile {index} Layer {li} 颜色未知；原值保留"));
            }
            if l.get("lights").is_none() {
                warnings.push(format!("Profile {index} Layer {li} 灯光字段缺失"));
            }
            layers.push(Layer {
                id: lid,
                name: l
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Layer {}（名称未知）", li + 1)),
                color,
                keys: keys.try_into().map_err(|_| error("应有 13 键"))?,
                encoders,
                joystick: Joystick {
                    kind: l["layout"]["joystick"].clone(),
                    sectors,
                },
                lights: Lights {
                    backlight: l["lights"]["backlight"].clone(),
                    underglow: l["lights"]["underglow"].clone(),
                },
            });
        }
        profiles.push(Profile {
            id,
            index,
            name: p
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Profile {}（名称未知）", index + 1)),
            layers,
        });
    }
    Ok(DeviceConfig {
        source: Source::Imported,
        read_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        firmware: None,
        profiles,
        active_profile: ActiveProfile::Unknown(raw["activeProfileId"].clone()),
        raw,
        warnings,
        macros: vec![],
        multi_actions: vec![],
        smart_actions,
    })
}
