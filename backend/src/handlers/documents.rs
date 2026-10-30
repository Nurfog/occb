//! Endpoints de documentos (metadatos en DB + binarios en `upload.dir`).
//!
//! Solo-autenticado (`Extension<Claims>`). Los binarios se guardan en el
//! directorio de uploads con nombre generado (uuid); el `filename` que envía
//! el cliente se sanea (anti path-traversal) y solo se conserva como
//! `original_name` informativo.

use axum::{
    Extension, Json,
    body::Bytes,
    extract::{Multipart, Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::Claims,
    error::AppError,
    middleware::auth_ext::UserPermissions,
    models::{
        document::{CreateDocument, Document, DocumentFilter},
        escape_like,
    },
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub id: Uuid,
    pub filename: String,
    pub original_name: String,
    pub mime_type: Option<String>,
    pub file_size: i64,
    pub folder: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const LIST_COLUMNS: &str = "id, filename, original_name, mime_type, file_size, folder, \
    uploaded_by, created_at, updated_at";

pub async fn list_documents(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Query(params): Query<DocumentFilter>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("documents.view")
        .map_err(|_| AppError::Forbidden)?;
    params.validate()?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    // Filtros opcionales con binds; el `format!` solo añade fragmentos fijos
    // con índices contados en el mismo orden en que se bindea.
    let mut sql = format!("SELECT {LIST_COLUMNS} FROM documents WHERE 1=1");
    let mut idx: u32 = 1;
    if params.folder.is_some() {
        sql.push_str(&format!(" AND folder = ${idx}"));
        idx += 1;
    }
    if params.search.is_some() {
        sql.push_str(&format!(" AND (original_name ILIKE ${idx} ESCAPE '\\')"));
        idx += 1;
    }
    if params.mime_type.is_some() {
        sql.push_str(&format!(" AND mime_type ILIKE ${idx} ESCAPE '\\'"));
        idx += 1;
    }
    sql.push_str(&format!(
        " AND (${idx}::uuid IS NULL OR tenant_id = ${idx})"
    ));
    sql.push_str(" ORDER BY created_at DESC");

    let search_pattern;
    let mime_pattern;
    let mut q = sqlx::query_as::<_, Document>(&sql);
    if let Some(ref folder) = params.folder {
        q = q.bind(folder.as_str());
    }
    if let Some(ref search) = params.search {
        search_pattern = format!("%{}%", escape_like(search));
        q = q.bind(search_pattern.as_str());
    }
    if let Some(ref mime) = params.mime_type {
        // Se escapan comodines LIKE del filtro para evitar matches amplios.
        mime_pattern = format!("{}%", escape_like(mime));
        q = q.bind(mime_pattern.as_str());
    }
    q = q.bind(scope);

    let documents = q.fetch_all(&state.db).await?;
    Ok(Json(documents))
}

pub async fn get_document(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("documents.view")
        .map_err(|_| AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let sql = format!(
        "SELECT {LIST_COLUMNS} FROM documents WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2)"
    );
    let doc = sqlx::query_as::<_, Document>(&sql)
        .bind(id)
        .bind(scope)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(doc))
}

/// Alta de metadatos sin binario (el upload de bytes está pendiente de storage).
pub async fn create_document(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Json(input): Json<CreateDocument>,
    // TODO storage: validar contra el binario real cuando haya upload dir.
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("documents.upload")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let uploaded_by = input.uploaded_by.unwrap_or(claims.sub);
    let doc = sqlx::query_as::<_, Document>(
        "INSERT INTO documents (filename, original_name, mime_type, file_size, folder, \
            uploaded_by, tenant_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id, filename, original_name, mime_type, file_size, folder, uploaded_by, \
            created_at, updated_at",
    )
    .bind(&input.filename)
    .bind(&input.original_name)
    .bind(input.mime_type.as_deref())
    .bind(input.file_size)
    .bind(input.folder.as_deref())
    .bind(uploaded_by)
    .bind(scope)
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(doc)))
}

/// Sanea el nombre de archivo del cliente (anti path-traversal):
/// se queda con el basename (`/` y `\`), sin controles, máx. 255 chars.
/// Devuelve error si queda vacío o es `.`/`..`.
fn sanitize_filename(raw: &str) -> Result<String, AppError> {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    let base = base.trim();
    if base.is_empty() || base == "." || base == ".." || base.len() > 255 {
        return Err(AppError::Validation("nombre de archivo inválido".into()));
    }
    Ok(base.to_string())
}

/// Nombre de almacenamiento: uuid + extensión saneada (anti path-traversal:
/// el nombre NUNCA contiene separadores porque se genera aquí).
fn storage_name(original: &str) -> Result<String, AppError> {
    if original.contains('/') || original.contains('\\') || original.contains("..") {
        return Err(AppError::Validation("nombre de archivo inválido".into()));
    }
    let ext = original
        .rsplit('.')
        .next()
        .filter(|e| !e.is_empty() && e.len() <= 16 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_default();
    Ok(format!("{}{ext}", Uuid::new_v4()))
}

/// Valida el `folder` opcional del multipart (metadato; sin separadores).
fn sanitize_folder(raw: &str) -> Result<Option<String>, AppError> {
    let folder = raw.trim();
    if folder.is_empty() {
        return Ok(None);
    }
    if folder.len() > 100
        || folder.contains('/')
        || folder.contains('\\')
        || folder.contains("..")
        || folder.chars().any(|c| c.is_control())
    {
        return Err(AppError::Validation("folder inválido".into()));
    }
    Ok(Some(folder.to_string()))
}

/// Subida multipart (`file` + `folder` opcional): streaming con límite
/// `MAX_FILE_SIZE_MB`, guarda en `upload.dir` y registra en `documents`.
pub async fn upload_document(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("documents.upload")
        .map_err(|_| AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let max_bytes = state.upload.max_file_size_bytes();

    let mut folder: Option<String> = None;
    let mut file_bytes: Option<Bytes> = None;
    let mut original_name: Option<String> = None;
    let mut mime_type: Option<String> = None;

    while let Some(mut field) = multipart.next_field().await.map_err(|e| {
        tracing::warn!("multipart inválido: {e}");
        AppError::Validation("multipart inválido".to_string())
    })? {
        let name = field.name().unwrap_or("").to_string();
        if name == "folder" {
            let text = field
                .text()
                .await
                .map_err(|_| AppError::Validation("campo folder inválido".to_string()))?;
            folder = sanitize_folder(&text)?;
        } else if name == "file" && file_bytes.is_none() {
            let fname = field.file_name().unwrap_or("").to_string();
            let clean = sanitize_filename(&fname)?;
            let ctype = field.content_type().map(str::to_string);
            // Streaming con corte: acumula hasta el límite y falla (422).
            let mut buf = Vec::new();
            while let Some(chunk) = field.chunk().await.map_err(|e| {
                tracing::warn!("error leyendo chunk multipart");
                let _ = e;
                AppError::Validation("error leyendo el archivo".to_string())
            })? {
                buf.extend_from_slice(&chunk);
                if buf.len() as u64 > max_bytes {
                    return Err(AppError::Validation(format!(
                        "archivo demasiado grande (máx. {} MB)",
                        state.upload.max_file_size_mb
                    )));
                }
            }
            if buf.is_empty() {
                return Err(AppError::Validation("archivo vacío".into()));
            }
            original_name = Some(clean);
            mime_type = ctype;
            file_bytes = Some(Bytes::from(buf));
        }
        // Otros campos se ignoran (compatibilidad sin sorpresas).
    }

    let (bytes, original) = match (file_bytes, original_name) {
        (Some(b), Some(n)) => (b, n),
        _ => {
            return Err(AppError::Validation(
                "falta el campo `file` con nombre de archivo".into(),
            ));
        }
    };

    let stored = storage_name(&original)?;
    let path = std::path::Path::new(&state.upload.dir).join(&stored);
    tokio::fs::write(&path, &bytes).await.map_err(|e| {
        tracing::error!("no se pudo guardar el upload en el directorio configurado: {e}");
        AppError::Internal
    })?;

    let doc = sqlx::query_as::<_, Document>(
        "INSERT INTO documents (filename, original_name, mime_type, file_size, folder, \
            uploaded_by, tenant_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id, filename, original_name, mime_type, file_size, folder, uploaded_by, \
            created_at, updated_at",
    )
    .bind(&stored)
    .bind(&original)
    .bind(mime_type.as_deref())
    .bind(bytes.len() as i64)
    .bind(folder.as_deref())
    .bind(claims.sub)
    .bind(scope)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        // Compensa el binario si la DB falla (sin huérfanos en disco).
        let _ = std::fs::remove_file(&path);
        AppError::from(e)
    })?;

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            id: doc.id,
            filename: doc.filename,
            original_name: doc.original_name,
            mime_type: doc.mime_type,
            file_size: doc.file_size,
            folder: doc.folder,
            created_at: doc.created_at,
        }),
    ))
}

/// Descarga: sirve los bytes de `upload.dir` con el content-type de la DB.
/// 404 si el metadato o el binario faltan.
pub async fn download_document(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    perms
        .require("documents.view")
        .map_err(|_| AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let sql = format!(
        "SELECT {LIST_COLUMNS} FROM documents WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2)"
    );
    let doc = sqlx::query_as::<_, Document>(&sql)
        .bind(id)
        .bind(scope)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    // Defensa en profundidad: el `filename` almacenado lo genera el servidor
    // (uuid); si una fila legacy trae separadores, no se toca el filesystem.
    if doc.filename.contains('/') || doc.filename.contains('\\') || doc.filename.contains("..") {
        tracing::error!("documents: filename sospechoso en DB");
        return Err(AppError::NotFound);
    }
    let path = std::path::Path::new(&state.upload.dir).join(&doc.filename);
    let bytes = tokio::fs::read(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::NotFound
        } else {
            tracing::error!("documents: no se pudo leer el binario");
            AppError::Internal
        }
    })?;
    let content_type = doc
        .mime_type
        .as_deref()
        .filter(|m| !m.is_empty())
        .unwrap_or("application/octet-stream");
    let safe_name: String = doc
        .original_name
        .chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\\')
        .collect();
    let disposition = format!(
        "attachment; filename=\"{}\"",
        if safe_name.is_empty() {
            "download".to_string()
        } else {
            safe_name
        }
    );
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CONTENT_DISPOSITION, disposition),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        bytes,
    )
        .into_response())
}

pub async fn delete_document(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
    // TODO storage: borrar también el binario del upload dir cuando exista.
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("documents.delete")
        .map_err(|_| AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let sql = format!(
        "SELECT {LIST_COLUMNS} FROM documents WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2)"
    );
    sqlx::query_as::<_, Document>(&sql)
        .bind(id)
        .bind(scope)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let result =
        sqlx::query("DELETE FROM documents WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2)")
            .bind(id)
            .bind(scope)
            .execute(&state.db)
            .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_bloquea_traversal_y_vacios() {
        assert_eq!(sanitize_filename("informe.pdf").unwrap(), "informe.pdf");
        assert_eq!(sanitize_filename("../../etc/passwd").unwrap(), "passwd");
        assert_eq!(sanitize_filename("C:\\win\\a.txt").unwrap(), "a.txt");
        assert!(sanitize_filename("..").is_err());
        assert!(sanitize_filename("").is_err());
        assert!(sanitize_filename("   ").is_err());
    }

    #[test]
    fn storage_name_es_uuid_sin_separadores() {
        let n = storage_name("informe.PDF").unwrap();
        assert!(n.ends_with(".pdf"));
        assert!(!n.contains('/') && !n.contains('\\') && !n.contains(".."));
        assert_ne!(n, storage_name("informe.PDF").unwrap());
        assert!(storage_name("../x").is_err());
    }

    #[test]
    fn sanitize_folder_rechaza_separadores() {
        assert_eq!(sanitize_folder("").unwrap(), None);
        assert_eq!(sanitize_folder("general").unwrap().unwrap(), "general");
        assert!(sanitize_folder("../x").is_err());
        assert!(sanitize_folder("a/b").is_err());
    }
}
