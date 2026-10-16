-- Migration 032: Fix notifications schema after M028 partial failure
-- The M028 migration used CREATE TABLE IF NOT EXISTS which was silently skipped
-- because the table already existed from M014. This migration ensures the table
-- has the correct schema expected by the backend (column: read BOOLEAN).

-- Add updated_at column if missing (M028 intended it but never applied)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'notifications' AND column_name = 'updated_at'
    ) THEN
        ALTER TABLE notifications ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
    END IF;
END $$;

-- Ensure trigger exists for updated_at
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trg_notifications_updated_at'
        AND tgrelid = 'notifications'::regclass
    ) THEN
        CREATE TRIGGER trg_notifications_updated_at
        BEFORE UPDATE ON notifications
        FOR EACH ROW EXECUTE FUNCTION set_updated_at();
    END IF;
END $$;

-- Drop duplicate index if both exist (M014 created idx_notifications_user,
-- M028 tried to create idx_notifications_user_id)
DROP INDEX IF EXISTS idx_notifications_user_id;

-- Ensure the correct composite index exists for unread count queries
CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications(user_id, read) WHERE read = FALSE;
