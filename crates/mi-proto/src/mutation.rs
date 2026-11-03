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
/// Candidate client-side response validation, not proof that a file was written.
/// Input 1.1.0's sendRpcCall and writeFileChunkedFromStr require a result/error
/// envelope but impose no fs.writebin or lights.preview result payload schema (IA-07/17). Match the
/// request ID using our existing alias rules; independent byte readback remains
/// mandatory. This validator grants no native transport authority.
pub fn validate_mutation_response_candidate(
    reply: &Value,
    expected_id: u16,
) -> Result<(), ProtoError> {
    if !reply.is_object()
        || expected_id >= 999
        || crate::alias(reply, "id", "i")?.and_then(Value::as_u64) != Some(u64::from(expected_id))
        || reply.get("error").is_some()
        || reply.get("result").is_none()
    {
        return Err(ProtoError::Invalid(
            "invalid candidate write response".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Matches the hash-pinned Input SDK controlled replay: a UTF-8 code point
    // may cross raw chunk boundaries; offsets count bytes, never characters.
    #[test]
    fn chunk_boundaries_preserve_utf8_bytes_and_final_marker() {
        let data = format!("{}中😀", "a".repeat(3071)).into_bytes();
        let mut restored = Vec::new();
        for (offset, size, completed) in [(0, 3072, false), (3072, 6, true)] {
            let bytes = keymap_chunk(&data, offset, 7).unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            let params = &value["params"];
            let part = STANDARD.decode(params["data"].as_str().unwrap()).unwrap();
            assert_eq!(part.len(), size);
            assert_eq!(params["offset"], offset);
            assert_eq!(params["completed"], completed);
            assert_eq!(params["append"], true);
            assert_eq!(params["file"], "keymap.json");
            assert!(params.get("tx").is_none());
            restored.extend(part);
        }
        assert_eq!(restored, data);
        assert!(keymap_chunk(b"", 0, 7).is_err());
        assert!(keymap_chunk(&data, 1, 7).is_err());
        assert!(keymap_chunk(&data, 0, 999).is_err());
        let exact: Value =
            serde_json::from_slice(&keymap_chunk(&vec![0; 3072], 0, 7).unwrap()).unwrap();
        assert_eq!(exact["params"]["completed"], true);
    }
    #[test]
    fn candidate_accepts_result_presence_without_assigning_payload_semantics() {
        for result in [
            Value::Null,
            json!(true),
            json!(false),
            json!({}),
            json!([1]),
            json!(0),
            json!("ok"),
        ] {
            for id in [json!({"id":7}), json!({"i":7}), json!({"id":7,"i":7})] {
                let mut reply = id;
                reply["result"] = result.clone();
                validate_mutation_response_candidate(&reply, 7).unwrap();
            }
        }
    }

    #[test]
    fn candidate_rejects_unowned_malformed_and_error_responses() {
        for reply in [
            Value::Null,
            json!([]),
            json!(true),
            json!({"result":null}),
            json!({"id":8,"result":null}),
            json!({"id":"7","result":null}),
            json!({"id":7.0,"result":null}),
            json!({"id":7,"i":8,"result":null}),
            json!({"id":7}),
            json!({"id":7,"error":null}),
            json!({"id":7,"error":{"code":500,"message":"failed"}}),
            json!({"id":7,"result":null,"error":null}),
        ] {
            assert!(
                validate_mutation_response_candidate(&reply, 7).is_err(),
                "{reply}"
            );
        }
        assert!(
            validate_mutation_response_candidate(&json!({"id":999,"result":null}), 999).is_err()
        );
    }
}
