-- Migration 033: Add FK constraint on email_logs.template_id
-- This column was added in M018 without a FOREIGN KEY, allowing orphaned references.

-- Null-out orphans so ADD CONSTRAINT cannot fail on dirty data.
UPDATE email_logs SET template_id = NULL
WHERE template_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM email_templates WHERE email_templates.id = email_logs.template_id);

DO $$ BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_email_logs_template'
    ) THEN
        ALTER TABLE email_logs
            ADD CONSTRAINT fk_email_logs_template
            FOREIGN KEY (template_id) REFERENCES email_templates(id)
            ON DELETE SET NULL;
    END IF;
END $$;
