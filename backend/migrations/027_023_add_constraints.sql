-- Add missing unique constraints and defaults (idempotent: M034 also guards uq_reset_token_hash)
DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'uq_reset_token_hash'
    ) THEN
        ALTER TABLE password_reset_tokens ADD CONSTRAINT uq_reset_token_hash UNIQUE (token_hash);
    END IF;
END $$;
CREATE UNIQUE INDEX IF NOT EXISTS uq_pipeline_default ON pipelines(entity_type) WHERE is_default = true;
CREATE UNIQUE INDEX IF NOT EXISTS uq_stage_default ON pipeline_stages(pipeline_id) WHERE is_default = true;
CREATE UNIQUE INDEX IF NOT EXISTS uq_stage_position ON pipeline_stages(pipeline_id, position);
ALTER TABLE documents ALTER COLUMN mime_type SET DEFAULT 'application/octet-stream';
