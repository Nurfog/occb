//! Derechos de titulares Ley 21.719 (Chile): acceso, rectificación, supresión,
//! oposición, portabilidad y bloqueo.
//!
//! - Públicos (sin JWT): `create_request`, `verify_request`. El token de
//!   verificación es opaco (reutiliza `crate::auth::{new_refresh_token,
//!   hash_refresh_token}`); en DB solo su SHA-256. NUNCA se devuelve al cliente.
//! - Staff: `list_requests`, `get_request` (permiso `privacy.view`);
//!   `update_request` (permiso `privacy.manage`).
//! - SQL 100% parametrizado, LIKE con `escape_like` + `ESCAPE '\'`, ORDER BY de
//!   lista blanca, LIMIT/OFFSET i64 bindeados, sin `SELECT *`.
//! - Auditoría y emails best-effort (se ignoran errores de SMTP/audit).
//!
//! NOTA PARA EL INTEGRADOR (search/export): el bloqueo (`contacts.blocked =
//! TRUE` por oposición/bloqueo legal) significa "no tratar ni ceder". Los
//! endpoints de búsqueda/exportación (`handlers::search`, exports CSV de
//! contactos) DEBEN excluir bloqueados con
//! `AND (blocked IS NULL OR blocked = FALSE)` en sus WHERE. Este módulo no
//! toca `search.rs` ni exports (archivos congelados); ese filtro es TODO del
//! integrador al cablear rutas.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{DateTime, Datelike, Utc, Weekday};
use serde_json::{Value, json};
use sqlx::Postgres;
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::{Claims, hash_refresh_token, new_refresh_token},
    error::AppError,
    handlers::audit::insert_audit_log,
    middleware::auth_ext::UserPermissions,
    models::{
        escape_like,
        privacy::{
            ACCIONES_VALIDAS, CreatePrivacyRequest, ESTADOS_VALIDOS, PrivacyQuery, PrivacyRequest,
            PrivacyRequestListItem, RECTIFICACION_WHITELIST, TIPOS_VALIDOS, UpdatePrivacyRequest,
            VerifyPrivacyRequest,
        },
    },
    services::email::{EmailMessage, send_via_smtp},
    state::AppState,
};

const PRIVACY_COLUMNS: &str = "id, contact_id, email, type, status, motivo, \
    identity_verified, verify_token_hash, verify_expires_at, due_at, extended, \
    extended_at, corrections, legal_hold, resolution, resolved_at, created_at, updated_at";

const PIE_RECLAMO_AGENCIA: &str = "Si no estás de acuerdo con esta resolución, puedes reclamar ante la \
    Agencia de Protección de Datos en 30 días hábiles (art. 41 Ley 21.719).";

// ─── Helpers puros (testeables sin DB) ─────────────────────────────────────

/// Máquina de estados estricta. Toda transición no listada -> 422.
fn puede_transitar(de: &str, accion: &str) -> bool {
    matches!(
        (de, accion),
        ("recibida", "verificar")
            | ("verificada", "admitir")
            | ("verificada", "prorrogar")
            | ("en_curso", "prorrogar")
            | ("en_curso", "resolver-cumplida")
            | ("en_curso", "resolver-denegada")
            | ("recibida", "desistir")
            | ("verificada", "desistir")
            | ("en_curso", "desistir")
            | ("cumplida", "levantar_bloqueo")
            | ("denegada", "levantar_bloqueo")
    )
}

/// Plazo general: +30 días corridos.
fn due_normal(now: DateTime<Utc>) -> DateTime<Utc> {
    now + chrono::Duration::days(30)
}

/// Bloqueo: +2 días hábiles (salta sáb/dom).
fn due_bloqueo(now: DateTime<Utc>) -> DateTime<Utc> {
    let mut d = now;
    let mut habiles = 0;
    while habiles < 2 {
        d += chrono::Duration::days(1);
        match d.weekday() {
            Weekday::Sat | Weekday::Sun => {}
            _ => habiles += 1,
        }
    }
    d
}

fn calcular_due_at(tipo: &str, now: DateTime<Utc>) -> DateTime<Utc> {
    if tipo == "bloqueo" {
        due_bloqueo(now)
    } else {
        due_normal(now)
    }
}

fn email_valido(s: &str) -> bool {
    let partes: Vec<&str> = s.split('@').collect();
    if partes.len() != 2 {
        return false;
    }
    let (local, dominio) = (partes[0].trim(), partes[1].trim());
    if local.is_empty() || dominio.is_empty() || s.len() > 320 || local.len() > 64 {
        return false;
    }
    dominio.contains('.') && !dominio.starts_with('.') && !dominio.ends_with('.')
}

/// Compara el token presentado con el hash guardado (SHA-256 hex).
fn hash_igual(token_presentado: &str, hash_guardado: &str) -> bool {
    hash_refresh_token(token_presentado.trim()) == hash_guardado
}

/// Valida `corrections` para rectificación: objeto, solo claves de la lista
/// blanca, strings acotados, email con formato. `con_bloqueo` se ignora (es
/// metadata del formulario, no columna). Devuelve pares (columna, valor).
fn extraer_corrections(v: &Value) -> Result<Vec<(String, String)>, AppError> {
    let obj = v
        .as_object()
        .ok_or_else(|| AppError::Validation("corrections debe ser un objeto JSON".into()))?;
    let mut pares = Vec::new();
    for (k, val) in obj {
        if k == "con_bloqueo" {
            continue;
        }
        if !RECTIFICACION_WHITELIST.contains(&k.as_str()) {
            return Err(AppError::Validation(format!("clave no permitida: {k}")));
        }
        let s = val
            .as_str()
            .ok_or_else(|| AppError::Validation(format!("valor no textual para clave: {k}")))?;
        let s = s.trim().to_string();
        match k.as_str() {
            "first_name" | "last_name" => {
                if s.is_empty() || s.len() > 100 {
                    return Err(AppError::Validation(format!("{k} de 1 a 100 caracteres")));
                }
            }
            "email" => {
                if !email_valido(&s) {
                    return Err(AppError::Validation("email inválido en corrections".into()));
                }
            }
            "phone" => {
                if s.len() > 50 {
                    return Err(AppError::Validation("phone max 50 caracteres".into()));
                }
            }
            "position" => {
                if s.len() > 100 {
                    return Err(AppError::Validation("position max 100 caracteres".into()));
                }
            }
            "notes" => {
                if s.len() > 1000 {
                    return Err(AppError::Validation("notes max 1000 caracteres".into()));
                }
            }
            _ => {
                return Err(AppError::Validation(format!("clave no permitida: {k}")));
            }
        }
        pares.push((k.clone(), s));
    }
    if pares.is_empty() {
        return Err(AppError::Validation(
            "corrections vacío (sin claves rectificables)".into(),
        ));
    }
    Ok(pares)
}

fn best_effort_email(to: &str, subject: &str, body: &str) -> EmailMessage {
    EmailMessage {
        from: None,
        to: to.to_string(),
        cc: None,
        bcc: None,
        subject: subject.to_string(),
        body_text: body.to_string(),
        body_html: None,
    }
}

// ─── 1. Crear solicitud (PÚBLICO) ───────────────────────────────────────────

/// POST público: crea la solicitud, genera token de verificación (solo hash en
/// DB, expira en 7 días) y envía acuse best-effort. Responde 202 sin el token.
pub async fn create_request(
    State(state): State<AppState>,
    Json(input): Json<CreatePrivacyRequest>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    let email = input.email.trim().to_lowercase();
    if email.is_empty() || email.len() > 320 || !email_valido(&email) {
        return Err(AppError::Validation("email inválido".into()));
    }
    let tipo = input.request_type.trim().to_lowercase();
    if !TIPOS_VALIDOS.contains(&tipo.as_str()) {
        return Err(AppError::Validation("tipo de solicitud inválido".into()));
    }
    let motivo = input
        .motivo
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // Vinculación automática por email: el solicitante normalmente solo conoce
    // su email, no el UUID interno. Sin vínculo no hay nada que ejecutar
    // (supresión/bloqueo serían no-op), así que se resuelve aquí.
    let linked_contact: Option<Uuid> = if let Some(cid) = input.contact_id {
        let fila: Option<Option<String>> =
            sqlx::query_scalar("SELECT email::text FROM contacts WHERE id = $1")
                .bind(cid)
                .fetch_optional(&state.db)
                .await?;
        let Some(cemail) = fila else {
            return Err(AppError::Validation("contacto no existe".into()));
        };
        if let Some(ce) = cemail
            && ce.trim().to_lowercase() != email
        {
            return Err(AppError::Validation(
                "el email no coincide con el contacto".into(),
            ));
        }
        Some(cid)
    } else {
        sqlx::query_scalar("SELECT id FROM contacts WHERE lower(email) = lower($1) LIMIT 1")
            .bind(&email)
            .fetch_optional(&state.db)
            .await?
    };

    let now = Utc::now();
    let due_at = calcular_due_at(&tipo, now);
    let token = new_refresh_token();
    let token_hash = hash_refresh_token(&token);
    let token_exp = now + chrono::Duration::days(7);
    if let Some(ref c) = input.corrections
        && c.to_string().len() > 20_000
    {
        return Err(AppError::Validation("corrections demasiado grande".into()));
    }

    let fila: (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO privacy_requests (contact_id, email, type, status, motivo, \
            verify_token_hash, verify_expires_at, due_at, corrections) \
         VALUES ($1, $2, $3::privacy_request_type, 'recibida'::privacy_request_status, \
            $4, $5, $6, $7, $8) RETURNING id, due_at",
    )
    .bind(linked_contact)
    .bind(&email)
    .bind(&tipo)
    .bind(motivo.as_deref())
    .bind(&token_hash)
    .bind(token_exp)
    .bind(due_at)
    .bind(input.corrections.clone())
    .fetch_one(&state.db)
    .await?;
    let (id, due_at_db) = fila;

    let msg = best_effort_email(
        &email,
        "Solicitud recibida – Ley 21.719",
        &format!(
            "Hemos recibido tu solicitud de derechos (Ley 21.719).\n\nFolio: {id}\nTipo: {tipo}\n\
             Plazo de respuesta: 30 días corridos.\n\nTe contactaremos a este correo para \
             verificar tu identidad. Si no reconoces esta solicitud, ignora este mensaje."
        ),
    );
    let _ = send_via_smtp(&state.smtp, &msg).await;
    let _ = insert_audit_log(
        &state.db,
        None,
        "create",
        "privacy_request",
        id,
        None,
        Some(json!({ "email": email, "type": tipo, "con_bloqueo": input.con_bloqueo })),
    )
    .await;

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "id": id, "status": "recibida", "due_at": due_at_db })),
    ))
}

// ─── 2. Verificar identidad (PÚBLICO) ───────────────────────────────────────

/// POST público: verifica el token opaco. Mismo 422 genérico si la solicitud no
/// existe, el token es malo/ausente o expiró (no filtrar existencia).
pub async fn verify_request(
    State(state): State<AppState>,
    Json(input): Json<VerifyPrivacyRequest>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    let generico = || AppError::Validation("código inválido o expirado".into());
    let sql = format!("SELECT {PRIVACY_COLUMNS} FROM privacy_requests WHERE id = $1");
    let req = sqlx::query_as::<_, PrivacyRequest>(&sql)
        .bind(input.request_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(generico)?;
    if !matches!(
        req.status,
        crate::models::privacy::PrivacyRequestStatus::Recibida
    ) {
        return Err(AppError::Validation(
            "la solicitud no está en estado recibida".into(),
        ));
    }
    let ok = match (req.verify_token_hash.as_deref(), req.verify_expires_at) {
        (Some(guardado), Some(exp)) => hash_igual(&input.token, guardado) && exp > Utc::now(),
        _ => false,
    };
    if !ok {
        return Err(generico());
    }

    let mut tx = state.db.begin().await?;
    let n = sqlx::query(
        "UPDATE privacy_requests SET identity_verified = TRUE, \
            status = 'verificada'::privacy_request_status, verify_token_hash = NULL, \
            updated_at = NOW() \
         WHERE id = $1 AND status = 'recibida'::privacy_request_status",
    )
    .bind(req.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::Validation(
            "la solicitud no está en estado recibida".into(),
        ));
    }
    if matches!(
        req.r#type,
        crate::models::privacy::PrivacyRequestType::Bloqueo
    ) && let Some(cid) = req.contact_id
    {
        sqlx::query("UPDATE contacts SET blocked = TRUE WHERE id = $1")
            .bind(cid)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    let _ = insert_audit_log(
        &state.db,
        None,
        "verify",
        "privacy_request",
        req.id,
        None,
        Some(json!({ "status": "verificada" })),
    )
    .await;
    Ok(Json(json!({ "status": "verificada" })))
}

// ─── 3. Listar (staff view) ─────────────────────────────────────────────────

/// GET staff: paginado con sort de lista blanca y `overdue` calculado en SQL.
pub async fn list_requests(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Query(params): Query<PrivacyQuery>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("privacy.view")
        .map_err(|_| AppError::Forbidden)?;
    params.validate()?;
    let page = params.page_clamped();
    let per_page = params.per_page_clamped();
    let offset = params.offset();
    let sort_col = params.sort_column();
    let sort_dir = params.sort_direction();

    let status = params
        .status
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    if let Some(ref s) = status
        && !ESTADOS_VALIDOS.contains(&s.as_str())
    {
        return Err(AppError::Validation("estado inválido".into()));
    }
    let tipo = params
        .request_type
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    if let Some(ref t) = tipo
        && !TIPOS_VALIDOS.contains(&t.as_str())
    {
        return Err(AppError::Validation("tipo inválido".into()));
    }
    let pat_email: Option<String> = params
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", escape_like(s)));
    let pat_q: Option<String> = params
        .q
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", escape_like(s)));

    let overdue_sql = "(due_at < NOW() AND status IN \
        ('recibida'::privacy_request_status, 'verificada'::privacy_request_status, \
          'en_curso'::privacy_request_status))";
    let count_sql = format!(
        "SELECT COUNT(*) FROM privacy_requests \
         WHERE ($1::privacy_request_status IS NULL OR status = $1::privacy_request_status) \
           AND ($2::privacy_request_type IS NULL OR type = $2::privacy_request_type) \
           AND ($3::text IS NULL OR email::text ILIKE $3 ESCAPE '\\') \
           AND ($4::text IS NULL OR email::text ILIKE $4 ESCAPE '\\') \
           AND ($5::bool IS NULL OR ({overdue_sql} = $5))"
    );
    let data_sql = format!(
        "SELECT id, contact_id, email::text AS email, type, status, motivo, \
            identity_verified, due_at, extended, created_at, ({overdue_sql}) AS overdue \
         FROM privacy_requests \
         WHERE ($1::privacy_request_status IS NULL OR status = $1::privacy_request_status) \
           AND ($2::privacy_request_type IS NULL OR type = $2::privacy_request_type) \
           AND ($3::text IS NULL OR email::text ILIKE $3 ESCAPE '\\') \
           AND ($4::text IS NULL OR email::text ILIKE $4 ESCAPE '\\') \
           AND ($5::bool IS NULL OR ({overdue_sql} = $5)) \
         ORDER BY {sort_col} {sort_dir} LIMIT $6 OFFSET $7"
    );

    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(status.as_deref())
        .bind(tipo.as_deref())
        .bind(pat_email.as_deref())
        .bind(pat_q.as_deref())
        .bind(params.overdue)
        .fetch_one(&state.db)
        .await?;
    let items = sqlx::query_as::<_, PrivacyRequestListItem>(&data_sql)
        .bind(status.as_deref())
        .bind(tipo.as_deref())
        .bind(pat_email.as_deref())
        .bind(pat_q.as_deref())
        .bind(params.overdue)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

    Ok(Json(
        json!({ "data": items, "page": page, "per_page": per_page, "total": total }),
    ))
}

// ─── 4. Detalle (staff view) ────────────────────────────────────────────────

/// GET staff: 404 si no existe. El hash del token nunca sale al cliente.
pub async fn get_request(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("privacy.view")
        .map_err(|_| AppError::Forbidden)?;
    let sql = format!("SELECT {PRIVACY_COLUMNS} FROM privacy_requests WHERE id = $1");
    let req = sqlx::query_as::<_, PrivacyRequest>(&sql)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let mut out = req;
    out.verify_token_hash = None;
    Ok(Json(out))
}

// ─── 5. Transiciones (staff manage) ─────────────────────────────────────────

/// PATCH staff: máquina de estados estricta (transición inválida -> 422).
/// Auditoría en cada transición con `actor = claims.sub`.
pub async fn update_request(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdatePrivacyRequest>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("privacy.manage")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let accion = input.accion.trim().to_lowercase();
    if !ACCIONES_VALIDAS.contains(&accion.as_str()) {
        return Err(AppError::Validation("acción inválida".into()));
    }

    let mut tx = state.db.begin().await?;
    let sql = format!("SELECT {PRIVACY_COLUMNS} FROM privacy_requests WHERE id = $1 FOR UPDATE");
    let req = sqlx::query_as::<_, PrivacyRequest>(&sql)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    let de = req.status.as_str().to_string();
    if !puede_transitar(&de, &accion) {
        return Err(AppError::Validation(format!(
            "transición inválida: {de} -> {accion}"
        )));
    }
    let actor = claims.sub;
    let email_to = req.email.clone();
    let tipo = req.r#type.as_str().to_string();

    match accion.as_str() {
        "verificar" => {
            sqlx::query(
                "UPDATE privacy_requests SET identity_verified = TRUE, \
                    status = 'verificada'::privacy_request_status, verify_token_hash = NULL, \
                    updated_at = NOW() WHERE id = $1",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
            if tipo == "bloqueo"
                && let Some(cid) = req.contact_id
            {
                sqlx::query("UPDATE contacts SET blocked = TRUE WHERE id = $1")
                    .bind(cid)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            let _ = insert_audit_log(
                &state.db,
                Some(actor),
                "verify",
                "privacy_request",
                id,
                Some(json!({ "from": de })),
                Some(json!({ "status": "verificada" })),
            )
            .await;
            Ok(Json(json!({ "id": id, "status": "verificada" })))
        }
        "admitir" => {
            sqlx::query(
                "UPDATE privacy_requests SET status = 'en_curso'::privacy_request_status, \
                    updated_at = NOW() WHERE id = $1",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
            if tipo == "bloqueo"
                && let Some(cid) = req.contact_id
            {
                sqlx::query("UPDATE contacts SET blocked = TRUE WHERE id = $1")
                    .bind(cid)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            let msg = best_effort_email(
                &email_to,
                "Solicitud admitida a trámite – Ley 21.719",
                &format!(
                    "Tu solicitud (folio {id}, tipo {tipo}) fue admitida a trámite y está en curso. \
                     Te notificaremos su resolución antes del vencimiento del plazo legal."
                ),
            );
            let _ = send_via_smtp(&state.smtp, &msg).await;
            let _ = insert_audit_log(
                &state.db,
                Some(actor),
                "admit",
                "privacy_request",
                id,
                Some(json!({ "from": de })),
                Some(json!({ "status": "en_curso" })),
            )
            .await;
            Ok(Json(json!({ "id": id, "status": "en_curso" })))
        }
        "prorrogar" => {
            if req.extended {
                return Err(AppError::Validation(
                    "la solicitud ya fue prorrogada una vez".into(),
                ));
            }
            let nueva: DateTime<Utc> = sqlx::query_scalar(
                "UPDATE privacy_requests SET extended = TRUE, extended_at = NOW(), \
                    due_at = due_at + INTERVAL '30 days', updated_at = NOW() \
                 WHERE id = $1 RETURNING due_at",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            let msg = best_effort_email(
                &email_to,
                "Prórroga de plazo – Ley 21.719",
                &format!(
                    "Tu solicitud (folio {id}) fue prorrogada por 30 días por complejidad del caso. \
                     Nuevo vencimiento: {nueva}."
                ),
            );
            let _ = send_via_smtp(&state.smtp, &msg).await;
            let _ = insert_audit_log(
                &state.db,
                Some(actor),
                "extend",
                "privacy_request",
                id,
                Some(json!({ "from": de })),
                Some(json!({ "extended": true, "due_at": nueva })),
            )
            .await;
            Ok(Json(json!({ "id": id, "status": de, "due_at": nueva })))
        }
        "resolver-cumplida" => {
            let (resolution_final, bundle): (String, Option<Value>) = match tipo.as_str() {
                "supresion" => {
                    let sugerida =
                        ejecutar_supresion(&mut tx, req.contact_id, req.email.clone()).await?;
                    let r = input
                        .resolution
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or(sugerida);
                    (r, None)
                }
                "bloqueo" => {
                    ejecutar_bloqueo(&mut tx, req.contact_id).await?;
                    let r = input
                        .resolution
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            "Se aplicó el bloqueo: los datos no serán tratados ni cedidos.".into()
                        });
                    (r, None)
                }
                "oposicion" => {
                    ejecutar_oposicion(&mut tx, req.contact_id).await?;
                    let r = input
                        .resolution
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            "Se acogió la oposición: cese del tratamiento objetado.".into()
                        });
                    (r, None)
                }
                "rectificacion" => {
                    let resumen =
                        ejecutar_rectificacion(&mut tx, req.contact_id, req.corrections.clone())
                            .await?;
                    let r = input
                        .resolution
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or(resumen);
                    (r, None)
                }
                "portabilidad" => {
                    let data =
                        bundle_portabilidad(&mut tx, req.contact_id, req.email.clone()).await?;
                    let n_deals = data
                        .get("deals")
                        .and_then(Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0);
                    let n_acts = data
                        .get("activities")
                        .and_then(Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0);
                    let r = input
                        .resolution
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            format!(
                                "Bundle de portabilidad entregado ({n_deals} negocios, {n_acts} actividades)."
                            )
                        });
                    (r, Some(data))
                }
                "acceso" => {
                    let data = resumen_acceso(&mut tx, req.contact_id, req.email.clone()).await?;
                    let r = input
                        .resolution
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            "Se entregó resumen de acceso con fines y plazos de conservación."
                                .into()
                        });
                    (r, Some(data))
                }
                _ => return Err(AppError::Validation("tipo de solicitud inválido".into())),
            };
            sqlx::query(
                "UPDATE privacy_requests SET status = 'cumplida'::privacy_request_status, \
                    resolution = $2, legal_hold = COALESCE($3, legal_hold), \
                    resolved_at = NOW(), updated_at = NOW() WHERE id = $1",
            )
            .bind(id)
            .bind(&resolution_final)
            .bind(
                input
                    .legal_hold
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
            )
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            let msg = best_effort_email(
                &email_to,
                "Resolución de solicitud – Ley 21.719",
                &format!(
                    "Tu solicitud (folio {id}) fue resuelta favorablemente.\n\n{resolution_final}"
                ),
            );
            let _ = send_via_smtp(&state.smtp, &msg).await;
            let _ = insert_audit_log(
                &state.db,
                Some(actor),
                "resolve",
                "privacy_request",
                id,
                Some(json!({ "from": de })),
                Some(json!({ "status": "cumplida", "type": tipo })),
            )
            .await;
            let mut resp = json!({ "id": id, "status": "cumplida" });
            if let Some(data) = bundle {
                resp["data"] = data;
            }
            Ok(Json(resp))
        }
        "resolver-denegada" => {
            let fundamento = input
                .resolution
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    AppError::Validation(
                        "resolver-denegada requiere `resolution` con fundamento (causa + antecedentes)".into(),
                    )
                })?;
            sqlx::query(
                "UPDATE privacy_requests SET status = 'denegada'::privacy_request_status, \
                    resolution = $2, legal_hold = COALESCE($3, legal_hold), \
                    resolved_at = NOW(), updated_at = NOW() WHERE id = $1",
            )
            .bind(id)
            .bind(fundamento)
            .bind(
                input
                    .legal_hold
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
            )
            .execute(&mut *tx)
            .await?;
            if let Some(cid) = req.contact_id {
                sqlx::query("UPDATE contacts SET blocked = FALSE WHERE id = $1")
                    .bind(cid)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            let msg = best_effort_email(
                &email_to,
                "Resolución de solicitud – Ley 21.719",
                &format!(
                    "Tu solicitud (folio {id}) fue denegada por el siguiente fundamento:\n\n{fundamento}\n\n{PIE_RECLAMO_AGENCIA}"
                ),
            );
            let _ = send_via_smtp(&state.smtp, &msg).await;
            let _ = insert_audit_log(
                &state.db,
                Some(actor),
                "deny",
                "privacy_request",
                id,
                Some(json!({ "from": de })),
                Some(json!({ "status": "denegada" })),
            )
            .await;
            Ok(Json(json!({ "id": id, "status": "denegada" })))
        }
        "desistir" => {
            sqlx::query(
                "UPDATE privacy_requests SET status = 'desistida'::privacy_request_status, \
                    resolution = COALESCE($2, resolution), resolved_at = NOW(), \
                    updated_at = NOW() WHERE id = $1",
            )
            .bind(id)
            .bind(
                input
                    .resolution
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
            )
            .execute(&mut *tx)
            .await?;
            if let Some(cid) = req.contact_id {
                sqlx::query("UPDATE contacts SET blocked = FALSE WHERE id = $1")
                    .bind(cid)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            let _ = insert_audit_log(
                &state.db,
                Some(actor),
                "withdraw",
                "privacy_request",
                id,
                Some(json!({ "from": de })),
                Some(json!({ "status": "desistida" })),
            )
            .await;
            Ok(Json(json!({ "id": id, "status": "desistida" })))
        }
        "levantar_bloqueo" => {
            let cid = req.contact_id.ok_or_else(|| {
                AppError::Validation("sin contacto asociado para desbloquear".into())
            })?;
            sqlx::query("UPDATE contacts SET blocked = FALSE WHERE id = $1")
                .bind(cid)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            let _ = insert_audit_log(
                &state.db,
                Some(actor),
                "unblock",
                "privacy_request",
                id,
                Some(json!({ "from": de })),
                Some(json!({ "contact_id": cid, "blocked": false })),
            )
            .await;
            Ok(Json(json!({ "id": id, "status": de })))
        }
        _ => Err(AppError::Validation("acción inválida".into())),
    }
}

// ─── 6. Stats (staff view) ──────────────────────────────────────────────────

/// GET staff: conteo por estado + vencidas. `plazo_dias` fijo en 30.
pub async fn request_stats(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("privacy.view")
        .map_err(|_| AppError::Forbidden)?;
    let filas: Vec<(String, i64)> =
        sqlx::query_as("SELECT status::text, COUNT(*) FROM privacy_requests GROUP BY status")
            .fetch_all(&state.db)
            .await?;
    let mut por_estado = serde_json::Map::new();
    for e in ESTADOS_VALIDOS {
        por_estado.insert((*e).to_string(), json!(0));
    }
    for (estado, n) in filas {
        por_estado.insert(estado, json!(n));
    }
    let vencidas: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM privacy_requests WHERE due_at < NOW() AND status IN \
            ('recibida'::privacy_request_status, 'verificada'::privacy_request_status, \
             'en_curso'::privacy_request_status)",
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(
        json!({ "por_estado": por_estado, "vencidas": vencidas, "plazo_dias": 30 }),
    ))
}

// ─── 7. Ejecución (privado; todo dentro de la transacción del llamante) ─────

/// Supresión: anonimiza contacto, limpia actividades/deals/leads y elimina el
/// usuario con ese email (refresh CASCADE, audit SET NULL). Devuelve resolución
/// sugerida si el staff no dio una.
async fn ejecutar_supresion(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    contact_id: Option<Uuid>,
    request_email: String,
) -> Result<String, AppError> {
    let mut email_previo: Option<String> = None;
    if let Some(cid) = contact_id {
        let fila: Option<Option<String>> =
            sqlx::query_scalar("SELECT email::text FROM contacts WHERE id = $1 FOR UPDATE")
                .bind(cid)
                .fetch_optional(&mut **tx)
                .await?
                .ok_or(AppError::NotFound)?;
        email_previo = fila.flatten();
        sqlx::query(
            "UPDATE contacts SET first_name = 'Suprimido', \
                last_name = '(suprimido Ley 21.719)', email = NULL, phone = NULL, \
                position = NULL, notes = NULL, embedding = NULL, company_id = NULL, \
                blocked = FALSE WHERE id = $1",
        )
        .bind(cid)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "UPDATE activities SET description = '[dato suprimido – Ley 21.719]' \
             WHERE contact_id = $1",
        )
        .bind(cid)
        .execute(&mut **tx)
        .await?;
        sqlx::query("UPDATE deals SET notes = NULL WHERE contact_id = $1")
            .bind(cid)
            .execute(&mut **tx)
            .await?;
    }
    let objetivo = email_previo
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(request_email);
    let objetivo = objetivo.trim().to_string();
    if !objetivo.is_empty() {
        sqlx::query("UPDATE leads SET email = NULL, phone = NULL WHERE lower(email) = lower($1)")
            .bind(&objetivo)
            .execute(&mut **tx)
            .await?;
        let uid: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
            .bind(&objetivo)
            .fetch_optional(&mut **tx)
            .await?;
        if let Some(uid) = uid {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(uid)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok("Datos personales suprimidos y anonimizados (contacto, actividades, negocios, leads y usuario).".into())
}

/// Bloqueo legal: no tratar ni ceder.
async fn ejecutar_bloqueo(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    contact_id: Option<Uuid>,
) -> Result<(), AppError> {
    let cid = contact_id
        .ok_or_else(|| AppError::Validation("sin contacto asociado para bloqueo".into()))?;
    let n = sqlx::query("UPDATE contacts SET blocked = TRUE WHERE id = $1")
        .bind(cid)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Oposición: cese del tratamiento objetado (se materializa como bloqueo).
async fn ejecutar_oposicion(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    contact_id: Option<Uuid>,
) -> Result<(), AppError> {
    ejecutar_bloqueo(tx, contact_id).await
}

/// Rectificación: UPDATE dinámico solo con columnas de la lista blanca.
async fn ejecutar_rectificacion(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    contact_id: Option<Uuid>,
    corrections: Option<Value>,
) -> Result<String, AppError> {
    let cid = contact_id
        .ok_or_else(|| AppError::Validation("sin contacto asociado para rectificar".into()))?;
    let corr = corrections
        .as_ref()
        .ok_or_else(|| AppError::Validation("rectificación requiere `corrections`".into()))?;
    let pares = extraer_corrections(corr)?;
    let mut qb = sqlx::QueryBuilder::<Postgres>::new("UPDATE contacts SET ");
    {
        let mut sep = qb.separated(", ");
        for (col, val) in &pares {
            match col.as_str() {
                "first_name" => {
                    sep.push("first_name = ");
                    sep.push_bind(val);
                }
                "last_name" => {
                    sep.push("last_name = ");
                    sep.push_bind(val);
                }
                "email" => {
                    sep.push("email = ");
                    sep.push_bind(val);
                }
                "phone" => {
                    sep.push("phone = ");
                    sep.push_bind(val);
                }
                "position" => {
                    sep.push("position = ");
                    sep.push_bind(val);
                }
                "notes" => {
                    sep.push("notes = ");
                    sep.push_bind(val);
                }
                _ => {
                    return Err(AppError::Validation(format!("clave no permitida: {col}")));
                }
            }
        }
        sep.push("updated_at = NOW()");
    }
    qb.push(" WHERE id = ");
    qb.push_bind(cid);
    let n = qb.build().execute(&mut **tx).await?.rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    let cols: Vec<&str> = pares.iter().map(|(c, _)| c.as_str()).collect();
    Ok(format!(
        "Se rectificaron {} campo(s): {}.",
        pares.len(),
        cols.join(", ")
    ))
}

#[derive(sqlx::FromRow, serde::Serialize)]
struct ContactDump {
    id: Uuid,
    first_name: String,
    last_name: String,
    email: Option<String>,
    phone: Option<String>,
    company_id: Option<Uuid>,
    position: Option<String>,
    notes: Option<String>,
    blocked: bool,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, serde::Serialize)]
struct CompanyDump {
    id: Uuid,
    name: String,
    email: Option<String>,
    phone: Option<String>,
}

#[derive(sqlx::FromRow, serde::Serialize)]
struct DealDump {
    id: Uuid,
    title: String,
    stage: String,
    notes: Option<String>,
}

#[derive(sqlx::FromRow, serde::Serialize)]
struct ActivityDump {
    id: Uuid,
    subject: String,
    description: Option<String>,
}

#[derive(sqlx::FromRow, serde::Serialize)]
struct RequestHist {
    id: Uuid,
    #[sqlx(rename = "tipo")]
    request_type: String,
    #[sqlx(rename = "estado")]
    status: String,
    created_at: DateTime<Utc>,
}

/// Portabilidad: bundle con contacto + empresa/deals/activities + historial.
async fn bundle_portabilidad(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    contact_id: Option<Uuid>,
    email: String,
) -> Result<Value, AppError> {
    let email = email.trim().to_lowercase();
    let contact: Option<ContactDump> = match contact_id {
        Some(cid) => {
            sqlx::query_as(
                "SELECT id, first_name, last_name, email::text AS email, phone, company_id, \
                position, notes, blocked, created_at FROM contacts WHERE id = $1",
            )
            .bind(cid)
            .fetch_optional(&mut **tx)
            .await?
        }
        None => None,
    };
    let company: Option<CompanyDump> = match contact.as_ref().and_then(|c| c.company_id) {
        Some(comp_id) => {
            sqlx::query_as(
                "SELECT id, name, email::text AS email, phone FROM companies WHERE id = $1",
            )
            .bind(comp_id)
            .fetch_optional(&mut **tx)
            .await?
        }
        None => None,
    };
    let deals: Vec<DealDump> =
        match contact_id {
            Some(cid) => sqlx::query_as(
                "SELECT id, title, stage::text AS stage, notes FROM deals WHERE contact_id = $1 \
             ORDER BY created_at DESC",
            )
            .bind(cid)
            .fetch_all(&mut **tx)
            .await?,
            None => Vec::new(),
        };
    let activities: Vec<ActivityDump> = match contact_id {
        Some(cid) => {
            sqlx::query_as(
                "SELECT id, subject, description FROM activities WHERE contact_id = $1 \
             ORDER BY created_at DESC",
            )
            .bind(cid)
            .fetch_all(&mut **tx)
            .await?
        }
        None => Vec::new(),
    };
    let requests: Vec<RequestHist> = sqlx::query_as(
        "SELECT id, type::text AS tipo, status::text AS estado, created_at \
         FROM privacy_requests WHERE ($1::uuid IS NULL OR contact_id = $1) \
           AND email::text = $2 ORDER BY created_at DESC",
    )
    .bind(contact_id)
    .bind(&email)
    .fetch_all(&mut **tx)
    .await?;
    Ok(json!({
        "contact": contact,
        "company": company,
        "deals": deals,
        "activities": activities,
        "requests": requests,
    }))
}

/// Acceso: resumen con contacto, conteos, finalidades y plazo de conservación.
async fn resumen_acceso(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    contact_id: Option<Uuid>,
    email: String,
) -> Result<Value, AppError> {
    let email = email.trim().to_lowercase();
    let contact: Option<ContactDump> = match contact_id {
        Some(cid) => {
            sqlx::query_as(
                "SELECT id, first_name, last_name, email::text AS email, phone, company_id, \
                position, notes, blocked, created_at FROM contacts WHERE id = $1",
            )
            .bind(cid)
            .fetch_optional(&mut **tx)
            .await?
        }
        None => None,
    };
    let (n_deals, n_acts): (i64, i64) = match contact_id {
        Some(cid) => {
            let d: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM deals WHERE contact_id = $1")
                .bind(cid)
                .fetch_one(&mut **tx)
                .await?;
            let a: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM activities WHERE contact_id = $1")
                    .bind(cid)
                    .fetch_one(&mut **tx)
                    .await?;
            (d, a)
        }
        None => (0, 0),
    };
    let n_companies: i64 = match contact.as_ref().and_then(|c| c.company_id) {
        Some(_) => 1,
        None => 0,
    };
    let n_requests: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM privacy_requests WHERE ($1::uuid IS NULL OR contact_id = $1) \
           AND email::text = $2",
    )
    .bind(contact_id)
    .bind(&email)
    .fetch_one(&mut **tx)
    .await?;
    Ok(json!({
        "contact": contact,
        "counts": {
            "companies": n_companies,
            "deals": n_deals,
            "activities": n_acts,
            "requests": n_requests,
        },
        "finalidades": "Gestión comercial y de clientes del CRM (contacto, ventas y soporte).",
        "plazo_conservacion": "Los datos se conservan mientras dure la relación comercial y los \
            plazos legales aplicables; luego se suprimen o anonimizan.",
    }))
}

// ─── Tests (puros, sin DB) ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn transicion_invalida_rechazada_y_validas_aceptadas() {
        assert!(puede_transitar("recibida", "verificar"));
        assert!(puede_transitar("verificada", "admitir"));
        assert!(puede_transitar("verificada", "prorrogar"));
        assert!(puede_transitar("en_curso", "prorrogar"));
        assert!(puede_transitar("en_curso", "resolver-cumplida"));
        assert!(puede_transitar("en_curso", "resolver-denegada"));
        assert!(puede_transitar("recibida", "desistir"));
        assert!(puede_transitar("verificada", "desistir"));
        assert!(puede_transitar("en_curso", "desistir"));
        assert!(puede_transitar("cumplida", "levantar_bloqueo"));
        assert!(puede_transitar("denegada", "levantar_bloqueo"));
        // Inválidas.
        assert!(!puede_transitar("recibida", "admitir"));
        assert!(!puede_transitar("recibida", "resolver-cumplida"));
        assert!(!puede_transitar("verificada", "resolver-cumplida"));
        assert!(!puede_transitar("cumplida", "admitir"));
        assert!(!puede_transitar("denegada", "prorrogar"));
        assert!(!puede_transitar("recibida", "levantar_bloqueo"));
        assert!(!puede_transitar("en_curso", "levantar_bloqueo"));
        assert!(!puede_transitar("cumplida", "desistir"));
        assert!(!puede_transitar("desistida", "admitir"));
        assert!(!puede_transitar("desconocido", "verificar"));
        assert!(!puede_transitar("recibida", "inexistente"));
    }

    #[test]
    fn due_normal_es_mas_30_dias() {
        let base = Utc.with_ymd_and_hms(2026, 1, 5, 12, 0, 0).unwrap();
        assert_eq!(due_normal(base), base + chrono::Duration::days(30));
        assert_eq!(calcular_due_at("acceso", base), due_normal(base));
        assert_eq!(calcular_due_at("supresion", base), due_normal(base));
        assert_eq!(calcular_due_at("portabilidad", base), due_normal(base));
    }

    #[test]
    fn due_bloqueo_cae_en_dia_habil() {
        // Viernes 2026-10-16 -> +2 hábiles = martes 2026-10-20.
        let vie = Utc.with_ymd_and_hms(2026, 10, 16, 12, 0, 0).unwrap();
        assert_eq!(vie.weekday(), Weekday::Fri);
        let d = due_bloqueo(vie);
        assert_eq!(d.weekday(), Weekday::Tue);
        assert!(!matches!(d.weekday(), Weekday::Sat | Weekday::Sun));
        // Lunes -> miércoles.
        let lun = Utc.with_ymd_and_hms(2026, 10, 12, 12, 0, 0).unwrap();
        assert_eq!(lun.weekday(), Weekday::Mon);
        assert_eq!(due_bloqueo(lun).weekday(), Weekday::Wed);
        // Sábado -> martes (salta dom/lun... lun(1), mar(2)).
        let sab = Utc.with_ymd_and_hms(2026, 10, 17, 12, 0, 0).unwrap();
        assert_eq!(sab.weekday(), Weekday::Sat);
        let ds = due_bloqueo(sab);
        assert!(!matches!(ds.weekday(), Weekday::Sat | Weekday::Sun));
        assert_eq!(calcular_due_at("bloqueo", vie), due_bloqueo(vie));
    }

    #[test]
    fn verify_compara_hash_sha256() {
        let t = crate::auth::new_refresh_token();
        assert_eq!(t.len(), 64);
        let h = crate::auth::hash_refresh_token(&t);
        assert!(hash_igual(&t, &h));
        assert!(hash_igual(&format!("  {t}  "), &h)); // trim antes de hashear
        assert!(!hash_igual(&crate::auth::new_refresh_token(), &h));
        assert!(!hash_igual("otro-token", &h));
    }

    #[test]
    fn whitelist_rectificacion_rechaza_clave_extrana() {
        let mala = json!({ "first_name": "Ana", "is_admin": true });
        assert!(extraer_corrections(&mala).is_err());
        let no_objeto = json!(["first_name"]);
        assert!(extraer_corrections(&no_objeto).is_err());
        let email_malo = json!({ "email": "no-es-email" });
        assert!(extraer_corrections(&email_malo).is_err());
        let vacio = json!({});
        assert!(extraer_corrections(&vacio).is_err());
        let ok = json!({ "first_name": "Ana", "email": "a@b.com" });
        let pares = extraer_corrections(&ok).unwrap();
        assert_eq!(pares.len(), 2);
        // `con_bloqueo` es metadata y se ignora.
        let meta = json!({ "first_name": "Ana", "con_bloqueo": true });
        assert_eq!(extraer_corrections(&meta).unwrap().len(), 1);
    }
}
