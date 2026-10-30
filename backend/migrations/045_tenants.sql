-- 045: SaaS multi-empresa (tenants) + superadmin.
--
-- Modelo: el root instalado es superadmin (users.is_superadmin, platform
-- level, tenant_id NULL). Cada empresa cliente es un `tenant` con estado
-- (pendiente/activa/suspendida); sus usuarios cuelgan de tenant_id y sus
-- admins se crean desde el dashboard del superadmin.
-- Acceso: un tenant suspendido bloquea login y API de sus usuarios (ver
-- auth_middleware). Fase 2 (aislamiento por filas del CRM): ver
-- docs/saas-tenants.md; en esta fase los datos CRM siguen globales.

CREATE TABLE IF NOT EXISTS tenants (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 200),
    slug        TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9-]{2,40}$'),
    status      TEXT NOT NULL DEFAULT 'pendiente'
        CHECK (status IN ('pendiente', 'activa', 'suspendida')),
    contact_name TEXT NULL,
    contact_email CITEXT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_tenants_status ON tenants (status);

DROP TRIGGER IF EXISTS trg_tenants_updated_at ON tenants;
CREATE TRIGGER trg_tenants_updated_at
    BEFORE UPDATE ON tenants
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE users ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants (id) ON DELETE SET NULL;
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_superadmin BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX IF NOT EXISTS idx_users_tenant ON users (tenant_id);

-- Licencias atadas opcionalmente a un tenant (registro; la exigencia sigue
-- siendo a nivel despliegue hasta la fase 2).
ALTER TABLE licenses ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants (id) ON DELETE SET NULL;

-- Backfill: el usuario más antiguo (el root instalado) es superadmin.
-- Regla a futuro: create_first_admin lo fija en código.
UPDATE users SET is_superadmin = TRUE
WHERE id = (SELECT id FROM users ORDER BY created_at LIMIT 1);

ALTER TABLE tenants ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS app_all ON tenants;
CREATE POLICY app_all ON tenants FOR ALL USING (true) WITH CHECK (true);
