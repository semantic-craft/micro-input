//! Evidence-based fs.list/readbin adapter; only two exact paths are representable.
use super::*;
use base64::Engine;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigFile {
    Keymap,
    SmartActions,
}
impl ConfigFile {
    pub fn name(self) -> &'static str {
        match self {
            Self::Keymap => "keymap.json",
            Self::SmartActions => "smart_actions.json",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub file: ConfigFile,
    pub size: u64,
    pub checksum: [u8; 20],
}
impl FileMeta {
    pub fn checksum_hex(&self) -> String {
        self.checksum.iter().map(|b| format!("{b:02x}")).collect()
    }
}
pub struct ReadChunk {
    pub total_size: u64,
    pub data: Vec<u8>,
}
#[derive(Clone)]
pub struct VerifiedFile {
    pub meta: FileMeta,
    pub bytes: Arc<[u8]>,
}
#[derive(Clone)]
pub struct DeviceFiles {
    pub keymap: VerifiedFile,
    pub smart_actions: Option<VerifiedFile>,
}
pub struct ReadOnlySchema;
fn invalid(message: &str) -> ProtoError {
    ProtoError::Invalid(message.into())
}
fn result(v: &Value) -> Result<&Value, ProtoError> {
    if v.get("error").is_some() {
        return Err(invalid("文件 RPC 返回错误；快照未更新"));
    }
    v.get("result")
        .ok_or_else(|| invalid("文件响应缺少 result"))
}
fn size(v: &Value) -> Result<u64, ProtoError> {
    v.as_u64()
        .or_else(|| {
            v.as_str()
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|s| s.parse().ok())
        })
        .ok_or_else(|| invalid("文件 size 必须为无符号整数"))
}
impl DeviceSchema for ReadOnlySchema {
    fn identity_request(&self) -> Result<(Method, Value), ProtoError> {
        Ok((Method::DeviceStatus, Value::Null))
    }
    fn list_request(&self) -> Result<(Method, Value), ProtoError> {
        Ok((Method::FsList, serde_json::json!({"checksum":true})))
    }
    fn files(&self, response: &Value) -> Result<Vec<FileMeta>, ProtoError> {
        let entries = result(response)?
            .as_array()
            .ok_or_else(|| invalid("fs.list result 必须为数组"))?;
        let mut files = vec![];
        for e in entries {
            let file = match e["name"].as_str() {
                Some("keymap.json") => ConfigFile::Keymap,
                Some("smart_actions.json") => ConfigFile::SmartActions,
                _ => continue,
            };
            if files.iter().any(|m: &FileMeta| m.file == file) {
                return Err(invalid("重复目标文件名"));
            }
            let size = size(&e["size"])?;
            let checksum = e["checksum"]
                .as_str()
                .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| invalid("缺少或无效 SHA-1 checksum"))?;
            let mut digest = [0; 20];
            for (i, b) in digest.iter_mut().enumerate() {
                *b = u8::from_str_radix(&checksum[i * 2..i * 2 + 2], 16)
                    .map_err(|_| invalid("checksum 无效"))?;
            }
            files.push(FileMeta {
                file,
                size,
                checksum: digest,
            });
        }
        files.sort_by_key(|f| f.file);
        Ok(files)
    }
    fn read_request(
        &self,
        file: ConfigFile,
        offset: u64,
        len: u32,
    ) -> Result<(Method, Value), ProtoError> {
        Ok((
            Method::FsReadbin,
            serde_json::json!({"file":file.name(),"offset":offset,"len":len}),
        ))
    }
    fn chunk(&self, response: &Value) -> Result<ReadChunk, ProtoError> {
        let r = result(response)?;
        let total_size = r["total_size"]
            .as_u64()
            .ok_or_else(|| invalid("total_size 必须为非负整数"))?;
        let encoded = r["data"]
            .as_str()
            .filter(|s| s.len() <= 4096)
            .ok_or_else(|| invalid("data 缺失或超限"))?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid("无效 Base64 数据"))?;
        Ok(ReadChunk { total_size, data })
    }
    fn parse(&self, keymap: &[u8], smart: Option<&[u8]>) -> Result<DeviceConfig, SessionError> {
        mi_model::parse_device_files(keymap, smart).map_err(|e| SessionError::Config(e.to_string()))
    }
}
