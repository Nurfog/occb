-- 044: módulos estilo DLC + licencias por key (SaaS).
--
-- Modelo: el despliegue trae un catálogo de módulos (`modules`). Los módulos
-- base (`crm`, `privacy`) van incluidos; los DLC (p.ej. `flow`) exigen una
-- licencia activa que los cubra. Las rutas de un DLC se protegen con el
-- middleware `require_module(slug)` + JWT.
--
-- - `licenses.key_hash`: solo SHA-256 (la key en claro se muestra UNA vez al
--   emitirla, igual que los refresh tokens). Formato `occm_<64 hex>`.
-- - `licenses.modules`: slugs cubiertos; `'*'` = todos (plan total).
-- - Sin licencia válida para el módulo -> 402 `module_locked` (el frontend
--   muestra estado "DLC bloqueado", no un 403 genérico).

CREATE TABLE IF NOT EXISTS modules (
    slug        TEXT PRIMARY KEY CHECK (slug ~ '^[a-z0-9-]{2,40}$'),
    name        TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    description TEXT NOT NULL DEFAULT '',
    version     TEXT NOT NULL DEFAULT '0.1.0',
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    route_prefix TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS licenses (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    label       TEXT NOT NULL CHECK (char_length(label) BETWEEN 1 AND 200),
    key_hash    TEXT NOT NULL UNIQUE,
    key_prefix  TEXT NOT NULL,
    modules     TEXT[] NOT NULL DEFAULT '{}',
    seats       INT NULL CHECK (seats IS NULL OR seats > 0),
    expires_at  TIMESTAMPTZ NULL,
    revoked_at  TIMESTAMPTZ NULL,
    metadata    JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_licenses_modules ON licenses USING GIN (modules);

DROP TRIGGER IF EXISTS trg_modules_updated_at ON modules;
CREATE TRIGGER trg_modules_updated_at
    BEFORE UPDATE ON modules
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS trg_licenses_updated_at ON licenses;
CREATE TRIGGER trg_licenses_updated_at
    BEFORE UPDATE ON licenses
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

ALTER TABLE modules ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS app_all ON modules;
CREATE POLICY app_all ON modules FOR ALL USING (true) WITH CHECK (true);
ALTER TABLE licenses ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS app_all ON licenses;
CREATE POLICY app_all ON licenses FOR ALL USING (true) WITH CHECK (true);

-- Catálogo base: crm + privacy incluidos; flow registrado pero apagado
-- (se enciende al integrar el módulo, ver docs/modules/flow-integration-plan.md).
INSERT INTO modules (slug, name, description, version, enabled, route_prefix)
VALUES
    ('crm', 'CRM base', 'Contactos, compañías, deals, actividades y reportes.', '0.1.0', TRUE, '/api/v1'),
    ('privacy', 'Privacidad Ley 21.719', 'Derechos de titulares, consentimientos y bloqueos.', '0.1.0', TRUE, '/api/v1/privacy'),
    ('flow', 'Automatizaciones Flow', 'Clon estilo n8n: flujos, nodos y ejecuciones.', '0.1.0', FALSE, '/api/v1/flow')
ON CONFLICT (slug) DO NOTHING;
