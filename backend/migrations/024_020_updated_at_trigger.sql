-- Migration 020: Add updated_at trigger for all tables with updated_at column
-- This trigger automatically sets updated_at to NOW() on UPDATE.

CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Apply trigger to each table that has an updated_at column (idempotent: safe to re-run)
DO $$ DECLARE t text; triggers text[] := ARRAY[
    'users:trg_users_updated_at',
    'companies:trg_companies_updated_at',
    'documents:trg_documents_updated_at',
    'webhooks:trg_webhooks_updated_at',
    'user_integrations:trg_user_integrations_updated_at',
    'whatsapp_config:trg_whatsapp_config_updated_at',
    'lead_assignment_config:trg_lead_assignment_config_updated_at',
    'ai_config:trg_ai_config_updated_at',
    'pipelines:trg_pipelines_updated_at',
    'profiles:trg_profiles_updated_at',
    'leads:trg_leads_updated_at',
    'email_templates:trg_email_templates_updated_at',
    'calendar_tokens:trg_calendar_tokens_updated_at'
]; parts text[]; tbl text; trg text;
BEGIN
    FOREACH t IN ARRAY triggers LOOP
        parts := string_to_array(t, ':'); tbl := parts[1]; trg := parts[2];
        IF to_regclass(tbl) IS NOT NULL AND NOT EXISTS (
            SELECT 1 FROM pg_trigger WHERE tgname = trg AND tgrelid = tbl::regclass
        ) THEN
            EXECUTE format('CREATE TRIGGER %I BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION set_updated_at()', trg, tbl);
        END IF;
    END LOOP;
END $$;
