ALTER TABLE deals ADD COLUMN IF NOT EXISTS position INTEGER DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_deals_stage_position ON deals(stage, position);
