-- Replace the UNIQUE constraint on message_id (which allows multiple NULLs)
-- with a partial unique index that only enforces uniqueness for non-null values.
-- Drop the constraint first: doing so also drops its backing index, so a separate
-- DROP INDEX is neither needed nor allowed (Postgres rejects dropping an index
-- that a constraint depends on).
ALTER TABLE whatsapp_messages DROP CONSTRAINT IF EXISTS uq_whatsapp_msg_id;
CREATE UNIQUE INDEX IF NOT EXISTS uq_whatsapp_msg_id_notnull ON whatsapp_messages(message_id) WHERE message_id IS NOT NULL;
