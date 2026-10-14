-- 002: usuarios + tokens de refresco (idempotente).
-- Anti-inyección: el backend SOLO usa consultas parametrizadas ($1, $2...);
-- ningún input de usuario se concatena en SQL (ver backend/src/handlers).

CREATE TABLE IF NOT EXISTS users (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    email         CITEXT      NOT NULL UNIQUE CHECK (char_length(email::text) <= 320),
    name          TEXT        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    password_hash TEXT        NOT NULL,
    -- Reservada para búsqueda semántica futura con pgvector. Sin índice hasta
    -- tener datos (elegir HNSW/IVFFlat entonces).
    embedding     vector(1536),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Refresh tokens opacos: en DB solo el SHA-256 (nunca el token en claro).
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash TEXT        NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user ON refresh_tokens (user_id);

CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_users_updated_at ON users;
CREATE TRIGGER trg_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
