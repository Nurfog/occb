-- 040: hardening de seguridad DB (opción elegida: SQLx + funciones, SIN ORM).
--
-- Decisión: NO se introduce ORM (SeaORM/Diesel). Motivos:
--  1. Origen y destino ya usan SQLx parametrizado ($1 + .bind), auditable línea
--     a línea. Un ORM no añade seguridad sobre binds correctos; añade superficie
--     (macros, query-builder dinámico, migraciones propias) y dificulta auditar
--     el SQL real que llega a Postgres.
--  2. La mejor defensa en profundidad es: validación en Rust (validator +
--     allowlists ORDER BY + escape_like) + SQL 100% parametrizado + rol mínimo
--     + FUNCIONES SECURITY DEFINER para las 3 operaciones críticas
--     transaccionales + RLS como segunda capa + timeouts.
--
-- Esta migración (idempotente):
--  a. Fija search_path en set_updated_at() (evita hijack de search_path).
--  b. Crea 3 funciones SECURITY DEFINER con SET search_path = public, pg_temp:
--     app_refresh_rotate, app_convert_lead, app_update_deal_stage.
--     Los handlers actuales siguen usando SQL directo (compatible); las funciones
--     quedan disponibles para migrar los handlers críticos sin cambiar la API.
--  c. Endurece rol y esquema: REVOKE CREATE ON SCHEMA public FROM PUBLIC,
--     timeouts a nivel DB como respaldo (los de conexión de la app mandan).
--  d. Habilita RLS en tablas CRM con políticas permisivas iniciales
--     (USING (true) para el rol actual). Segunda capa lista para escalar a
--     multi-tenant por claims.sub sin cambiar handlers hoy.
-- Fase 2 (pendiente, documentada): cambiar handlers a SELECT * FROM app_*($1...)
-- y REVOKE INSERT/UPDATE/DELETE directos, dejando solo EXECUTE.

-- a. search_path fijo en trigger existente.
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql SET search_path = public, pg_temp;

-- b.1 Rotación atómica de refresh (detección de reuso incluida).
CREATE OR REPLACE FUNCTION app_refresh_rotate(
    p_old_hash TEXT,
    p_new_hash TEXT,
    p_expires_at TIMESTAMPTZ,
    OUT p_user_id UUID,
    OUT p_reused BOOLEAN
) RETURNS RECORD AS $$
DECLARE
    v_row_id UUID;
BEGIN
    SELECT id, user_id INTO v_row_id, p_user_id
    FROM refresh_tokens
    WHERE token_hash = p_old_hash
      AND revoked_at IS NULL
      AND expires_at > now()
    FOR UPDATE;

    IF NOT FOUND THEN
        SELECT EXISTS(
            SELECT 1 FROM refresh_tokens
            WHERE token_hash = p_old_hash AND revoked_at IS NOT NULL
        ) INTO p_reused;
        p_user_id := NULL;
        RETURN;
    END IF;

    UPDATE refresh_tokens SET revoked_at = now() WHERE id = v_row_id;
    INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
    VALUES (p_user_id, p_new_hash, p_expires_at);
    p_reused := FALSE;
    RETURN;
END;
$$ LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp;
REVOKE ALL ON FUNCTION app_refresh_rotate(TEXT, TEXT, TIMESTAMPTZ) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app_refresh_rotate(TEXT, TEXT, TIMESTAMPTZ) TO PUBLIC;

-- b.2 Conversión de lead idempotente (solo qualified sin convertir).
CREATE OR REPLACE FUNCTION app_convert_lead(
    p_lead_id UUID,
    OUT p_converted_id UUID
) RETURNS UUID AS $$
DECLARE
    v_status TEXT;
BEGIN
    SELECT status::text INTO v_status
    FROM leads WHERE id = p_lead_id FOR UPDATE;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'lead no encontrado' USING ERRCODE = 'P0002';
    END IF;
    IF v_status IS DISTINCT FROM 'qualified' THEN
        RAISE EXCEPTION 'lead no está en estado qualified' USING ERRCODE = 'P0001';
    END IF;

    UPDATE leads
    SET status = 'converted'::lead_status,
        converted_at = now()
    WHERE id = p_lead_id
      AND status = 'qualified'::lead_status
      AND converted_at IS NULL
    RETURNING id INTO p_converted_id;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'lead ya convertido (condición de carrera)' USING ERRCODE = 'P0001';
    END IF;
    RETURN;
END;
$$ LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp;
REVOKE ALL ON FUNCTION app_convert_lead(UUID) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app_convert_lead(UUID) TO PUBLIC;

-- b.3 Cambio de etapa de deal (lista blanca en DB como segunda capa).
CREATE OR REPLACE FUNCTION app_update_deal_stage(
    p_deal_id UUID,
    p_stage TEXT,
    OUT p_new_stage TEXT
) RETURNS TEXT AS $$
BEGIN
    IF p_stage NOT IN ('lead','qualified','proposal','negotiation','closed_won','closed_lost') THEN
        RAISE EXCEPTION 'etapa inválida' USING ERRCODE = '22023';
    END IF;
    UPDATE deals SET stage = p_stage::deal_stage
    WHERE id = p_deal_id
    RETURNING stage::text INTO p_new_stage;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'deal no encontrado' USING ERRCODE = 'P0002';
    END IF;
    RETURN;
END;
$$ LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp;
REVOKE ALL ON FUNCTION app_update_deal_stage(UUID, TEXT) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app_update_deal_stage(UUID, TEXT) TO PUBLIC;

-- c. Endurecimiento de esquema y timeouts (respaldo; la app fija los suyos).
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
ALTER DATABASE occb_db SET statement_timeout = '5s';
ALTER DATABASE occb_db SET idle_in_transaction_session_timeout = '10s';
ALTER DATABASE occb_db SET lock_timeout = '3s';

-- d. RLS como segunda capa (políticas permisivas hoy; escalar a multi-tenant
--    por owner_id/claims.sub en fase 2 sin cambiar handlers).
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_tables WHERE tablename = 'contacts') THEN RETURN; END IF;
END $$;
ALTER TABLE IF EXISTS contacts ENABLE ROW LEVEL SECURITY;
ALTER TABLE IF EXISTS companies ENABLE ROW LEVEL SECURITY;
ALTER TABLE IF EXISTS deals ENABLE ROW LEVEL SECURITY;
ALTER TABLE IF EXISTS activities ENABLE ROW LEVEL SECURITY;
ALTER TABLE IF EXISTS leads ENABLE ROW LEVEL SECURITY;
ALTER TABLE IF EXISTS documents ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS app_all ON contacts;
CREATE POLICY app_all ON contacts FOR ALL USING (true) WITH CHECK (true);
DROP POLICY IF EXISTS app_all ON companies;
CREATE POLICY app_all ON companies FOR ALL USING (true) WITH CHECK (true);
DROP POLICY IF EXISTS app_all ON deals;
CREATE POLICY app_all ON deals FOR ALL USING (true) WITH CHECK (true);
DROP POLICY IF EXISTS app_all ON activities;
CREATE POLICY app_all ON activities FOR ALL USING (true) WITH CHECK (true);
DROP POLICY IF EXISTS app_all ON leads;
CREATE POLICY app_all ON leads FOR ALL USING (true) WITH CHECK (true);
DROP POLICY IF EXISTS app_all ON documents;
CREATE POLICY app_all ON documents FOR ALL USING (true) WITH CHECK (true);
