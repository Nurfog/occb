-- 042: Ley 21.719 en contexto educacional (colegios) + retención legal.
--
-- Un alumno inscrito NO puede suprimirse sin más: mientras exista matrícula
-- o relación educacional vigente hay fundamento legal y deberes de
-- conservación (contrato educacional, subvenciones, registros MINEDUC,
-- convivencia). La supresión en ese caso debe DENEGARSE fundadamente
-- (retención legal) y ofrecerse el bloqueo como alternativa.
-- Además los titulares suelen ser NNA: la solicitud puede presentarla el
-- apoderado/representante, por eso se registran sus datos para verificación.

ALTER TABLE privacy_requests
    ADD COLUMN IF NOT EXISTS solicitante_nombre TEXT,
    ADD COLUMN IF NOT EXISTS relacion TEXT NOT NULL DEFAULT 'titular'
        CHECK (relacion IN ('titular', 'apoderado', 'representante', 'otro')),
    ADD COLUMN IF NOT EXISTS documento_ref TEXT;
