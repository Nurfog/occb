//! Utilidades CSV: escape anti-inyección + import de deals (grupo A, port adaptado).
//!
//! Adaptación: el origen usa la crate `csv` (no disponible en la plantilla
//! destino y sin dependencias nuevas permitidas), así que `parse_csv_rows`
//! es un parser RFC 4180 mínimo propio (comillas, `""` escapado, `\n` dentro
//! de comillas). La primera fila se trata como cabecera y se salta, igual
//! que hacía el lector del origen.
//! `DealImportRow.value` es `f64` (ver `models::deal`).

use serde::Serialize;
use uuid::Uuid;

use super::deal::DealStage;

#[derive(Serialize)]
pub struct ImportResult {
    pub imported: u32,
    pub errors: Vec<String>,
}

#[derive(Debug)]
pub struct DealImportRow {
    pub title: String,
    pub value: f64,
    pub currency: String,
    pub stage: DealStage,
    pub contact_id: Option<Uuid>,
    pub company_id: Option<Uuid>,
    pub expected_close_date: Option<chrono::DateTime<chrono::Utc>>,
    pub notes: Option<String>,
}

pub fn escape_csv(value: &str) -> String {
    // OWASP CSV Injection: una celda que empieza por = + - @ (tras espacios/
    // tabs iniciales, que las hojas recortan) se evalúa como fórmula.
    // Prefijar una comilla simple lo neutraliza. Aplicar antes del
    // entrecomillado RFC 4180 de abajo.
    let needs_prefix = matches!(
        value.trim_start_matches([' ', '\t']).chars().next(),
        Some('=') | Some('+') | Some('-') | Some('@')
    );
    let value = if needs_prefix {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

/// Parser CSV mínimo (sin dependencias): filas con campos entrecomillados,
/// `""` como comilla literal y saltos de línea dentro de comillas.
/// La primera fila no vacía se considera cabecera y se omite del resultado.
pub fn parse_csv_rows(content: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = content.chars().peekable();

    macro_rules! end_field {
        () => {
            row.push(std::mem::take(&mut field));
        };
    }
    macro_rules! end_row {
        () => {
            end_field!();
            // Descarta líneas en blanco (todos los campos vacíos); conserva
            // filas con contenido aunque sea solo separadores con datos.
            if row.iter().any(|f| !f.is_empty()) {
                rows.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        };
    }

    while let Some(c) = chars.next() {
        if c != '\n' && c != '\r' && c != '"' && c != ',' {
            // Camino rápido: literal (incluye el resto del campo sin comillas).
            field.push(c);
            continue;
        }
        match c {
            '"' => {
                if in_quotes {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                } else if field.is_empty() {
                    in_quotes = true;
                } else {
                    field.push('"');
                }
            }
            ',' => {
                if in_quotes {
                    field.push(',');
                } else {
                    end_field!();
                }
            }
            '\r' => {
                if in_quotes {
                    field.push('\r');
                } else {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    end_row!();
                }
            }
            '\n' => {
                if in_quotes {
                    field.push('\n');
                } else {
                    end_row!();
                }
            }
            _ => {
                field.push(c);
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        end_row!();
    }

    // Saltar la cabecera (primera fila), como hacía el lector del origen.
    if rows.is_empty() {
        rows
    } else {
        rows.into_iter().skip(1).collect()
    }
}

pub fn parse_deal_import_row(fields: &[String]) -> Result<DealImportRow, &'static str> {
    if fields.len() < 3 {
        return Err("formato inválido");
    }
    let title = fields[0].trim().to_string();
    if title.is_empty() {
        return Err("título vacío");
    }
    let value = fields[1]
        .trim()
        .parse::<f64>()
        .map_err(|_| "valor inválido")?;
    if !value.is_finite() || value < 0.0 {
        return Err("valor inválido");
    }

    Ok(DealImportRow {
        title,
        value,
        currency: optional_field(fields, 2).unwrap_or_else(|| "USD".to_string()),
        stage: fields
            .get(3)
            .map(|field| parse_deal_stage(field))
            .unwrap_or(DealStage::Lead),
        contact_id: optional_field(fields, 4).and_then(|value| value.parse::<Uuid>().ok()),
        company_id: optional_field(fields, 5).and_then(|value| value.parse::<Uuid>().ok()),
        expected_close_date: optional_field(fields, 6)
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
            .map(|date| date.with_timezone(&chrono::Utc)),
        notes: optional_field(fields, 7),
    })
}

fn optional_field(fields: &[String], index: usize) -> Option<String> {
    fields
        .get(index)
        .map(|field| field.trim().to_string())
        .filter(|field| !field.is_empty())
}

fn parse_deal_stage(value: &str) -> DealStage {
    match value.trim() {
        "lead" | "Lead" => DealStage::Lead,
        "qualified" | "Qualified" => DealStage::Qualified,
        "proposal" | "Proposal" => DealStage::Proposal,
        "negotiation" | "Negotiation" => DealStage::Negotiation,
        "closed_won" | "Closed Won" | "closedwon" => DealStage::ClosedWon,
        "closed_lost" | "Closed Lost" | "closedlost" => DealStage::ClosedLost,
        _ => DealStage::Lead,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_salta_cabecera_y_respeta_comillas() {
        let content =
            "first_name,last_name,email\nJohn,Doe,john@x.com\n\"Jane, Jr\",Doe,jane@x.com\n";
        let rows = parse_csv_rows(content);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["John", "Doe", "john@x.com"]);
        assert_eq!(rows[1], vec!["Jane, Jr", "Doe", "jane@x.com"]);
    }

    #[test]
    fn parser_ignora_lineas_en_blanco() {
        let content = "a,b\n\n1,2\n";
        let rows = parse_csv_rows(content);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], vec!["1", "2"]);
    }

    #[test]
    fn escape_csv_neutraliza_formulas() {
        assert_eq!(escape_csv("=1+1"), "'=1+1");
        assert_eq!(escape_csv("a,b"), "\"a,b\"");
        assert_eq!(escape_csv("normal"), "normal");
    }
}
