-- Migration 034: Fix audit/integrity issues (idempotent, safe to re-run via psql loop)
-- Never renames existing migrations; only appends. All statements are IF NOT EXISTS-guarded.

-- Ensure trigger helper exists (originally created in M020).
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- ─── 1. calendar_events.external_id: partial UNIQUE (NULLs excluded) ─────────
-- M022 created a plain UNIQUE index which allows duplicate NULLs but still
-- indexes NULL rows. Replace with a partial index enforcing uniqueness only
-- for non-NULL external_ids (e.g. Google/Microsoft sync ids).
CREATE UNIQUE INDEX IF NOT EXISTS idx_calendar_events_external_notnull
    ON calendar_events(external_id) WHERE external_id IS NOT NULL;
DROP INDEX IF EXISTS idx_calendar_events_external;

-- ─── 2a. password_reset_tokens.token_hash UNIQUE (M023 was non-idempotent) ───
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'uq_reset_token_hash'
    ) THEN
        ALTER TABLE password_reset_tokens
            ADD CONSTRAINT uq_reset_token_hash UNIQUE (token_hash);
    END IF;
END $$;

-- ─── 2b. deals → pipelines / pipeline_stages FKs ON DELETE SET NULL ──────────
-- M010 added bare REFERENCES (default NO ACTION, auto-named
-- deals_pipeline_id_fkey / deals_pipeline_stage_id_fkey). Re-create as named
-- constraints with SET NULL so deleting a pipeline/stage orphans nothing.
-- First null-out any orphaned references so ADD CONSTRAINT cannot fail.
UPDATE deals SET pipeline_id = NULL
WHERE pipeline_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM pipelines WHERE pipelines.id = deals.pipeline_id);
UPDATE deals SET pipeline_stage_id = NULL
WHERE pipeline_stage_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM pipeline_stages WHERE pipeline_stages.id = deals.pipeline_stage_id);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_deals_pipeline'
    ) THEN
        ALTER TABLE deals DROP CONSTRAINT IF EXISTS deals_pipeline_id_fkey;
        ALTER TABLE deals ADD CONSTRAINT fk_deals_pipeline
            FOREIGN KEY (pipeline_id) REFERENCES pipelines(id) ON DELETE SET NULL;
    END IF;
END $$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_deals_pipeline_stage'
    ) THEN
        ALTER TABLE deals DROP CONSTRAINT IF EXISTS deals_pipeline_stage_id_fkey;
        ALTER TABLE deals ADD CONSTRAINT fk_deals_pipeline_stage
            FOREIGN KEY (pipeline_stage_id) REFERENCES pipeline_stages(id) ON DELETE SET NULL;
    END IF;
END $$;

-- ─── 3. updated_at columns + triggers for tables missing them ────────────────
-- These tables have no updated_at column yet (verified in M008/M010/M012/M018),
-- so add the column first (032 pattern), then the trigger. Fully idempotent.

-- pipeline_stages
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'pipeline_stages' AND column_name = 'updated_at'
    ) THEN
        ALTER TABLE pipeline_stages ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
    END IF;
END $$;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trg_pipeline_stages_updated_at'
        AND tgrelid = 'pipeline_stages'::regclass
    ) THEN
        CREATE TRIGGER trg_pipeline_stages_updated_at
        BEFORE UPDATE ON pipeline_stages
        FOR EACH ROW EXECUTE FUNCTION set_updated_at();
    END IF;
END $$;

-- tags
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'tags' AND column_name = 'updated_at'
    ) THEN
        ALTER TABLE tags ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
    END IF;
END $$;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trg_tags_updated_at'
        AND tgrelid = 'tags'::regclass
    ) THEN
        CREATE TRIGGER trg_tags_updated_at
        BEFORE UPDATE ON tags
        FOR EACH ROW EXECUTE FUNCTION set_updated_at();
    END IF;
END $$;

-- entity_tags
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'entity_tags' AND column_name = 'updated_at'
    ) THEN
        ALTER TABLE entity_tags ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
    END IF;
END $$;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trg_entity_tags_updated_at'
        AND tgrelid = 'entity_tags'::regclass
    ) THEN
        CREATE TRIGGER trg_entity_tags_updated_at
        BEFORE UPDATE ON entity_tags
        FOR EACH ROW EXECUTE FUNCTION set_updated_at();
    END IF;
END $$;

-- agent_lead_assignments
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'agent_lead_assignments' AND column_name = 'updated_at'
    ) THEN
        ALTER TABLE agent_lead_assignments ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
    END IF;
END $$;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trg_agent_lead_assignments_updated_at'
        AND tgrelid = 'agent_lead_assignments'::regclass
    ) THEN
        CREATE TRIGGER trg_agent_lead_assignments_updated_at
        BEFORE UPDATE ON agent_lead_assignments
        FOR EACH ROW EXECUTE FUNCTION set_updated_at();
    END IF;
END $$;

-- whatsapp_messages
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'whatsapp_messages' AND column_name = 'updated_at'
    ) THEN
        ALTER TABLE whatsapp_messages ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
    END IF;
END $$;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trg_whatsapp_messages_updated_at'
        AND tgrelid = 'whatsapp_messages'::regclass
    ) THEN
        CREATE TRIGGER trg_whatsapp_messages_updated_at
        BEFORE UPDATE ON whatsapp_messages
        FOR EACH ROW EXECUTE FUNCTION set_updated_at();
    END IF;
END $$;

-- email_logs
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'email_logs' AND column_name = 'updated_at'
    ) THEN
        ALTER TABLE email_logs ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
    END IF;
END $$;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trg_email_logs_updated_at'
        AND tgrelid = 'email_logs'::regclass
    ) THEN
        CREATE TRIGGER trg_email_logs_updated_at
        BEFORE UPDATE ON email_logs
        FOR EACH ROW EXECUTE FUNCTION set_updated_at();
    END IF;
END $$;

-- ─── 4. Backfill documents.mime_type NULLs ───────────────────────────────────
UPDATE documents SET mime_type = 'application/octet-stream' WHERE mime_type IS NULL;

-- ─── 5. entity_type CHECK constraints: intentionally OMITTED ─────────────────
-- entity_tags already has CHECK (contact, company, deal, lead) via M012+M017.
-- calendar_events.entity_type and email_logs.entity_type are free-form and may
-- hold legacy values; adding a CHECK now could fail on existing rows. If a new
-- table needing an entity_type CHECK is added later, define it inline in its
-- CREATE TABLE statement.
