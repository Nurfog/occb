CREATE TABLE IF NOT EXISTS notification_preferences (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    email_enabled BOOLEAN NOT NULL DEFAULT true,
    push_enabled BOOLEAN NOT NULL DEFAULT true,
    weekly_digest BOOLEAN NOT NULL DEFAULT false,
    marketing_emails BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

DO $$ BEGIN
    IF to_regclass('notification_preferences') IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trg_notification_preferences_updated_at'
        AND tgrelid = 'notification_preferences'::regclass
    ) THEN
        CREATE TRIGGER trg_notification_preferences_updated_at BEFORE UPDATE ON notification_preferences
            FOR EACH ROW EXECUTE FUNCTION set_updated_at();
    END IF;
END $$;
