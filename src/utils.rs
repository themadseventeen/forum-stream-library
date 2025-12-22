use once_cell::sync::Lazy;
use reqwest::Client;
use serde::{Deserialize, Serialize};


#[derive(Serialize, Deserialize)]
pub struct InsertDiscordIdRequest {
    pub discord_message_id: u64,
    pub post_id: i64,
}

pub fn trim_to_n_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub async fn ntfy(message: &str, topic: &str) {
    static CLIENT: Lazy<Client> = Lazy::new(|| Client::new());
    let data = message.to_string();
    let _ = CLIENT
        .post(format!("https://ntfy.themadseventeen.xyz/{topic}"))
        .body(data)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .send()
        .await;
}

// removes all but top level fields
pub fn _skim_json(value: &mut serde_json::Value) {
    if let serde_json::Value::Object(obj) = value {
        let keys: Vec<String> = obj.keys().cloned().collect();
        for key in keys {
            if let Some(val) = obj.get_mut(&key) {
                match val {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        *val = serde_json::Value::Null; // Or use Value::Object(Map::new()) if preferred
                    }
                    _ => {}
                }
            }
        }
    }
}
