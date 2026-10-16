-- Add 'lead' to entity_tags CHECK constraint
ALTER TABLE entity_tags DROP CONSTRAINT IF EXISTS entity_tags_entity_type_check;
ALTER TABLE entity_tags ADD CONSTRAINT entity_tags_entity_type_check CHECK (entity_type IN ('contact', 'company', 'deal', 'lead'));
