use serde_json::Value;

const PLACEHOLDER_ICON_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

pub fn handle_v2_get_client_icon(body: &str) -> String {
    let id = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());

    let result_json = serde_json::json!({
        "id": id,
        "result": { "icon": PLACEHOLDER_ICON_BASE64 }
    });

    format!("V2/GET_CLIENT_ICON\n{result_json}\n")
}
