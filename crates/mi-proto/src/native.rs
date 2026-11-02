//! R-D04/R-D13; E-052/E-075: hidapi 2.6.7 numbered macOS reports.
use crate::*;
/// Every production request is validated before framing or touching the HID link.
pub fn validate_read_only(r: &RpcRequest) -> Result<(), ProtoError> {
    let valid = match r.method {
        Method::DeviceStatus | Method::SysVersion => r.params.is_null(),
        Method::FsList => r.params == json!({"checksum": true}),
        Method::FsReadbin => r.params.as_object().is_some_and(|p| {
            p.len() == 3
                && matches!(
                    p.get("file").and_then(Value::as_str),
                    Some("keymap.json" | "smart_actions.json")
                )
                && p.get("offset").and_then(Value::as_u64).is_some()
                && p.get("len")
                    .and_then(Value::as_u64)
                    .is_some_and(|n| (1..=3072).contains(&n))
        }),
        Method::FsRead => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ProtoError::Invalid(
            "read-only method/parameters rejected".into(),
        ))
    }
}
#[derive(Default)]
pub struct NativeFramer {
    bytes: Vec<u8>,
    depth: usize,
    quoted: bool,
    escaped: bool,
}
impl Framer for NativeFramer {
    fn encode(&self, payload: &[u8]) -> Result<Vec<Report>, ProtoError> {
        Ok(payload
            .chunks(61)
            .map(|part| {
                let mut r = vec![0; 64];
                r[0] = 6;
                r[1] = 2;
                r[2] = part.len() as u8;
                r[3..3 + part.len()].copy_from_slice(part);
                r
            })
            .collect())
    }
    fn feed(&mut self, r: &[u8]) -> Result<Vec<Line>, ProtoError> {
        if r.len() != 64 || r[0] != 6 || r[2] > 61 {
            self.clear();
            return Err(ProtoError::Invalid("invalid numbered HID report".into()));
        }
        // Debug/other channels never enter the RPC accumulator.
        if r[1] != 2 {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for b in &r[3..3 + r[2] as usize] {
            if self.bytes.is_empty() && b.is_ascii_whitespace() {
                continue;
            }
            if self.bytes.is_empty() && *b != b'{' {
                return Err(ProtoError::Invalid("expected JSON object".into()));
            }
            self.bytes.push(*b);
            if self.bytes.len() > 64 * 1024 {
                self.clear();
                return Err(ProtoError::Invalid("RPC exceeds 64 KiB policy".into()));
            }
            if self.quoted {
                if self.escaped {
                    self.escaped = false
                } else if *b == b'\\' {
                    self.escaped = true
                } else if *b == b'"' {
                    self.quoted = false
                }
            } else {
                match *b {
                    b'"' => self.quoted = true,
                    b'{' => self.depth += 1,
                    b'}' => {
                        self.depth -= 1;
                        if self.depth == 0 {
                            out.push(std::mem::take(&mut self.bytes));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(out)
    }
    fn clear(&mut self) {
        self.bytes.clear();
        self.depth = 0;
        self.quoted = false;
        self.escaped = false;
    }
}
