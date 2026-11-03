//! Candidate chunk encoding only. No transport authority and no arbitrary path or method.
//! The production read-only request vocabulary remains unchanged.
use crate::ProtoError;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
pub const CHUNK_BYTES: usize = 3072;
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
pub fn keymap_chunk(candidate: &[u8], offset: usize, id: u16) -> Result<Vec<u8>, ProtoError> {
    if id >= 999
        || candidate.is_empty()
        || candidate.len() > MAX_FILE_BYTES
        || offset >= candidate.len()
        || offset % CHUNK_BYTES != 0
    {
        return Err(ProtoError::Invalid("invalid bounded keymap chunk".into()));
    }
    let end = offset.saturating_add(CHUNK_BYTES).min(candidate.len());
    serde_json::to_vec(&json!({"id":id,"method":"fs.writebin","params":{"file":"keymap.json","data":STANDARD.encode(&candidate[offset..end]),"append":true,"offset":offset,"completed":end==candidate.len()}})).map_err(|e|ProtoError::Invalid(e.to_string()))
}
/// Synthetic acknowledgement contract, deliberately not asserted to be firmware behavior.
pub fn synthetic_ack(reply: &Value) -> Result<(), ProtoError> {
    if reply.get("error").is_some()
        || reply.get("result") != Some(&json!({"synthetic_written":true}))
    {
        return Err(ProtoError::Invalid(
            "synthetic chunk not acknowledged".into(),
        ));
    }
    Ok(())
}
