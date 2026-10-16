-- 041: derechos de titulares Ley 21.719 (Chile) — solicitudes ARCO + bloqueo.
-- Idempotente (IF NOT EXISTS / ON CONFLICT DO NOTHING / DROP IF EXISTS).
-- La app usa SQL 100% parametrizado ($n + .bind); ningún input se concatena.

-- Tipos (guardados para re-ejecución segura).
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'privacy_request_type') THEN
        CREATE TYPE privacy_request_type AS ENUM (
            'acceso', 'rectificacion', 'supresion', 'oposicion', 'portabilidad', 'bloqueo'
        );
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'privacy_request_status') THEN
        CREATE TYPE privacy_request_status AS ENUM (
            'recibida', 'verificada', 'en_curso', 'cumplida', 'denegada', 'desistida'
        );
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS privacy_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contact_id UUID NULL REFERENCES contacts(id) ON DELETE SET NULL,
    email CITEXT NOT NULL,
    type privacy_request_type NOT NULL,
    status privacy_request_status NOT NULL DEFAULT 'recibida',
    motivo TEXT NULL CHECK (motivo IS NULL OR char_length(motivo) <= 2000),
    identity_verified BOOLEAN NOT NULL DEFAULT FALSE,
    verify_token_hash TEXT NULL,
    verify_expires_at TIMESTAMPTZ NULL,
    due_at TIMESTAMPTZ NOT NULL,
    extended BOOLEAN NOT NULL DEFAULT FALSE,
    extended_at TIMESTAMPTZ NULL,
    corrections JSONB NULL,
    legal_hold TEXT NULL,
    resolution TEXT NULL,
    resolved_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_privacy_requests_status_due ON privacy_requests (status, due_at);
CREATE INDEX IF NOT EXISTS idx_privacy_requests_email ON privacy_requests (email);

-- Flag de bloqueo en contactos (oposición / bloqueo legal: no tratar ni ceder).
ALTER TABLE contacts ADD COLUMN IF NOT EXISTS blocked BOOLEAN NOT NULL DEFAULT FALSE;

-- Trigger updated_at (set_updated_at() existe desde 002/040).
DROP TRIGGER IF EXISTS trg_privacy_requests_updated_at ON privacy_requests;
CREATE TRIGGER trg_privacy_requests_updated_at
    BEFORE UPDATE ON privacy_requests
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- RLS segunda capa (igual que el resto de tablas CRM: política permisiva hoy).
ALTER TABLE privacy_requests ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS app_all ON privacy_requests;
CREATE POLICY app_all ON privacy_requests FOR ALL USING (true) WITH CHECK (true);

-- Seeds RBAC (patrón 035/039: UNIQUE(profile_id, permission) + ON CONFLICT DO NOTHING).
INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('privacy.view'),
  ('privacy.manage')
) AS v(permission)
WHERE p.name = 'Administrador'
ON CONFLICT DO NOTHING;

INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('privacy.view')
) AS v(permission)
WHERE p.name = 'Vendedor'
ON CONFLICT DO NOTHING;
