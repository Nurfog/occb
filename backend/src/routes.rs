//! Tabla de rutas del API (port completo opencrmccb + health/ready/version).
//!
//! - Públicas: health, ready, version, branding, whatsapp webhook, oauth
//!   callback, auth login/register/refresh/forgot/reset, solicitudes de
//!   derechos Ley 21.719 (estas últimas con rate-limit estricto 10/min).
//! - Protegidas: todo el CRM con JWT (`auth_middleware`), incluida la
//!   gestión de solicitudes Ley 21.719 (`privacy.view`/`privacy.manage`).
//! - Admin: users + webhooks + pipelines/perfiles/branding con
//!   `auth_middleware` + `admin_only_middleware` (exige `admin.access`).
//! - `/metrics` NO se expone (ver middleware/metrics stub).

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{delete, get, patch, post, put},
};

use crate::{handlers, middleware, state::AppState};

pub fn router(state: AppState) -> Router {
    // ── Públicas (sin JWT) ──
    let public = Router::new()
        .route("/", get(handlers::health::health))
        .route("/api/v1/health", get(handlers::health::health))
        .route("/api/v1/ready", get(handlers::ready::ready))
        .route("/api/v1/version", get(handlers::health::version))
        .route("/api/v1/branding", get(handlers::admin::get_branding))
        .route(
            "/api/v1/integrations/whatsapp/webhook",
            get(handlers::whatsapp::webhook_verify),
        )
        .route(
            "/api/v1/integrations/whatsapp/webhook",
            post(handlers::whatsapp::webhook_receive),
        )
        .route(
            "/api/v1/integrations/{provider}/callback",
            get(handlers::oauth::callback),
        )
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route(
            "/api/v1/auth/forgot-password",
            post(handlers::auth_ext::forgot_password),
        )
        .route(
            "/api/v1/auth/reset-password",
            post(handlers::auth_ext::reset_password),
        )
        .route("/api/v1/auth/register", post(handlers::auth::register))
        .route("/api/v1/auth/refresh", post(handlers::auth::refresh));

    // ── Públicas Ley 21.719 (sin JWT, rate-limit estricto anti-spam) ──
    let privacy_public = Router::new()
        .route(
            "/api/v1/privacy/requests",
            post(handlers::privacy::create_request),
        )
        .route(
            "/api/v1/privacy/requests/verify",
            post(handlers::privacy::verify_request),
        )
        .layer(from_fn_with_state(
            state.clone(),
            middleware::rate_limit::strict_rate_limit_middleware,
        ));

    // ── Protegidas (JWT) ──
    let protected = Router::new()
        .route("/api/v1/search", get(handlers::search::global_search))
        // Tags
        .route(
            "/api/v1/tags",
            get(handlers::tags::list_tags).post(handlers::tags::create_tag),
        )
        .route(
            "/api/v1/tags/{id}",
            put(handlers::tags::update_tag).delete(handlers::tags::delete_tag),
        )
        .route("/api/v1/tags/assign", post(handlers::tags::assign_tag))
        .route(
            "/api/v1/tags/{tag_id}/{entity_type}/{entity_id}",
            delete(handlers::tags::remove_tag),
        )
        .route(
            "/api/v1/tags/{entity_type}/{entity_id}",
            get(handlers::tags::get_entity_tags),
        )
        // Contacts
        .route(
            "/api/v1/contacts",
            get(handlers::contacts::list_contacts).post(handlers::contacts::create_contact),
        )
        .route(
            "/api/v1/contacts/export",
            get(handlers::contacts::export_contacts),
        )
        .route(
            "/api/v1/contacts/import",
            post(handlers::contacts::import_contacts),
        )
        .route(
            "/api/v1/contacts/bulk-delete",
            post(handlers::contacts::bulk_delete_contacts),
        )
        .route(
            "/api/v1/contacts/{id}",
            get(handlers::contacts::get_contact)
                .put(handlers::contacts::update_contact)
                .delete(handlers::contacts::delete_contact),
        )
        .route(
            "/api/v1/contacts/{id}/deals",
            get(handlers::related::get_contact_deals),
        )
        .route(
            "/api/v1/contacts/{id}/activities",
            get(handlers::related::get_contact_activities),
        )
        // Companies
        .route(
            "/api/v1/companies",
            get(handlers::companies::list_companies).post(handlers::companies::create_company),
        )
        .route(
            "/api/v1/companies/export",
            get(handlers::companies::export_companies),
        )
        .route(
            "/api/v1/companies/import",
            post(handlers::companies::import_companies),
        )
        .route(
            "/api/v1/companies/{id}",
            get(handlers::companies::get_company)
                .put(handlers::companies::update_company)
                .delete(handlers::companies::delete_company),
        )
        .route(
            "/api/v1/companies/{id}/contacts",
            get(handlers::related::get_company_contacts),
        )
        .route(
            "/api/v1/companies/{id}/deals",
            get(handlers::related::get_company_deals),
        )
        .route(
            "/api/v1/companies/{id}/revenue",
            get(handlers::related::get_company_revenue),
        )
        // Deals
        .route(
            "/api/v1/deals",
            get(handlers::deals::list_deals).post(handlers::deals::create_deal),
        )
        .route("/api/v1/deals/export", get(handlers::deals::export_deals))
        .route("/api/v1/deals/import", post(handlers::deals::import_deals))
        .route(
            "/api/v1/deals/{id}",
            get(handlers::deals::get_deal)
                .put(handlers::deals::update_deal)
                .delete(handlers::deals::delete_deal),
        )
        .route(
            "/api/v1/deals/{id}/stage",
            patch(handlers::deals::update_deal_stage),
        )
        .route(
            "/api/v1/deals/{id}/activities",
            get(handlers::related::get_deal_activities),
        )
        // Activities
        .route(
            "/api/v1/activities",
            get(handlers::activities::list_activities).post(handlers::activities::create_activity),
        )
        .route(
            "/api/v1/activities/{id}",
            put(handlers::activities::update_activity)
                .delete(handlers::activities::delete_activity),
        )
        .route(
            "/api/v1/activities/{id}/complete",
            post(handlers::activities::complete_activity),
        )
        // Dashboard
        .route(
            "/api/v1/dashboard/stats",
            get(handlers::dashboard::get_dashboard_stats),
        )
        .route(
            "/api/v1/dashboard/pipeline",
            get(handlers::dashboard::get_pipeline),
        )
        .route(
            "/api/v1/dashboard/top-deals",
            get(handlers::dashboard::get_top_deals),
        )
        .route(
            "/api/v1/dashboard/recent-activities",
            get(handlers::dashboard::get_recent_activities),
        )
        // Documents
        .route(
            "/api/v1/documents",
            get(handlers::documents::list_documents).post(handlers::documents::create_document),
        )
        .route(
            "/api/v1/documents/upload",
            post(handlers::documents::upload_document),
        )
        .route(
            "/api/v1/documents/{id}/download",
            get(handlers::documents::download_document),
        )
        .route(
            "/api/v1/documents/{id}",
            get(handlers::documents::get_document).delete(handlers::documents::delete_document),
        )
        // Audit
        .route("/api/v1/audit", get(handlers::audit::list_audit_logs))
        .route(
            "/api/v1/audit/entity/{type}/{id}",
            get(handlers::audit::get_entity_history),
        )
        // Privacidad Ley 21.719 (gestión staff; la creación/verificación
        // pública vive en `privacy_public` con rate-limit estricto)
        .route(
            "/api/v1/privacy/requests",
            get(handlers::privacy::list_requests),
        )
        .route(
            "/api/v1/privacy/requests/stats",
            get(handlers::privacy::request_stats),
        )
        .route(
            "/api/v1/privacy/requests/{id}",
            get(handlers::privacy::get_request).patch(handlers::privacy::update_request),
        )
        // Profile (contrato origen)
        .route("/api/v1/auth/me", get(handlers::auth_ext::get_profile))
        .route(
            "/api/v1/auth/profile",
            put(handlers::auth_ext::update_profile),
        )
        .route(
            "/api/v1/auth/password",
            put(handlers::auth_ext::change_password),
        )
        .route("/api/v1/auth/logout", post(handlers::auth::logout))
        // Legacy occb (compat): detalle de usuario
        .route("/api/v1/users/{id}", get(handlers::users::get_user))
        // Notifications
        .route(
            "/api/v1/notifications/test",
            post(handlers::notifications::send_test_email),
        )
        .route(
            "/api/v1/notifications/preferences",
            get(handlers::notifications::get_notification_preferences)
                .put(handlers::notifications::update_notification_preferences),
        )
        .route(
            "/api/v1/notifications",
            get(handlers::notifications::list_notifications),
        )
        .route(
            "/api/v1/notifications/unread-count",
            get(handlers::notifications::get_unread_count),
        )
        .route(
            "/api/v1/notifications/{id}/read",
            put(handlers::notifications::mark_as_read),
        )
        .route(
            "/api/v1/notifications/read-all",
            put(handlers::notifications::mark_all_as_read),
        )
        .route(
            "/api/v1/notifications/{id}",
            delete(handlers::notifications::delete_notification),
        )
        // Reports
        .route(
            "/api/v1/reports/pipeline",
            get(handlers::reports::get_pipeline_report),
        )
        .route(
            "/api/v1/reports/win-loss",
            get(handlers::reports::get_win_loss_report),
        )
        // Email
        .route("/api/v1/email/send", post(handlers::email::send_email))
        .route("/api/v1/email/logs", get(handlers::email::list_email_logs))
        .route(
            "/api/v1/email/logs/{id}",
            get(handlers::email::get_email_log),
        )
        .route(
            "/api/v1/email/templates",
            get(handlers::email::list_templates).post(handlers::email::create_template),
        )
        .route(
            "/api/v1/email/templates/{id}",
            put(handlers::email::update_template).delete(handlers::email::delete_template),
        )
        .route(
            "/api/v1/email/send-from-template/{template_id}",
            post(handlers::email::send_from_template),
        )
        // Calendar
        .route(
            "/api/v1/calendar/status",
            get(handlers::calendar::connection_status),
        )
        .route(
            "/api/v1/calendar/events",
            get(handlers::calendar::list_events).post(handlers::calendar::create_event),
        )
        .route(
            "/api/v1/calendar/events/{id}",
            put(handlers::calendar::update_event).delete(handlers::calendar::delete_event),
        )
        .route(
            "/api/v1/calendar/auth/{provider}",
            get(handlers::calendar::get_auth_url),
        )
        .route(
            "/api/v1/calendar/{provider}/callback",
            get(handlers::calendar::google_callback),
        )
        .route(
            "/api/v1/calendar/microsoft/callback",
            get(handlers::calendar::microsoft_callback),
        )
        .route(
            "/api/v1/calendar/sync/google",
            post(handlers::calendar::sync_google),
        )
        // Pipelines (solo lectura)
        .route("/api/v1/pipelines", get(handlers::admin::list_pipelines))
        // Integrations
        .route(
            "/api/v1/integrations",
            get(handlers::oauth::list_integrations),
        )
        .route(
            "/api/v1/integrations/{provider}/connect",
            get(handlers::oauth::connect),
        )
        .route(
            "/api/v1/integrations/{provider}/disconnect",
            delete(handlers::oauth::disconnect),
        )
        // WhatsApp
        .route(
            "/api/v1/integrations/whatsapp/config",
            get(handlers::whatsapp::get_whatsapp_config)
                .put(handlers::whatsapp::update_whatsapp_config),
        )
        .route(
            "/api/v1/integrations/whatsapp/send",
            post(handlers::whatsapp::send_whatsapp_message),
        )
        .route(
            "/api/v1/integrations/whatsapp/messages",
            get(handlers::whatsapp::get_messages),
        )
        .route(
            "/api/v1/integrations/whatsapp/conversations",
            get(handlers::whatsapp::list_conversations),
        )
        // Lead assignment
        .route(
            "/api/v1/settings/lead-assignment",
            get(handlers::whatsapp::get_lead_assignment_config)
                .put(handlers::whatsapp::update_lead_assignment_config),
        )
        .route(
            "/api/v1/settings/lead-assignment/assign",
            post(handlers::whatsapp::assign_lead),
        )
        // Leads
        .route(
            "/api/v1/leads",
            get(handlers::leads::list_leads).post(handlers::leads::create_lead),
        )
        .route("/api/v1/leads/stats", get(handlers::leads::lead_stats))
        .route(
            "/api/v1/leads/{id}",
            get(handlers::leads::get_lead)
                .put(handlers::leads::update_lead)
                .delete(handlers::leads::delete_lead),
        )
        .route(
            "/api/v1/leads/{id}/convert",
            post(handlers::leads::convert_lead),
        )
        .route(
            "/api/v1/leads/{id}/activities",
            get(handlers::leads::list_lead_activities).post(handlers::leads::create_lead_activity),
        )
        .route(
            "/api/v1/leads/{lead_id}/activities/{activity_id}",
            post(handlers::leads::complete_lead_activity)
                .delete(handlers::leads::delete_lead_activity),
        )
        // AI
        .route(
            "/api/v1/settings/ai",
            get(handlers::ai::get_ai_config).put(handlers::ai::update_ai_config),
        )
        .route("/api/v1/leads/extract", post(handlers::ai::extract_lead))
        .route(
            "/api/v1/leads/extractions",
            get(handlers::ai::get_extractions),
        )
        .layer(from_fn_with_state(
            state.clone(),
            middleware::auth::auth_middleware,
        ));

    // ── Admin (JWT + admin.access) ──
    let admin = Router::new()
        .route(
            "/api/v1/users",
            get(handlers::admin::admin_list_users).post(handlers::admin::admin_create_user),
        )
        .route(
            "/api/v1/users/{id}",
            delete(handlers::admin::admin_delete_user),
        )
        .route(
            "/api/v1/users/{id}/profile",
            put(handlers::admin::admin_update_user),
        )
        .route(
            "/api/v1/admin/pipelines",
            post(handlers::admin::create_pipeline),
        )
        .route(
            "/api/v1/admin/pipelines/{id}",
            put(handlers::admin::update_pipeline).delete(handlers::admin::delete_pipeline),
        )
        .route(
            "/api/v1/admin/pipelines/{pipeline_id}/stages",
            post(handlers::admin::create_stage),
        )
        .route(
            "/api/v1/admin/pipelines/{pipeline_id}/stages/{stage_id}",
            delete(handlers::admin::delete_stage),
        )
        .route(
            "/api/v1/admin/profiles",
            get(handlers::admin::list_profiles).post(handlers::admin::create_profile),
        )
        .route(
            "/api/v1/admin/profiles/{id}",
            put(handlers::admin::update_profile),
        )
        .route(
            "/api/v1/admin/branding",
            put(handlers::admin::update_branding),
        )
        .route(
            "/api/v1/webhooks",
            get(handlers::webhooks::list_webhooks).post(handlers::webhooks::create_webhook),
        )
        .route(
            "/api/v1/webhooks/{id}",
            patch(handlers::webhooks::update_webhook).delete(handlers::webhooks::delete_webhook),
        )
        .route(
            "/api/v1/webhooks/{id}/deliveries",
            get(handlers::webhooks::list_deliveries),
        )
        .layer(from_fn_with_state(
            state.clone(),
            middleware::auth_ext::admin_only_middleware,
        ))
        .layer(from_fn_with_state(
            state.clone(),
            middleware::auth::auth_middleware,
        ));

    public
        .merge(privacy_public)
        .merge(protected)
        .merge(admin)
        .layer(from_fn_with_state(
            state.clone(),
            middleware::rate_limit::rate_limit_middleware,
        ))
        .with_state(state)
}
