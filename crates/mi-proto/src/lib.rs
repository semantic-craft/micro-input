//! R-D04/R-D05/R-D08/R-D09: read-only RPC vocabulary and deliberately closed wire boundary.
use serde_json::{json, Value};
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProtoError {
    #[error("协议未验证：{0}")]
    Unverified(&'static str),
    #[error("无效 RPC：{0}")]
    Invalid(String),
}
/// No arbitrary method constructor; mutation methods have no enum variant.
/// ```compile_fail
/// use mi_proto::Method;
/// let forbidden = Method::FsWrite;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    SysVersion,
    DeviceStatus,
    FsList,
    FsRead,
    FsReadbin,
}
impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Self::SysVersion => "sys.version",
            Self::DeviceStatus => "device.status",
            Self::FsList => "fs.list",
            Self::FsRead => "fs.read",
            Self::FsReadbin => "fs.readbin",
        }
    }
}
#[derive(Debug, Clone)]
pub struct RpcRequest {
    pub method: Method,
    pub params: Value,
    id: u16,
}
impl RpcRequest {
    pub fn new(method: Method, params: Value, id: u16) -> Result<Self, ProtoError> {
        if id >= 999 {
            return Err(ProtoError::Invalid("id must be < 999".into()));
        }
        Ok(Self { method, params, id })
    }
    pub fn id(&self) -> u16 {
        self.id
    }
}
#[derive(Debug, Clone)]
pub struct RpcResponse {
    pub id: u16,
    pub raw: Value,
}
#[derive(Debug, Clone)]
pub struct Notification {
    pub method: String,
    pub raw: Value,
}
#[derive(Debug, Clone)]
pub enum Incoming {
    Response(RpcResponse),
    Notification(Notification),
}
pub trait RpcCodec: Send + Sync {
    fn encode(&self, r: &RpcRequest) -> Result<Vec<u8>, ProtoError>;
    fn decode(&self, line: &[u8]) -> Result<Incoming, ProtoError>;
}
pub struct JsonCodec;
impl RpcCodec for JsonCodec {
    fn encode(&self, r: &RpcRequest) -> Result<Vec<u8>, ProtoError> {
        let s = json!({"method":r.method.name(),"params":r.params,"id":r.id}).to_string();
        let mut out = String::new();
        for c in s.chars() {
            if c.is_ascii() {
                out.push(c)
            } else {
                let mut b = [0; 2];
                for u in c.encode_utf16(&mut b) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
        }
        Ok(out.into_bytes())
    }
    fn decode(&self, line: &[u8]) -> Result<Incoming, ProtoError> {
        let v: Value =
            serde_json::from_slice(line).map_err(|e| ProtoError::Invalid(e.to_string()))?;
        if !v.is_object() {
            return Err(ProtoError::Invalid("expected object".into()));
        }
        if v.get("id").is_some() {
            let id = v["id"]
                .as_u64()
                .filter(|n| *n < 999)
                .ok_or_else(|| ProtoError::Invalid("invalid id".into()))?;
            return Ok(Incoming::Response(RpcResponse {
                id: id as u16,
                raw: v,
            }));
        }
        let method = v
            .get("method")
            .or_else(|| v.get("m"))
            .and_then(Value::as_str)
            .ok_or_else(|| ProtoError::Invalid("missing method/id".into()))?
            .to_owned();
        Ok(Incoming::Notification(Notification { method, raw: v }))
    }
}
pub type Report = Vec<u8>;
pub type Line = Vec<u8>;
pub trait Framer: Send {
    fn encode(&self, payload: &[u8]) -> Result<Vec<Report>, ProtoError>;
    fn feed(&mut self, report: &[u8]) -> Result<Vec<Line>, ProtoError>;
    fn clear(&mut self);
}
// TODO(UNKNOWN-01): report ID/header/channel/length/newline framing needs verified evidence.
pub struct UnimplementedFramer;
impl Framer for UnimplementedFramer {
    fn encode(&self, _: &[u8]) -> Result<Vec<Report>, ProtoError> {
        Err(ProtoError::Unverified("UNKNOWN-01 HID framing"))
    }
    fn feed(&mut self, _: &[u8]) -> Result<Vec<Line>, ProtoError> {
        Err(ProtoError::Unverified("UNKNOWN-01 HID framing"))
    }
    fn clear(&mut self) {}
}
// TODO(UNKNOWN-03): error and identity response schemas are opaque; never interpret success fields.
#[derive(Debug, Clone)]
pub enum Identity {
    Unknown(Value),
}
// TODO(UNKNOWN-07): notification parameters unknown; variants describe names only, never execute.
#[derive(Debug, Clone)]
pub enum HostNotification {
    InsertText(Value),
    Exec(Value),
    OpenApp(Value),
    OpenUrl(Value),
    Radial(Value),
    CheatSheetShow(Value),
    CheatSheetHide(Value),
    CheatSheetToggle(Value),
    Unknown(Value),
}
impl From<Notification> for HostNotification {
    fn from(n: Notification) -> Self {
        match n.method.as_str() {
            "kb.sa.inserttext" => Self::InsertText(n.raw),
            "kb.sa.exec" => Self::Exec(n.raw),
            "kb.sa.openapp" => Self::OpenApp(n.raw),
            "kb.sa.openurl" => Self::OpenUrl(n.raw),
            "kb.radial" => Self::Radial(n.raw),
            "kb.cs.show" => Self::CheatSheetShow(n.raw),
            "kb.cs.hide" => Self::CheatSheetHide(n.raw),
            "kb.cs.toggle" => Self::CheatSheetToggle(n.raw),
            _ => Self::Unknown(n.raw),
        }
    }
}
/// R-H04: outbound action is intentionally separate from the MVP read-only allowlist.
pub enum HostAction {
    FocusedAppUnknown(Value),
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_framer() {
        assert!(matches!(
            UnimplementedFramer.encode(b"{}"),
            Err(ProtoError::Unverified(_))
        ));
    }
    #[test]
    fn codec() {
        let r = RpcRequest::new(Method::SysVersion, json!({"synthetic":"中😀"}), 998).unwrap();
        let bytes = JsonCodec.encode(&r).unwrap();
        assert!(bytes.is_ascii());
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["params"]["synthetic"],
            "中😀"
        );
        assert!(RpcRequest::new(Method::FsList, Value::Null, 999).is_err());
        assert!(matches!(
            JsonCodec.decode(br#"{"m":"kb.radial"}"#).unwrap(),
            Incoming::Notification(_)
        ));
        assert!(JsonCodec.decode(b"[]").is_err());
    }
}
