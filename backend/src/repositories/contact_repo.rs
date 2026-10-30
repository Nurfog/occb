//! Repo de contactos (grupo A, port origen).
//!
//! - WHERE compartido para COUNT + SELECT vía `QueryBuilder` (binds siempre;
//!   `sort_col`/`sort_dir` solo desde la lista blanca de `PaginationParams`).
//! - LIKE con `escape_like` + `ESCAPE '\'`.
//! - COUNT y SELECT en concurrente (`tokio::join!`).

use sqlx::PgPool;
use sqlx::Postgres;
use sqlx::QueryBuilder;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::{Contact, ContactFilters, CreateContact, PaginationParams, UpdateContact};

const BASE_SELECT: &str = "SELECT id, first_name, last_name, email, phone, company_id, position, notes, created_at, updated_at FROM contacts";

/// WHERE compartido para COUNT y SELECT.
/// `sort_col`/`sort_dir` vienen de lista blanca, así que interpolarlos como
/// identificadores es seguro; todos los valores usan binds.
/// Predicado de aislamiento por empresa (fase 2): platform (None) ve todo,
/// tenant solo sus filas. Se añade con binds (QueryBuilder numera solo).
fn push_tenant_scope(qb: &mut QueryBuilder<'_, Postgres>, scope: Option<Uuid>) {
    // OJO: push_bind ya genera el `$N`; aquí solo va el paréntesis.
    qb.push("(");
    qb.push_bind(scope);
    qb.push("::uuid IS NULL OR tenant_id = ");
    qb.push_bind(scope);
    qb.push(")");
}

fn push_contact_conditions(
    qb: &mut QueryBuilder<'_, Postgres>,
    params: &PaginationParams,
    filters: &ContactFilters,
    scope: Option<Uuid>,
) -> Result<(), AppError> {
    let mut first = true;
    let mut and = |qb: &mut QueryBuilder<'_, Postgres>| {
        if first {
            qb.push(" WHERE ");
            first = false;
        } else {
            qb.push(" AND ");
        }
    };

    if let Some(search) = params.search.as_deref().filter(|s| !s.trim().is_empty()) {
        and(qb);
        let pattern = format!("%{}%", crate::models::escape_like(search));
        qb.push("(first_name ILIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" ESCAPE '\\' OR last_name ILIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" ESCAPE '\\' OR email ILIKE ");
        qb.push_bind(pattern);
        qb.push(" ESCAPE '\\')");
    }

    if let Some(company) = filters.company.as_deref().filter(|s| !s.trim().is_empty()) {
        and(qb);
        let pattern = format!("%{}%", crate::models::escape_like(company));
        qb.push("EXISTS (SELECT 1 FROM companies co WHERE co.id = contacts.company_id AND co.name ILIKE ");
        qb.push_bind(pattern);
        qb.push(" ESCAPE '\\')");
    }

    if let Some(position) = filters.position.as_deref().filter(|s| !s.trim().is_empty()) {
        and(qb);
        let pattern = format!("%{}%", crate::models::escape_like(position));
        qb.push("position ILIKE ");
        qb.push_bind(pattern);
        qb.push(" ESCAPE '\\'");
    }

    if let Some(after) = filters
        .created_after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let date = parse_filter_date(after)?;
        and(qb);
        // Cast explícito a ::date para que el tipo del bind sea inequívoco;
        // Postgres lo coerce a medianoche timestamptz en la comparación.
        qb.push("contacts.created_at >= ");
        qb.push_bind(date);
        qb.push("::date");
    }

    if let Some(before) = filters
        .created_before
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let date = parse_filter_date(before)?;
        and(qb);
        // Día completo inclusivo.
        qb.push("contacts.created_at < (");
        qb.push_bind(date);
        qb.push("::date + INTERVAL '1 day')");
    }

    and(qb);
    push_tenant_scope(qb, scope);

    Ok(())
}

fn parse_filter_date(s: &str) -> Result<chrono::NaiveDate, AppError> {
    chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::Validation("Invalid date filter (expected YYYY-MM-DD)".into()))
}

pub struct PgContactRepo {
    pool: PgPool,
}

impl PgContactRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_all(
        &self,
        params: &PaginationParams,
        filters: &ContactFilters,
        scope: Option<Uuid>,
    ) -> Result<(Vec<Contact>, i64), AppError> {
        let per_page = params.per_page();
        let offset = params.offset();
        let sort_col = params.sort_column().to_string();
        let sort_dir = params.sort_direction().to_string();

        let mut count_qb: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT COUNT(*) FROM contacts");
        push_contact_conditions(&mut count_qb, params, filters, scope)?;
        let count_fut = count_qb.build_query_as::<(i64,)>().fetch_one(&self.pool);

        let mut data_qb: QueryBuilder<Postgres> = QueryBuilder::new(BASE_SELECT);
        push_contact_conditions(&mut data_qb, params, filters, scope)?;
        data_qb.push(" ORDER BY ");
        data_qb.push(sort_col);
        data_qb.push(" ");
        data_qb.push(sort_dir);
        data_qb.push(" LIMIT ");
        data_qb.push_bind(per_page);
        data_qb.push(" OFFSET ");
        data_qb.push_bind(offset);
        let data_fut = data_qb.build_query_as::<Contact>().fetch_all(&self.pool);

        // COUNT + SELECT en concurrente en vez de secuencial.
        let (total, contacts) = tokio::join!(count_fut, data_fut);
        Ok((contacts?, total?.0))
    }

    pub async fn find_by_id(
        &self,
        id: Uuid,
        scope: Option<Uuid>,
    ) -> Result<Option<Contact>, AppError> {
        let contact = sqlx::query_as::<_, Contact>(&format!(
            "{BASE_SELECT} WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2)"
        ))
        .bind(id)
        .bind(scope)
        .fetch_optional(&self.pool)
        .await?;
        Ok(contact)
    }

    pub async fn create(
        &self,
        input: &CreateContact,
        scope: Option<Uuid>,
    ) -> Result<Contact, AppError> {
        // company_id apuntado debe ser visible (evita cross-link entre empresas).
        if let Some(company_id) = input.company_id {
            let visible: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2))",
            )
            .bind(company_id)
            .bind(scope)
            .fetch_one(&self.pool)
            .await?;
            if !visible {
                return Err(AppError::NotFound);
            }
        }
        let contact = sqlx::query_as::<_, Contact>(
            r#"
            INSERT INTO contacts (first_name, last_name, email, phone, company_id, position, notes, tenant_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, first_name, last_name, email, phone, company_id, position, notes, created_at, updated_at
            "#,
        )
        .bind(&input.first_name)
        .bind(&input.last_name)
        .bind(&input.email)
        .bind(&input.phone)
        .bind(input.company_id)
        .bind(&input.position)
        .bind(&input.notes)
        .bind(scope)
        .fetch_one(&self.pool)
        .await?;
        Ok(contact)
    }

    pub async fn update(
        &self,
        id: Uuid,
        input: &UpdateContact,
        scope: Option<Uuid>,
    ) -> Result<Option<Contact>, AppError> {
        if let Some(company_id) = input.company_id {
            let visible: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2))",
            )
            .bind(company_id)
            .bind(scope)
            .fetch_one(&self.pool)
            .await?;
            if !visible {
                return Err(AppError::NotFound);
            }
        }
        let contact = sqlx::query_as::<_, Contact>(
            r#"
            UPDATE contacts
            SET first_name = COALESCE($2, first_name),
                last_name = COALESCE($3, last_name),
                email = COALESCE($4, email),
                phone = COALESCE($5, phone),
                company_id = COALESCE($6, company_id),
                position = COALESCE($7, position),
                notes = COALESCE($8, notes),
                updated_at = NOW()
            WHERE id = $1 AND ($9::uuid IS NULL OR tenant_id = $9)
            RETURNING id, first_name, last_name, email, phone, company_id, position, notes, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&input.first_name)
        .bind(&input.last_name)
        .bind(&input.email)
        .bind(&input.phone)
        .bind(input.company_id)
        .bind(&input.position)
        .bind(&input.notes)
        .bind(scope)
        .fetch_optional(&self.pool)
        .await?;
        Ok(contact)
    }

    pub async fn delete(&self, id: Uuid, scope: Option<Uuid>) -> Result<Option<Contact>, AppError> {
        let old = self.find_by_id(id, scope).await?;
        if old.is_none() {
            return Ok(None);
        }

        let result = sqlx::query(
            "DELETE FROM contacts WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2)",
        )
        .bind(id)
        .bind(scope)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        Ok(old)
    }

    pub async fn bulk_delete(&self, ids: &[Uuid], scope: Option<Uuid>) -> Result<usize, AppError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let result = sqlx::query(
            "DELETE FROM contacts WHERE id = ANY($1) AND ($2::uuid IS NULL OR tenant_id = $2)",
        )
        .bind(ids)
        .bind(scope)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as usize)
    }

    pub async fn find_all_for_export(
        &self,
        search: Option<&str>,
        scope: Option<Uuid>,
    ) -> Result<Vec<Contact>, AppError> {
        // TODO: stremear el CSV en vez de cargar todo en memoria.
        // Tope de 10k filas para acotar memoria/tiempo.
        let contacts = if let Some(search) = search {
            let pattern = format!("%{}%", crate::models::escape_like(search));
            sqlx::query_as::<_, Contact>(&format!(
                "{BASE_SELECT} WHERE (first_name ILIKE $1 ESCAPE '\\' OR last_name ILIKE $1 ESCAPE '\\' OR email ILIKE $1 ESCAPE '\\') AND (blocked IS NULL OR blocked = FALSE) AND ($2::uuid IS NULL OR tenant_id = $2) ORDER BY created_at DESC LIMIT 10000"
            ))
            .bind(pattern)
            .bind(scope)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, Contact>(&format!(
                "{BASE_SELECT} WHERE (blocked IS NULL OR blocked = FALSE) AND ($1::uuid IS NULL OR tenant_id = $1) ORDER BY created_at DESC LIMIT 10000"
            ))
            .bind(scope)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(contacts)
    }

    pub async fn import_one(
        &self,
        first_name: &str,
        last_name: &str,
        email: Option<&str>,
        phone: Option<&str>,
        position: Option<&str>,
        scope: Option<Uuid>,
    ) -> Result<Contact, AppError> {
        let contact = sqlx::query_as::<_, Contact>(
            r#"
            INSERT INTO contacts (first_name, last_name, email, phone, position, tenant_id)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, first_name, last_name, email, phone, company_id, position, notes, created_at, updated_at
            "#,
        )
        .bind(first_name)
        .bind(last_name)
        .bind(email)
        .bind(phone)
        .bind(position)
        .bind(scope)
        .fetch_one(&self.pool)
        .await?;
        Ok(contact)
    }
}
