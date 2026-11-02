//! R-D04/R-D05/R-D08/R-D09: read-only RPC vocabulary and deliberately closed wire boundary.
use serde_json::{json, Value};
mod native;
pub use native::{validate_read_only, NativeFramer};
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
        let method = alias(&v, "method", "m")?;
        let is_response = v.get("result").is_some() || v.get("error").is_some();
        if let Some(method) = method {
            let method = method
                .as_str()
                .ok_or_else(|| ProtoError::Invalid("invalid method".into()))?
                .to_owned();
            if !is_response {
                return Ok(Incoming::Notification(Notification { method, raw: v }));
            }
            if !["device.status", "sys.version", "fs.list", "fs.readbin"].contains(&method.as_str())
                || v.get("params").is_some()
                || v.get("p").is_some()
            {
                return Err(ProtoError::Invalid("invalid response method echo".into()));
            }
        }
        let id = alias(&v, "id", "i")?
            .and_then(Value::as_u64)
            .filter(|n| *n < 999)
            .ok_or_else(|| ProtoError::Invalid("invalid response id".into()))?;
        if v.get("result").is_some() == v.get("error").is_some() {
            return Err(ProtoError::Invalid(
                "expected exactly one result/error".into(),
            ));
        }
        if let Some(e) = v.get("error") {
            if e.get("code").and_then(Value::as_i64).is_none()
                || e.get("message").and_then(Value::as_str).is_none()
            {
                return Err(ProtoError::Invalid("invalid RPC error".into()));
            }
        }
        Ok(Incoming::Response(RpcResponse {
            id: id as u16,
            raw: v,
        }))
    }
}
fn alias<'a>(v: &'a Value, long: &str, short: &str) -> Result<Option<&'a Value>, ProtoError> {
    if let (Some(a), Some(b)) = (v.get(long), v.get(short)) {
        if a != b {
            return Err(ProtoError::Invalid("conflicting aliases".into()));
        }
    }
    Ok(v.get(long).or_else(|| v.get(short)))
}
pub type Report = Vec<u8>;
pub type Line = Vec<u8>;
pub trait Framer: Send {
    fn encode(&self, payload: &[u8]) -> Result<Vec<Report>, ProtoError>;
    fn feed(&mut self, report: &[u8]) -> Result<Vec<Line>, ProtoError>;
    fn clear(&mut self);
}
// TODO(UNKNOWN-01): candidate backend has E-075 API evidence; current-firmware long reports await application acceptance.
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
    // E-077: native 0.6.3 replies echo the requested read-only method.
    #[test]
    fn response_can_echo_method_without_becoming_a_device_request() {
        assert!(
            matches!(JsonCodec.decode(br#"{"id":7,"method":"device.status","result":{"version":"synthetic"}}"#).unwrap(),Incoming::Response(r) if r.id==7)
        );
        assert!(matches!(
            JsonCodec
                .decode(br#"{"id":7,"method":"host.focused_app","params":{}}"#)
                .unwrap(),
            Incoming::Notification(_)
        ));
        assert!(JsonCodec
            .decode(br#"{"id":7,"method":"host.focused_app","result":{}}"#)
            .is_err());
    }
    #[test]
    fn native_frames_and_object_boundaries() {
        for (length, sizes) in [
            (61, vec![61]),
            (62, vec![61, 1]),
            (122, vec![61, 61]),
            (123, vec![61, 61, 1]),
        ] {
            let frames = NativeFramer::default().encode(&vec![b'x'; length]).unwrap();
            assert_eq!(frames.iter().map(|f| f[2]).collect::<Vec<_>>(), sizes);
            assert!(frames
                .iter()
                .all(|f| f.len() == 64 && f[0] == 6 && f[1] == 2));
        }
        let source =
            "{\"id\":1,\"result\":\"中😀\"}\r\n{\"m\":\"kb.radial\"}{\"i\":2,\"result\":null}";
        let mut reader = NativeFramer::default();
        let mut out = vec![];
        // Force UTF-8 and string punctuation across report boundaries.
        for part in source.as_bytes().chunks(2) {
            let mut report = vec![0; 64];
            report[0] = 6;
            report[1] = 2;
            report[2] = part.len() as u8;
            report[3..3 + part.len()].copy_from_slice(part);
            out.extend(reader.feed(&report).unwrap());
        }
        assert_eq!(out.len(), 3);
        assert_eq!(
            serde_json::from_slice::<Value>(&out[0]).unwrap()["result"],
            "中😀"
        );
        assert!(reader.feed(&[6, 2, 62]).is_err());
        let mut reader = NativeFramer::default();
        for _ in 0..1075 {
            let mut r = vec![b' '; 64];
            r[0] = 6;
            r[1] = 2;
            r[2] = 61;
            r[3] = b'{';
            if reader.feed(&r).is_err() {
                return;
            }
        }
        panic!("unbounded accumulator");
    }
    #[test]
    fn production_exit_is_strictly_read_only() {
        assert!(validate_read_only(
            &RpcRequest::new(Method::DeviceStatus, Value::Null, 0).unwrap()
        )
        .is_ok());
        for (method, params) in [
            (Method::FsRead, json!({"file":"keymap.json"})),
            (
                Method::FsReadbin,
                json!({"file":"private.json","offset":0,"len":1}),
            ),
            (Method::DeviceStatus, json!({"activate":true})),
            (
                Method::FsReadbin,
                json!({"file":"keymap.json","offset":0,"len":1,"write":true}),
            ),
        ] {
            assert!(validate_read_only(&RpcRequest::new(method, params, 0).unwrap()).is_err());
        }
    }
    // R-D05/R-D13: a device request carrying our ID is never a response.
    #[test]
    fn strict_response_envelopes_and_compact_ids() {
        assert!(matches!(
            JsonCodec
                .decode(br#"{"id":7,"method":"host.focused_app","params":{}}"#)
                .unwrap(),
            Incoming::Notification(_)
        ));
        assert!(
            matches!(JsonCodec.decode(br#"{"i":7,"result":{"version":"0.6.3"}}"#).unwrap(), Incoming::Response(r) if r.id == 7)
        );
        for invalid in [
            br#"{"id":7}"#.as_slice(),
            br#"{"id":7,"result":{},"error":null}"#,
            br#"{"id":7,"error":{"code":"bad","message":"error"}}"#,
            br#"{"id":7,"i":8,"result":{}}"#,
        ] {
            assert!(JsonCodec.decode(invalid).is_err());
        }
    }
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
