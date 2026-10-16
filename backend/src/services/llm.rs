//! Cliente LLM openai-compatible FASE 2.
//!
//! - Usa el `reqwest::Client` compartido del estado (timeout 30 s).
//! - POST `{api_url}/v1/chat/completions` con `{model, messages}`.
//! - Sin secretos en logs.

use crate::error::AppError;

/// URL del endpoint chat/completions a partir del `api_url` configurado.
fn chat_url(api_url: &str) -> String {
    let t = api_url.trim_end_matches('/');
    if t.ends_with("/chat/completions") {
        t.to_string()
    } else if t.ends_with("/v1") {
        format!("{t}/chat/completions")
    } else {
        format!("{t}/v1/chat/completions")
    }
}

/// Llama al LLM (openai-compatible) y devuelve el contenido del primer choice.
pub async fn call_llm_openai_compatible(
    http: &reqwest::Client,
    api_url: &str,
    api_key: Option<&str>,
    model: &str,
    system: &str,
    user_content: &str,
) -> Result<String, AppError> {
    if api_url.trim().is_empty() {
        return Err(AppError::Unavailable);
    }
    if model.trim().is_empty() {
        return Err(AppError::Validation("modelo requerido".into()));
    }
    let url = chat_url(api_url);
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user_content}
        ],
        "temperature": 0.1,
    });
    let mut req = http
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body);
    if let Some(k) = api_key.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.header("Authorization", format!("Bearer {k}"));
    }
    let resp = req.send().await.map_err(|e| {
        tracing::error!("llm: fallo de red");
        let _ = e;
        AppError::Internal
    })?;
    let status = resp.status();
    if !status.is_success() {
        tracing::error!("llm: upstream status={}", status.as_u16());
        return Err(AppError::Internal);
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| {
        tracing::error!("llm: respuesta no-JSON");
        let _ = e;
        AppError::Internal
    })?;
    Ok(data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("{}")
        .to_string())
}
