-- 043: registro probatorio de consentimientos Ley 21.719 (opt-in marketing/analítica).
-- Idempotente (IF NOT EXISTS). Cada decisión es una fila (historial: la última
-- por created_at es la vigente; revocar = insertar todo-falso).
-- La app usa SQL 100% parametrizado ($n + .bind); ningún input se concatena.

CREATE TABLE IF NOT EXISTS privacy_consents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    marketing BOOLEAN NOT NULL DEFAULT FALSE,
    analytics BOOLEAN NOT NULL DEFAULT FALSE,
    version INTEGER NOT NULL DEFAULT 1 CHECK (version BETWEEN 1 AND 99),
    source TEXT NOT NULL DEFAULT 'app'
        CHECK (char_length(source) BETWEEN 1 AND 20),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_privacy_consents_user
    ON privacy_consents (user_id, created_at DESC);

-- RLS segunda capa (igual que el resto de tablas: política permisiva hoy).
ALTER TABLE privacy_consents ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS app_all ON privacy_consents;
CREATE POLICY app_all ON privacy_consents FOR ALL USING (true) WITH CHECK (true);
