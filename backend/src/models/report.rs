//! Modelos de reportes agregados (solo lectura).

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PipelineReportItem {
    pub stage: String,
    pub count: i64,
    pub total_value: f64,
    pub avg_value: f64,
    pub percentage: f64,
}

#[derive(Debug, Serialize)]
pub struct PipelineReport {
    pub stages: Vec<PipelineReportItem>,
    pub total_deals: i64,
    pub total_value: f64,
}

#[derive(Debug, Serialize)]
pub struct WinLossReport {
    pub won_count: i64,
    pub lost_count: i64,
    pub win_rate: f64,
    pub loss_rate: f64,
    pub won_value: f64,
    pub lost_value: f64,
    pub total_closed: i64,
}
