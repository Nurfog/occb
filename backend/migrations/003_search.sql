-- 003: índices de búsqueda (idempotente).
-- Búsqueda con ILIKE parametrizado + escape de %, _, \ en el backend.
CREATE INDEX IF NOT EXISTS idx_users_email_trgm
    ON users USING gin ((email::text) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_users_name_trgm
    ON users USING gin (name gin_trgm_ops);
