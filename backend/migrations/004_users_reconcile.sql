-- 004: reconcilia users plantilla (name) con esquema CRM (first/last/role/profile).
-- Idempotente. Mantiene `name` para compat con endpoints actuales.

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

ALTER TABLE users ADD COLUMN IF NOT EXISTS first_name TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS last_name TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS role TEXT DEFAULT 'user';
ALTER TABLE users ADD COLUMN IF NOT EXISTS profile_id UUID;

-- Backfill desde `name` ("Nombre Apellido" -> first/last) solo donde faltan.
UPDATE users
SET first_name = COALESCE(first_name, split_part(name, ' ', 1)),
    last_name = COALESCE(last_name, NULLIF(substr(name, strpos(name, ' ') + 1), ''))
WHERE (first_name IS NULL OR last_name IS NULL) AND name IS NOT NULL;

-- A partir de aquí first/last obligatorios para filas nuevas (las viejas ya tienen).
-- No ponemos NOT NULL global para no romper filas legacy sin apellido.
