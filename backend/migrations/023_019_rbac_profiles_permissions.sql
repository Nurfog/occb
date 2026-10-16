-- ─── RBAC: Seed permisos para perfiles por defecto ──────────────
-- Los 3 perfiles ya existen (migration 010) pero sin permisos asignados.

-- Administrador: todos los permisos
INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('contacts.view'), ('contacts.create'), ('contacts.edit'), ('contacts.delete'),
  ('companies.view'), ('companies.create'), ('companies.edit'), ('companies.delete'),
  ('deals.view'), ('deals.create'), ('deals.edit'), ('deals.delete'),
  ('activities.view'), ('activities.create'), ('activities.edit'), ('activities.delete'),
  ('reports.view'),
  ('settings.view'), ('settings.edit'),
  ('admin.access')
) AS v(permission)
WHERE p.name = 'Administrador'
ON CONFLICT DO NOTHING;

-- Vendedor: CRUD contacts, view+create companies, CRUD deals, CRUD activities, view reports
INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('contacts.view'), ('contacts.create'), ('contacts.edit'), ('contacts.delete'),
  ('companies.view'), ('companies.create'),
  ('deals.view'), ('deals.create'), ('deals.edit'), ('deals.delete'),
  ('activities.view'), ('activities.create'), ('activities.edit'), ('activities.delete'),
  ('reports.view')
) AS v(permission)
WHERE p.name = 'Vendedor'
ON CONFLICT DO NOTHING;

-- Ejecutivo de cuentas: solo lectura
INSERT INTO profile_permissions (profile_id, permission)
SELECT p.id, v.permission
FROM profiles p
CROSS JOIN (VALUES
  ('contacts.view'),
  ('companies.view'),
  ('deals.view'),
  ('activities.view'),
  ('reports.view')
) AS v(permission)
WHERE p.name = 'Ejecutivo de cuentas'
ON CONFLICT DO NOTHING;

-- ─── Migrar usuarios existentes a profiles ─────────────────────
-- Usuarios admin → perfil Administrador
UPDATE users SET profile_id = (
  SELECT id FROM profiles WHERE name = 'Administrador' LIMIT 1
)
WHERE role = 'admin' AND profile_id IS NULL;

-- Usuarios user → perfil Vendedor
UPDATE users SET profile_id = (
  SELECT id FROM profiles WHERE name = 'Vendedor' LIMIT 1
)
WHERE role = 'user' AND profile_id IS NULL;

-- Usuarios sin role ni profile → perfil Vendedor (solo si no tienen profile)
UPDATE users SET profile_id = (
  SELECT id FROM profiles WHERE name = 'Vendedor' LIMIT 1
)
WHERE profile_id IS NULL AND (role IS NULL OR role = '');

-- ─── Mantener columna role para backward compat ────────────────
-- La columna se mantiene pero se hace nullable para nuevos usuarios.
ALTER TABLE users ALTER COLUMN role DROP NOT NULL;
