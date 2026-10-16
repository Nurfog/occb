//! Servicio de contactos (grupo A, port origen adaptado).
//!
//! Adaptación:
//! - Sin `AppState` con repos: el servicio posee su `PgContactRepo` (creado
//!   desde `state.db.clone()` en cada handler; es solo un `PgPool` clonado).
//! - Sin auditoría ni webhooks: el origen llamaba a `insert_audit_log` y a
//!   `enqueue_event`, que pertenecen a otros grupos.
//!   // TODO port: requiere audit (insert_audit_log) y webhooks
//!   // (enqueue_event + WebhookEvent) para re-añadir side-effects.
//! - `import`/`export` usan los helpers de `crate::models` (parser propio,
//!   sin la crate `csv`).

use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::error::AppError;
use crate::models::{
    Contact, ContactFilters, CreateContact, PaginatedResponse, PaginationParams, UpdateContact,
};
use crate::models::{ImportResult, escape_csv, parse_csv_rows};
use crate::repositories::contact_repo::PgContactRepo;

pub struct ContactService {
    repo: PgContactRepo,
}

impl ContactService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: PgContactRepo::new(pool),
        }
    }

    pub async fn list(
        &self,
        params: &PaginationParams,
        filters: &ContactFilters,
    ) -> Result<PaginatedResponse<Contact>, AppError> {
        let page = params.page();
        let per_page = params.per_page();
        let (contacts, total) = self.repo.find_all(params, filters).await?;
        Ok(PaginatedResponse::new(contacts, total, page, per_page))
    }

    pub async fn get(&self, id: Uuid) -> Result<Contact, AppError> {
        self.repo.find_by_id(id).await?.ok_or(AppError::NotFound)
    }

    pub async fn create(&self, input: &CreateContact) -> Result<Contact, AppError> {
        let contact = self.repo.create(input).await?;
        // TODO port: requiere audit (insert_audit_log "created"/"contact")
        // y webhooks (enqueue_event ContactCreated).
        Ok(contact)
    }

    pub async fn update(&self, id: Uuid, input: &UpdateContact) -> Result<Contact, AppError> {
        self.repo.find_by_id(id).await?.ok_or(AppError::NotFound)?;
        let contact = self
            .repo
            .update(id, input)
            .await?
            .ok_or(AppError::NotFound)?;
        // TODO port: requiere audit ("updated") y webhooks (ContactUpdated).
        Ok(contact)
    }

    pub async fn delete(&self, id: Uuid) -> Result<(), AppError> {
        self.repo.delete(id).await?.ok_or(AppError::NotFound)?;
        // TODO port: requiere audit ("deleted") y webhooks (ContactDeleted).
        Ok(())
    }

    pub async fn bulk_delete(&self, ids: &[Uuid]) -> Result<usize, AppError> {
        let deleted = self.repo.bulk_delete(ids).await?;
        // TODO port: requiere audit por fila + webhooks (ContactDeleted).
        // (El origen leía las filas antes de borrar para auditarlas.)
        Ok(deleted)
    }

    pub async fn export(&self, search: Option<&str>) -> Result<String, AppError> {
        let contacts = self.repo.find_all_for_export(search).await?;

        let mut csv = String::from("first_name,last_name,email,phone,position,company_id,notes\n");
        for c in &contacts {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                escape_csv(&c.first_name),
                escape_csv(&c.last_name),
                escape_csv(c.email.as_deref().unwrap_or("")),
                escape_csv(c.phone.as_deref().unwrap_or("")),
                escape_csv(c.position.as_deref().unwrap_or("")),
                c.company_id.map(|id| id.to_string()).unwrap_or_default(),
                escape_csv(c.notes.as_deref().unwrap_or(""))
            ));
        }

        Ok(csv)
    }

    pub async fn import(&self, body: &str) -> Result<ImportResult, AppError> {
        if body.len() > 2 * 1024 * 1024 {
            return Err(AppError::Validation(
                "Import body too large (max 2MB)".into(),
            ));
        }
        let rows = parse_csv_rows(body);
        // Acota trabajo por petición: 2MB de filas tiny = decenas de miles
        // de INSERTs secuenciales.
        if rows.len() > 1000 {
            return Err(AppError::Validation(
                "Too many rows (max 1000 per import)".into(),
            ));
        }

        let mut imported = 0u32;
        let mut errors = Vec::new();

        for (row_num, fields) in rows.iter().enumerate() {
            if fields.len() < 2 {
                errors.push(format!("Línea {}: formato inválido", row_num + 2));
                continue;
            }

            // Validar con las mismas reglas del create individual.
            let candidate = CreateContact {
                first_name: fields[0].trim().to_string(),
                last_name: fields[1].trim().to_string(),
                email: fields
                    .get(2)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                phone: fields
                    .get(3)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                company_id: None,
                position: fields
                    .get(4)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                notes: None,
            };
            if let Err(e) = candidate.validate() {
                errors.push(format!(
                    "Línea {}: {}",
                    row_num + 2,
                    first_validation_message(&e)
                ));
                continue;
            }

            match self
                .repo
                .import_one(
                    &candidate.first_name,
                    &candidate.last_name,
                    candidate.email.as_deref(),
                    candidate.phone.as_deref(),
                    candidate.position.as_deref(),
                )
                .await
            {
                Ok(_) => imported += 1,
                Err(e) => errors.push(format!("Línea {}: {}", row_num + 2, e)),
            }
        }

        Ok(ImportResult { imported, errors })
    }
}

/// Primer mensaje legible de un set de errores de `validator`.
fn first_validation_message(e: &validator::ValidationErrors) -> String {
    e.field_errors()
        .values()
        .flat_map(|errs| errs.iter())
        .filter_map(|e| e.message.as_ref().map(|m| m.to_string()))
        .next()
        .unwrap_or_else(|| "validación inválida".to_string())
}
