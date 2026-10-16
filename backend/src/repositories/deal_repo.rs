//! Repo de deals (grupo A, port origen adaptado).
//!
//! Adaptación: `value` es `f64` (sin `rust_decimal`); los INSERT/UPDATE usan
//! `value::float8 AS value` en el RETURNING y `map_deal_row` decodifica con
//! `Row::get::<f64, _>`. SELECTs base en `crate::queries::deal_queries`.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::{Deal, DealStage, UpdateDeal};
use crate::queries::deal_queries::{DEAL_SELECT, DEAL_SELECT_BY_ID};

pub struct PgDealRepo {
    pool: PgPool,
}

impl PgDealRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Deal>, AppError> {
        let deal = sqlx::query_as::<_, Deal>(DEAL_SELECT_BY_ID)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(deal)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        title: &str,
        value: f64,
        currency: &str,
        stage: DealStage,
        contact_id: Option<Uuid>,
        company_id: Option<Uuid>,
        expected_close_date: Option<chrono::DateTime<chrono::Utc>>,
        notes: Option<&str>,
    ) -> Result<Deal, AppError> {
        let row = sqlx::query(
            "INSERT INTO deals (title, value, currency, stage, contact_id, company_id, expected_close_date, notes) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id, title, value::float8 AS value, currency, stage, position, contact_id, company_id, pipeline_id, pipeline_stage_id, expected_close_date, notes, (SELECT CONCAT(c.first_name, ' ', c.last_name) FROM contacts c WHERE c.id = contact_id) as contact_name, (SELECT co.name FROM companies co WHERE co.id = company_id) as company_name, created_at, updated_at"
        )
        .bind(title)
        .bind(value)
        .bind(currency)
        .bind(stage)
        .bind(contact_id)
        .bind(company_id)
        .bind(expected_close_date)
        .bind(notes)
        .fetch_one(&self.pool)
        .await?;

        Ok(map_deal_row(row))
    }

    pub async fn update(&self, id: Uuid, input: &UpdateDeal) -> Result<Option<Deal>, AppError> {
        let row = sqlx::query(
            "UPDATE deals SET title = COALESCE($2, title), value = COALESCE($3, value), currency = COALESCE($4, currency), stage = COALESCE($5, stage), contact_id = COALESCE($6, contact_id), company_id = COALESCE($7, company_id), expected_close_date = COALESCE($8, expected_close_date), notes = COALESCE($9, notes), updated_at = NOW() WHERE id = $1 RETURNING id, title, value::float8 AS value, currency, stage, position, contact_id, company_id, pipeline_id, pipeline_stage_id, expected_close_date, notes, (SELECT CONCAT(c.first_name, ' ', c.last_name) FROM contacts c WHERE c.id = contact_id) as contact_name, (SELECT co.name FROM companies co WHERE co.id = company_id) as company_name, created_at, updated_at"
        )
        .bind(id)
        .bind(&input.title)
        .bind(input.value)
        .bind(&input.currency)
        .bind(input.stage.clone())
        .bind(input.contact_id)
        .bind(input.company_id)
        .bind(input.expected_close_date)
        .bind(&input.notes)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(map_deal_row))
    }

    pub async fn update_stage(
        &self,
        id: Uuid,
        stage: DealStage,
        position: Option<i32>,
    ) -> Result<Option<Deal>, AppError> {
        let row = sqlx::query(
            "UPDATE deals SET stage = $2, position = COALESCE($3, position), updated_at = NOW() WHERE id = $1 RETURNING id, title, value::float8 AS value, currency, stage, position, contact_id, company_id, pipeline_id, pipeline_stage_id, expected_close_date, notes, (SELECT CONCAT(c.first_name, ' ', c.last_name) FROM contacts c WHERE c.id = contact_id) as contact_name, (SELECT co.name FROM companies co WHERE co.id = company_id) as company_name, created_at, updated_at"
        )
        .bind(id)
        .bind(stage)
        .bind(position)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(map_deal_row))
    }

    pub async fn delete(&self, id: Uuid) -> Result<Option<Deal>, AppError> {
        let old = self.find_by_id(id).await?;
        if old.is_none() {
            return Ok(None);
        }

        let result = sqlx::query("DELETE FROM deals WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        Ok(old)
    }

    pub async fn find_all_for_export(&self, search: Option<&str>) -> Result<Vec<Deal>, AppError> {
        // TODO: stremear el CSV en vez de cargar todo en memoria.
        // Tope de 10k filas para acotar memoria/tiempo.
        let deals = if let Some(search) = search {
            let pattern = format!("%{}%", crate::models::escape_like(search));
            sqlx::query_as::<_, Deal>(&format!(
                "{DEAL_SELECT} WHERE d.title ILIKE $1 ESCAPE '\\' AND NOT EXISTS (SELECT 1 FROM contacts c WHERE c.id = d.contact_id AND c.blocked = TRUE) ORDER BY d.created_at DESC LIMIT 10000"
            ))
            .bind(pattern)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, Deal>(&format!(
                "{DEAL_SELECT} WHERE NOT EXISTS (SELECT 1 FROM contacts c WHERE c.id = d.contact_id AND c.blocked = TRUE) ORDER BY d.created_at DESC LIMIT 10000"
            ))
            .fetch_all(&self.pool)
            .await?
        };
        Ok(deals)
    }
}

fn map_deal_row(row: sqlx::postgres::PgRow) -> Deal {
    use sqlx::Row;
    Deal {
        id: row.get("id"),
        title: row.get("title"),
        value: row.get("value"),
        currency: row.get("currency"),
        stage: row.get("stage"),
        position: row.get("position"),
        contact_id: row.get("contact_id"),
        company_id: row.get("company_id"),
        pipeline_id: row.get("pipeline_id"),
        pipeline_stage_id: row.get("pipeline_stage_id"),
        expected_close_date: row.get("expected_close_date"),
        notes: row.get("notes"),
        contact_name: row.get("contact_name"),
        company_name: row.get("company_name"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}
