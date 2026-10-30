//! Grupo C — email: plantillas y registro de envíos.
//!
//! - Solo-autenticado (`Extension<Claims>`).
//! - `send_email` y `send_from_template` validan el input, envían por SMTP
//!   (`services::email`, respeta `EMAIL_ENABLED=false` → 503) y registran el
//!   resultado en `email_logs` (`sent`/`failed`).

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::Claims,
    error::AppError,
    middleware::auth_ext::UserPermissions,
    models::{
        CreateEmailTemplate, EmailLog, EmailTemplate, ListQuery, SendEmail, UpdateEmailTemplate,
        normalize_list,
    },
    services::email::{EmailMessage, send_via_smtp},
    state::AppState,
};

fn is_valid_email(addr: &str) -> bool {
    let parts: Vec<&str> = addr.split('@').collect();
    if parts.len() != 2 {
        return false;
    }
    let (local, domain) = (parts[0].trim(), parts[1].trim());
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    if local.len() > 64 || addr.len() > 254 {
        return false;
    }
    domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn validate_email_list(field: &str, value: Option<&str>) -> Result<(), AppError> {
    if let Some(list) = value.map(str::trim).filter(|s| !s.is_empty()) {
        for addr in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if !is_valid_email(addr) {
                return Err(AppError::Validation(format!(
                    "email {field} inválido: {addr}"
                )));
            }
        }
    }
    Ok(())
}

/// Sustituye `{{vars}}` en `text` con el mapa dado (`{{to}}`, `{{from}}`, …).
/// Las claves desconocidas se dejan intactas (sin pánico ni fuga de datos).
fn render_template(text: &str, vars: &[(&str, &str)]) -> String {
    let mut out = text.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

const LOG_COLUMNS: &str = "id, from_email, to_email, cc, bcc, subject, body, body_html, \
    entity_type, entity_id, status, template_id, sent_by, sent_at, created_at";

/// Envía por SMTP y registra en `email_logs` (`sent`; `failed` + 500 si el
/// SMTP falla). `EMAIL_ENABLED=false` → 503 sin tocar la red.
/// Parámetros del envío (agrupados para no exceder el tope de aridad).
struct DeliverParams<'a> {
    from_email: &'a str,
    input: &'a SendEmail,
    subject: &'a str,
    body: &'a str,
    body_html: Option<&'a str>,
    template_id: Option<Uuid>,
    sent_by: Uuid,
}

async fn deliver(state: &AppState, p: DeliverParams<'_>) -> Result<EmailLog, AppError> {
    let input = p.input;
    let msg = EmailMessage {
        from: Some(p.from_email.to_string()),
        to: input.to.clone(),
        cc: input.cc.clone(),
        bcc: input.bcc.clone(),
        subject: p.subject.to_string(),
        body_text: p.body.to_string(),
        body_html: p.body_html.map(str::to_string),
    };
    let send_result = send_via_smtp(&state.smtp, &msg).await;
    // `Unavailable` (SMTP deshabilitado) se propaga como 503 sin registrar.
    if matches!(send_result, Err(AppError::Unavailable)) {
        return Err(AppError::Unavailable);
    }
    let (status, sent_at_clause) = match send_result {
        Ok(()) => ("sent", "NOW()"),
        Err(_) => ("failed", "NULL"),
    };
    if status == "failed" {
        tracing::error!("smtp: envío a destinatario fallido (ver logs del servicio)");
    }
    let sql = format!(
        "INSERT INTO email_logs (from_email, to_email, cc, bcc, subject, body, body_html, \
            entity_type, entity_id, status, template_id, sent_by, sent_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, {sent_at_clause}) \
         RETURNING {LOG_COLUMNS}"
    );
    let log = sqlx::query_as::<_, EmailLog>(&sql)
        .bind(p.from_email)
        .bind(&input.to)
        .bind(input.cc.as_deref())
        .bind(input.bcc.as_deref())
        .bind(p.subject)
        .bind(p.body)
        .bind(p.body_html)
        .bind(input.entity_type.as_deref())
        .bind(input.entity_id)
        .bind(status)
        .bind(p.template_id)
        .bind(p.sent_by)
        .fetch_one(&state.db)
        .await?;
    if status == "failed" {
        return Err(AppError::Internal);
    }
    Ok(log)
}

/// POST /api/v1/email/send — valida, envía por SMTP y registra en `email_logs`.
pub async fn send_email(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Json(input): Json<SendEmail>,
) -> Result<Json<EmailLog>, AppError> {
    perms
        .require("email.send")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    validate_email_list("cc", input.cc.as_deref())?;
    validate_email_list("bcc", input.bcc.as_deref())?;
    // Sin SMTP no hay envío posible: 503 antes de validar el remitente
    // por defecto (SMTP_FROM vacío en dev).
    if !state.smtp.enabled {
        return Err(AppError::Unavailable);
    }
    let from_email = input
        .from
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(state.smtp.from.trim())
        .to_string();
    if from_email.is_empty() || !is_valid_email(&from_email) {
        return Err(AppError::Validation("email `from` inválido".into()));
    }
    let vars = [("to", input.to.trim()), ("from", from_email.trim())];
    let subject = render_template(input.subject.trim(), &vars);
    let body = render_template(input.body.trim(), &vars);
    let body_html = input
        .body_html
        .as_deref()
        .map(|h| render_template(h, &vars));
    let log = deliver(
        &state,
        DeliverParams {
            from_email: &from_email,
            input: &input,
            subject: &subject,
            body: &body,
            body_html: body_html.as_deref(),
            template_id: input.template_id,
            sent_by: claims.sub,
        },
    )
    .await?;
    Ok(Json(log))
}

/// GET /api/v1/email/logs — paginado estilo destino (`data/page/per_page/total`).
pub async fn list_email_logs(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, AppError> {
    perms
        .require("email.view")
        .map_err(|_| AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let (page, per_page, _sort, _dir) = normalize_list(&q);
    let offset = (page - 1) * per_page;

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM email_logs el LEFT JOIN users u ON u.id = el.sent_by \
         WHERE ($1::uuid IS NULL OR u.tenant_id = $1)",
    )
    .bind(scope)
    .fetch_one(&state.db)
    .await?;
    let logs = sqlx::query_as::<_, EmailLog>(
        "SELECT el.id, el.from_email, el.to_email, el.cc, el.bcc, el.subject, el.body, el.body_html, el.entity_type, el.entity_id, el.status, el.template_id, el.sent_by, el.sent_at, el.created_at
         FROM email_logs el LEFT JOIN users u ON u.id = el.sent_by
         WHERE ($1::uuid IS NULL OR u.tenant_id = $1)
         ORDER BY el.created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(scope)
    .bind(per_page)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        json!({ "data": logs, "page": page, "per_page": per_page, "total": total }),
    ))
}

/// GET /api/v1/email/logs/{id}.
pub async fn get_email_log(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<Json<EmailLog>, AppError> {
    perms
        .require("email.view")
        .map_err(|_| AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let log = sqlx::query_as::<_, EmailLog>(
        "SELECT el.id, el.from_email, el.to_email, el.cc, el.bcc, el.subject, el.body, el.body_html, el.entity_type, el.entity_id, el.status, el.template_id, el.sent_by, el.sent_at, el.created_at
         FROM email_logs el LEFT JOIN users u ON u.id = el.sent_by
         WHERE el.id = $1 AND ($2::uuid IS NULL OR u.tenant_id = $2)",
    )
    .bind(id)
    .bind(scope)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(log))
}

/// GET /api/v1/email/templates.
pub async fn list_templates(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Vec<EmailTemplate>>, AppError> {
    perms
        .require("email.view")
        .map_err(|_| AppError::Forbidden)?;
    let templates = sqlx::query_as::<_, EmailTemplate>(
        "SELECT id, name, subject, body, body_html, category, created_by, created_at, updated_at
         FROM email_templates ORDER BY name",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(templates))
}

/// POST /api/v1/email/templates.
pub async fn create_template(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Json(input): Json<CreateEmailTemplate>,
) -> Result<(StatusCode, Json<EmailTemplate>), AppError> {
    perms
        .require("email.send")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;

    let template = sqlx::query_as::<_, EmailTemplate>(
        "INSERT INTO email_templates (name, subject, body, body_html, category, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, name, subject, body, body_html, category, created_by, created_at, updated_at",
    )
    .bind(&input.name)
    .bind(&input.subject)
    .bind(&input.body)
    .bind(&input.body_html)
    .bind(input.category.unwrap_or_else(|| "general".into()))
    .bind(claims.sub)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(template)))
}

/// PUT /api/v1/email/templates/{id}.
pub async fn update_template(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateEmailTemplate>,
) -> Result<Json<EmailTemplate>, AppError> {
    perms
        .require("email.send")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let template = sqlx::query_as::<_, EmailTemplate>(
        "UPDATE email_templates SET
            name = COALESCE($2, name),
            subject = COALESCE($3, subject),
            body = COALESCE($4, body),
            body_html = COALESCE($5, body_html),
            category = COALESCE($6, category),
            updated_at = NOW()
         WHERE id = $1
         RETURNING id, name, subject, body, body_html, category, created_by, created_at, updated_at",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.subject)
    .bind(&input.body)
    .bind(&input.body_html)
    .bind(&input.category)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(template))
}

/// DELETE /api/v1/email/templates/{id}.
pub async fn delete_template(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    perms
        .require("email.send")
        .map_err(|_| AppError::Forbidden)?;
    let result = sqlx::query("DELETE FROM email_templates WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/email/send-from-template/{template_id} — renderiza la
/// plantilla (sustituye `{{to}}`/`{{from}}`), envía por SMTP y registra.
pub async fn send_from_template(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(template_id): Path<Uuid>,
    Json(input): Json<SendEmail>,
) -> Result<Json<EmailLog>, AppError> {
    perms
        .require("email.send")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    validate_email_list("cc", input.cc.as_deref())?;
    validate_email_list("bcc", input.bcc.as_deref())?;
    // Sin SMTP no hay envío posible: 503 antes de validar el remitente
    // por defecto (SMTP_FROM vacío en dev).
    if !state.smtp.enabled {
        return Err(AppError::Unavailable);
    }

    let template = sqlx::query_as::<_, EmailTemplate>(
        "SELECT id, name, subject, body, body_html, category, created_by, created_at, updated_at
         FROM email_templates WHERE id = $1",
    )
    .bind(template_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let from_email = input
        .from
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(state.smtp.from.trim())
        .to_string();
    if from_email.is_empty() || !is_valid_email(&from_email) {
        return Err(AppError::Validation("email `from` inválido".into()));
    }
    let vars = [("to", input.to.trim()), ("from", from_email.trim())];
    // El input manda si trae contenido; si no, la plantilla. En ambos casos
    // se sustituyen las `{{vars}}`.
    let base_subject = if input.subject.trim().is_empty() {
        template.subject.as_str()
    } else {
        input.subject.trim()
    };
    let base_body = if input.body.trim().is_empty() {
        template.body.as_str()
    } else {
        input.body.trim()
    };
    let base_html: Option<String> = input
        .body_html
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or(template.body_html.clone());
    let subject = render_template(base_subject, &vars);
    let body = render_template(base_body, &vars);
    let body_html = base_html.as_deref().map(|h| render_template(h, &vars));
    let log = deliver(
        &state,
        DeliverParams {
            from_email: &from_email,
            input: &input,
            subject: &subject,
            body: &body,
            body_html: body_html.as_deref(),
            template_id: Some(template.id),
            sent_by: claims.sub,
        },
    )
    .await?;
    Ok(Json(log))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_sustituye_vars_y_respeta_desconocidas() {
        let out = render_template(
            "Hola {{to}} de {{from}}, {{otro}} queda",
            &[("to", "a@x.com"), ("from", "b@x.com")],
        );
        assert_eq!(out, "Hola a@x.com de b@x.com, {{otro}} queda");
    }

    #[test]
    fn validacion_de_emails_auxiliar() {
        assert!(is_valid_email("a@b.com"));
        assert!(!is_valid_email("sin-arroba"));
        assert!(!is_valid_email("a@sinpunto"));
        assert!(validate_email_list("cc", Some("a@b.com, c@d.org")).is_ok());
        assert!(validate_email_list("cc", Some("mal")).is_err());
        assert!(validate_email_list("bcc", None).is_ok());
    }
}
