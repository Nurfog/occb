-- Drop orphaned notification_type enum created by M028 (never used, table uses M014 schema)
DROP TYPE IF EXISTS notification_type;

-- Add index on contacts.phone for WhatsApp inbound message lookups
CREATE INDEX IF NOT EXISTS idx_contacts_phone ON contacts(phone);
