-- ─── RBAC: permisos exigidos por el backend pero nunca seedados ─────────────
-- Los handlers usan `perms.require("<modulo>.<accion>")` con chequeo estricto
-- (sin bypass por admin.access). En una DB virgen, 019/030 solo otorgan
-- contacts/companies/deals/activities/reports/settings/tags, así que todo lo
-- demás (dashboard, calendar, documents, email, leads, etc.) devolvía 403
-- incluso para el primer administrador. Sigue el patrón de M030.
-- Idempotente (UNIQUE(profile_id, permission) + ON CONFLICT DO NOTHING).

-- Administrador: todos los permisos faltantes
INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('dashboard.view'),
  ('search.view'),
  ('leads.view'), ('leads.create'), ('leads.edit'), ('leads.delete'), ('leads.convert'),
  ('calendar.view'), ('calendar.create'), ('calendar.edit'), ('calendar.delete'),
  ('documents.view'), ('documents.upload'), ('documents.delete'),
  ('email.view'), ('email.send'),
  ('ai.use'),
  ('audit.view'),
  ('notifications.manage'),
  ('webhooks.view'), ('webhooks.manage')
) AS v(permission)
WHERE p.name = 'Administrador'
ON CONFLICT DO NOTHING;

-- Vendedor: flujo operativo (ver/crear/editar), sin auditoría ni webhooks ni IA
INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('dashboard.view'),
  ('search.view'),
  ('leads.view'), ('leads.create'), ('leads.edit'), ('leads.convert'),
  ('calendar.view'), ('calendar.create'), ('calendar.edit'),
  ('documents.view'), ('documents.upload'),
  ('email.view'), ('email.send')
) AS v(permission)
WHERE p.name = 'Vendedor'
ON CONFLICT DO NOTHING;

-- Ejecutivo de cuentas: solo lectura
INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('dashboard.view'),
  ('search.view'),
  ('leads.view'),
  ('calendar.view'),
  ('documents.view'),
  ('email.view')
) AS v(permission)
WHERE p.name = 'Ejecutivo de cuentas'
ON CONFLICT DO NOTHING;
