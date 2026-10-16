-- Add recurrence fields to activities
ALTER TABLE activities ADD COLUMN IF NOT EXISTS recurrence_type VARCHAR(20) DEFAULT 'none';
ALTER TABLE activities ADD COLUMN IF NOT EXISTS recurrence_interval INTEGER DEFAULT 1;
ALTER TABLE activities ADD COLUMN IF NOT EXISTS recurrence_end_date DATE;
ALTER TABLE activities ADD COLUMN IF NOT EXISTS parent_activity_id UUID REFERENCES activities(id) ON DELETE SET NULL;

-- Recurrence types: none, daily, weekly, monthly, yearly
CREATE INDEX IF NOT EXISTS idx_activities_parent ON activities(parent_activity_id) WHERE parent_activity_id IS NOT NULL;
