-- ─── RBAC: Agregar permisos de tags a perfiles ─────────────────
-- Los permisos de tags no fueron incluidos en la migración 019.

-- Administrador: todos los permisos de tags
INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('tags.view'), ('tags.create'), ('tags.edit'), ('tags.delete')
) AS v(permission)
WHERE p.name = 'Administrador'
ON CONFLICT DO NOTHING;

-- Vendedor: ver y crear tags
INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('tags.view'), ('tags.create')
) AS v(permission)
WHERE p.name = 'Vendedor'
ON CONFLICT DO NOTHING;

-- Ejecutivo de cuentas: solo ver tags
INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('tags.view')
) AS v(permission)
WHERE p.name = 'Ejecutivo de cuentas'
ON CONFLICT DO NOTHING;
