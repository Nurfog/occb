-- Superseded by M027 (partial unique index). Only apply on databases that
-- never saw M027; otherwise skip so re-runs converge instead of fighting M027.
DO $$ BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_class WHERE relname = 'uq_whatsapp_msg_id_notnull'
    ) THEN
        -- M027 already in place: nothing to do.
        NULL;
    ELSIF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'uq_whatsapp_msg_id'
    ) THEN
        ALTER TABLE whatsapp_messages ADD CONSTRAINT uq_whatsapp_msg_id UNIQUE (message_id);
    END IF;
END $$;
