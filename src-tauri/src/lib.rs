use keyring::{Entry, Error as KeyringError};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const PROVIDER_SERVICE: &str = "BOSCode AI Providers";

#[derive(Serialize)]
struct ProviderTestResult {
    provider: String,
    model: String,
    status: u16,
    latency_ms: u64,
    message: String,
}

fn validate_provider_id(provider_id: &str) -> Result<(), String> {
    let valid = !provider_id.is_empty()
        && provider_id.len() <= 80
        && provider_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'));

    if valid {
        Ok(())
    } else {
        Err("Invalid provider identifier.".to_string())
    }
}

fn provider_entry(provider_id: &str) -> Result<Entry, String> {
    validate_provider_id(provider_id)?;
    Entry::new(PROVIDER_SERVICE, provider_id)
        .map_err(|error| format!("Unable to open the secure credential store: {error}"))
}

fn provider_error_message(body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();

    if let Some(message) = parsed
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
    {
        return message.chars().take(240).collect();
    }

    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        "The provider did not return an error message.".to_string()
    } else {
        compact.chars().take(240).collect()
    }
}

#[tauri::command]
fn app_info() -> String {
    format!("BOSCode {}", env!("CARGO_PKG_VERSION"))
}

#[tauri::command]
fn save_provider_secret(provider_id: String, secret: String) -> Result<(), String> {
    let secret = secret.trim();

    if secret.is_empty() {
        return Err("API key cannot be empty.".to_string());
    }

    provider_entry(&provider_id)?
        .set_password(secret)
        .map_err(|error| format!("Unable to store the API key securely: {error}"))
}

#[tauri::command]
fn provider_secret_exists(provider_id: String) -> Result<bool, String> {
    match provider_entry(&provider_id)?.get_password() {
        Ok(_) => Ok(true),
        Err(KeyringError::NoEntry) => Ok(false),
        Err(error) => Err(format!("Unable to read the secure credential store: {error}")),
    }
}

#[tauri::command]
fn delete_provider_secret(provider_id: String) -> Result<(), String> {
    match provider_entry(&provider_id)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(format!("Unable to remove the API key: {error}")),
    }
}

#[tauri::command]
async fn test_provider_connection(
    provider_id: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
) -> Result<ProviderTestResult, String> {
    validate_provider_id(&provider_id)?;

    let base_url = base_url.trim().trim_end_matches('/');
    let model = model.trim();

    if base_url.is_empty() {
        return Err("API Base URL is required.".to_string());
    }

    if model.is_empty() {
        return Err("Model ID is required.".to_string());
    }

    let endpoint = format!("{base_url}/chat/completions");
    let parsed_url = reqwest::Url::parse(&endpoint)
        .map_err(|_| "API Base URL is not a valid URL.".to_string())?;

    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err("API Base URL must use http or https.".to_string());
    }

    let provided_secret = api_key
        .as_deref()
        .map(str::trim)
        .filter(|secret| !secret.is_empty())
        .map(ToOwned::to_owned);

    let stored_secret = if provided_secret.is_none() {
        match provider_entry(&provider_id)?.get_password() {
            Ok(secret) => Some(secret),
            Err(KeyringError::NoEntry) => None,
            Err(error) => {
                return Err(format!(
                    "Unable to read the secure credential store: {error}"
                ))
            }
        }
    } else {
        None
    };

    let secret = provided_secret.or(stored_secret);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|error| format!("Unable to initialize the HTTP client: {error}"))?;

    let payload = json!({
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": "Reply only with OK."
            }
        ],
        "max_tokens": 4,
        "stream": false
    });

    let mut request = client.post(parsed_url).json(&payload);
    if let Some(secret) = secret {
        request = request.bearer_auth(secret);
    }

    let started = Instant::now();
    let response = request
        .send()
        .await
        .map_err(|error| format!("Could not reach the provider: {error}"))?;

    let status = response.status();
    let status_code = status.as_u16();
    let body = response.text().await.unwrap_or_default();
    let latency_ms = started.elapsed().as_millis() as u64;

    if !status.is_success() {
        return Err(format!(
            "Provider returned HTTP {status_code}: {}",
            provider_error_message(&body)
        ));
    }

    Ok(ProviderTestResult {
        provider: provider_id,
        model: model.to_string(),
        status: status_code,
        latency_ms,
        message: "Connection verified.".to_string(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_info,
            save_provider_secret,
            provider_secret_exists,
            delete_provider_secret,
            test_provider_connection
        ])
        .run(tauri::generate_context!())
        .expect("error while running BOSCode");
}
