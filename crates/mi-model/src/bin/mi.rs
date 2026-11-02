use std::io::{Read, Write};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut text = String::new();
    let response = if args.len() != 2 || args[0] != "draft" {
        Err(
            serde_json::json!({"code":"invalid_request","message":"使用 mi draft read/validate/patch，JSON 通过 stdin 传入"}),
        )
    } else if let Err(e) = std::io::stdin()
        .take(4 * 1024 * 1024 + 1)
        .read_to_string(&mut text)
    {
        Err(serde_json::json!({"code":"invalid_request","message":e.to_string()}))
    } else {
        mi_model::storage::agent::execute(&args[1], &text)
            .map_err(|e| serde_json::to_value(e).unwrap())
    };
    let success = response.is_ok();
    let envelope = match response {
        Ok(result) => serde_json::json!({"api_version":1,"ok":true,"result":result}),
        Err(error) => serde_json::json!({"api_version":1,"ok":false,"error":error}),
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if serde_json::to_writer(&mut out, &envelope).is_err() || writeln!(out).is_err() {
        std::process::exit(2);
    }
    std::process::exit(if success { 0 } else { 2 });
}
