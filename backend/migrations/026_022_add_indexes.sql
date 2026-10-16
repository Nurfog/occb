-- Add missing indexes on FK and commonly-filtered columns
CREATE INDEX IF NOT EXISTS idx_deals_pipeline_stage ON deals(pipeline_stage_id);
CREATE INDEX IF NOT EXISTS idx_deals_pipeline ON deals(pipeline_id);
CREATE INDEX IF NOT EXISTS idx_users_profile ON users(profile_id);
CREATE INDEX IF NOT EXISTS idx_lead_activities_created_by ON lead_activities(created_by);
CREATE INDEX IF NOT EXISTS idx_whatsapp_messages_agent ON whatsapp_messages(agent_id);
CREATE INDEX IF NOT EXISTS idx_lead_extractions_status ON lead_extractions(status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_calendar_events_external ON calendar_events(external_id);
