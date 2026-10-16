//! Grupo C — ai: config + extracción de leads.
//!
//! - Solo-autenticado (`Extension<Claims>`).
//! - `get_ai_config`/`get_extractions`: SQL puro, sin api_key en respuesta.
//! - `update_ai_config`: cifra api_key (AES-GCM); sin clave -> 503.
//! - `extract_lead`: lee config activa + mensajes, llama LLM openai-compatible
//!   POST `{model,messages}` por `state.http_client` (30 s), parsea JSON y
//!   guarda en `lead_extractions` (+ contacto y audit best-effort).
//! - Sin secretos en logs.

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::Claims,
    error::AppError,
    services::{
        crypto::{decrypt_from_hex, encrypt_to_hex, require_key},
        llm as llm_svc,
    },
    state::AppState,
};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct AIConfig {
    pub id: Uuid,
    pub provider: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub is_active: bool,
}

const AI_CONFIG_COLUMNS: &str = "id, provider, api_url, api_key, model, is_active";

#[derive(Debug, Deserialize, Validate)]
pub struct AIConfigInput {
    #[validate(length(max = 100))]
    pub provider: Option<String>,
    #[validate(length(max = 2000), url)]
    pub api_url: Option<String>,
    #[validate(length(max = 2000))]
    pub api_key: Option<String>,
    #[validate(length(max = 200))]
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AIConfigResponse {
    pub provider: String,
    pub api_url: String,
    pub model: String,
    pub is_active: bool,
}

#[derive(Debug, Serialize)]
pub struct LeadExtraction {
    pub id: Uuid,
    pub phone_number: String,
    pub contact_id: Option<Uuid>,
    pub extracted_data: serde_json::Value,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ExtractLeadInput {
    #[validate(length(min = 1, max = 50))]
    pub phone_number: String,
}

#[derive(Debug, Deserialize)]
pub struct ExtractionFilter {
    pub phone_number: Option<String>,
}

/// GET /api/v1/settings/ai (SQL puro; sin api_key en la respuesta).
pub async fn get_ai_config(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<AIConfigResponse>, AppError> {
    // TODO port: permiso `ai.use`.
    let sql = format!(
        "SELECT {AI_CONFIG_COLUMNS} FROM ai_config WHERE is_active = true ORDER BY created_at DESC LIMIT 1"
    );
    let config = sqlx::query_as::<_, AIConfig>(&sql)
        .fetch_optional(&state.db)
        .await?;

    match config {
        Some(c) => Ok(Json(AIConfigResponse {
            provider: c.provider,
            api_url: c.api_url,
            model: c.model,
            is_active: c.is_active,
        })),
        None => Ok(Json(AIConfigResponse {
            provider: "ollama".into(),
            api_url: "http://localhost:11434".into(),
            model: "llama3.2".into(),
            is_active: false,
        })),
    }
}

/// PUT /api/v1/settings/ai — cifra api_key (AES-GCM).
pub async fn update_ai_config(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Json(input): Json<AIConfigInput>,
) -> Result<Json<AIConfigResponse>, AppError> {
    // TODO port: permiso `ai.use`.
    input.validate()?;
    let provider = input
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(100).collect::<String>());
    let api_url = input
        .api_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let model = input
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(200).collect::<String>());
    let raw_key: Option<String> = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let enc_key: Option<String> = match raw_key {
        Some(raw) => {
            if raw.len() > 2000 {
                return Err(AppError::Validation("api_key inválida".into()));
            }
            let key = require_key(
                state.config.token_encryption_key.as_ref(),
                state.config.env.is_prod(),
            )?;
            Some(encrypt_to_hex(&key, &raw)?)
        }
        None => None,
    };

    let existing: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM ai_config ORDER BY created_at DESC LIMIT 1")
            .fetch_optional(&state.db)
            .await?;

    if let Some(id) = existing {
        sqlx::query(
            r#"
            UPDATE ai_config
            SET provider = COALESCE($2, provider),
                api_url = COALESCE($3, api_url),
                api_key = COALESCE($4, api_key),
                model = COALESCE($5, model),
                is_active = true,
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(&provider)
        .bind(&api_url)
        .bind(&enc_key)
        .bind(&model)
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query(
            r#"
            INSERT INTO ai_config (provider, api_url, api_key, model, is_active)
            VALUES ($1, $2, $3, $4, true)
            "#,
        )
        .bind(provider.unwrap_or_else(|| "ollama".into()))
        .bind(api_url.unwrap_or_else(|| "http://localhost:11434".into()))
        .bind(&enc_key)
        .bind(model.unwrap_or_else(|| "llama3.2".into()))
        .execute(&state.db)
        .await?;
    }

    let sql = format!("SELECT {AI_CONFIG_COLUMNS} FROM ai_config ORDER BY created_at DESC LIMIT 1");
    let config = sqlx::query_as::<_, AIConfig>(&sql)
        .fetch_one(&state.db)
        .await?;

    Ok(Json(AIConfigResponse {
        provider: config.provider,
        api_url: config.api_url,
        model: config.model,
        is_active: config.is_active,
    }))
}

/// GET /api/v1/leads/extractions?phone_number= (SQL puro).
pub async fn get_extractions(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Query(params): Query<ExtractionFilter>,
) -> Result<Json<Vec<LeadExtraction>>, AppError> {
    // TODO port: permiso `ai.use`.
    if let Some(ref pn) = params.phone_number
        && (pn.len() > 50 || pn.trim().is_empty())
    {
        return Err(AppError::Validation("phone_number inválido".into()));
    }
    let rows = sqlx::query_as::<_, LeadExtractionRow>(
        r#"
        SELECT id, phone_number, contact_id, extracted_data, status, created_at
        FROM lead_extractions
        WHERE ($1::text IS NULL OR phone_number = $1)
        ORDER BY created_at DESC
        LIMIT 20
        "#,
    )
    .bind(&params.phone_number)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| LeadExtraction {
                id: r.id,
                phone_number: r.phone_number,
                contact_id: r.contact_id,
                extracted_data: r.extracted_data,
                status: r.status,
                created_at: r.created_at,
            })
            .collect(),
    ))
}

/// POST /api/v1/leads/extract — LLM openai-compatible + guarda extracción.
pub async fn extract_lead(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(input): Json<ExtractLeadInput>,
) -> Result<Json<LeadExtraction>, AppError> {
    // TODO port: permiso `ai.use`.
    input.validate()?;
    let phone = input.phone_number.trim();
    if phone.is_empty() {
        return Err(AppError::Validation("phone_number requerido".into()));
    }

    let sql = format!(
        "SELECT {AI_CONFIG_COLUMNS} FROM ai_config WHERE is_active = true ORDER BY created_at DESC LIMIT 1"
    );
    let ai = sqlx::query_as::<_, AIConfig>(&sql)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let messages: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT content FROM whatsapp_messages
        WHERE from_number = $1 OR to_number = $1
        ORDER BY created_at ASC
        LIMIT 50
        "#,
    )
    .bind(phone)
    .fetch_all(&state.db)
    .await?;
    if messages.is_empty() {
        return Err(AppError::NotFound);
    }
    let conversation = messages.join("\n");
    if conversation.trim().is_empty() {
        return Err(AppError::NotFound);
    }

    // Descifra api_key si hay (sin clave -> 503).
    let decrypted_key: Option<String> = match ai.api_key.as_deref().filter(|s| !s.trim().is_empty())
    {
        Some(enc) => {
            if enc.contains(':') {
                let key = require_key(
                    state.config.token_encryption_key.as_ref(),
                    state.config.env.is_prod(),
                )?;
                Some(decrypt_from_hex(&key, enc).map_err(|_| {
                    tracing::error!("ai: api_key indescifrable");
                    AppError::Internal
                })?)
            } else {
                // Valor legacy en claro: úsalo sin loguearlo.
                Some(enc.to_string())
            }
        }
        None => None,
    };

    let system_prompt = r#"Eres un asistente de CRM. Extrae información de contacto de una conversación de WhatsApp y devuelve SOLO un JSON con estos campos:
{
  "first_name": "string",
  "last_name": "string",
  "email": "string | null",
  "phone": "string",
  "company": "string | null",
  "position": "string | null",
  "interest": "string | null",
  "notes": "string | null"
}
Usa null si no encuentras el dato. Responde ÚNICAMENTE con el JSON."#;

    let extracted = llm_svc::call_llm_openai_compatible(
        &state.http_client,
        &ai.api_url,
        decrypted_key.as_deref(),
        &ai.model,
        system_prompt,
        &conversation,
    )
    .await?;

    let extracted_data = parse_extraction_json(&extracted);
    let contact_id = try_create_contact(&state, &extracted_data, phone).await;

    let row = sqlx::query_as::<_, LeadExtractionRow>(
        r#"
        INSERT INTO lead_extractions (phone_number, contact_id, raw_messages, extracted_data, status)
        VALUES ($1, $2, $3, $4, 'completed')
        RETURNING id, phone_number, contact_id, extracted_data, status, created_at
        "#,
    )
    .bind(phone)
    .bind(contact_id)
    .bind(&conversation)
    .bind(&extracted_data)
    .fetch_one(&state.db)
    .await?;

    if let Some(cid) = contact_id {
        let _ = crate::handlers::audit::insert_audit_log(
            &state.db,
            Some(claims.sub),
            "lead_extracted",
            "contact",
            cid,
            None,
            Some(extracted_data.clone()),
        )
        .await;
    }

    Ok(Json(LeadExtraction {
        id: row.id,
        phone_number: row.phone_number,
        contact_id: row.contact_id,
        extracted_data: row.extracted_data,
        status: row.status,
        created_at: row.created_at,
    }))
}

fn parse_extraction_json(raw: &str) -> serde_json::Value {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim())
        && v.is_object()
    {
        return v;
    }
    // Intenta extraer el primer objeto JSON embebido.
    if let (Some(s), Some(e)) = (raw.find('{'), raw.rfind('}'))
        && e > s
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw[s..=e])
        && v.is_object()
    {
        return v;
    }
    serde_json::json!({ "raw": raw.chars().take(2000).collect::<String>(), "error": "failed to parse AI response" })
}

async fn try_create_contact(
    state: &AppState,
    data: &serde_json::Value,
    phone: &str,
) -> Option<Uuid> {
    let first_name = data["first_name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    if first_name.len() > 100 {
        return None;
    }
    let last_name = data["last_name"].as_str().map(str::trim).unwrap_or("");
    let email = data["email"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if email.is_some_and(|e| e.len() > 320) {
        return None;
    }
    let position = data["position"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let notes = data["notes"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO contacts (first_name, last_name, email, phone, position, notes)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT DO NOTHING
        RETURNING id
        "#,
    )
    .bind(first_name)
    .bind(last_name)
    .bind(email)
    .bind(phone)
    .bind(position)
    .bind(notes)
    .fetch_optional(&state.db)
    .await
    .ok()?
}

#[derive(Debug, sqlx::FromRow)]
struct LeadExtractionRow {
    id: Uuid,
    phone_number: String,
    contact_id: Option<Uuid>,
    extracted_data: serde_json::Value,
    status: String,
    created_at: chrono::DateTime<chrono::Utc>,
}
