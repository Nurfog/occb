-- 004: RBAC por perfiles (idempotente).
-- Primer usuario registrado = admin (bootstrap); el resto = user.

CREATE TABLE IF NOT EXISTS profiles (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT        NOT NULL UNIQUE CHECK (char_length(name) BETWEEN 1 AND 50),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS profile_permissions (
    profile_id UUID NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    permission TEXT NOT NULL CHECK (char_length(permission) BETWEEN 1 AND 100),
    PRIMARY KEY (profile_id, permission)
);

ALTER TABLE users ADD COLUMN IF NOT EXISTS profile_id UUID REFERENCES profiles (id);

INSERT INTO profiles (name) VALUES ('admin'), ('user')
ON CONFLICT (name) DO NOTHING;

INSERT INTO profile_permissions (profile_id, permission)
SELECT id, 'admin.access' FROM profiles WHERE name = 'admin'
ON CONFLICT DO NOTHING;

INSERT INTO profile_permissions (profile_id, permission)
SELECT id, x.permission FROM profiles
CROSS JOIN (VALUES ('users.read'), ('users.list')) AS x(permission)
WHERE name = 'user'
ON CONFLICT DO NOTHING;

-- Usuarios existentes sin perfil -> 'user'.
UPDATE users
SET profile_id = (SELECT id FROM profiles WHERE name = 'user')
WHERE profile_id IS NULL;

-- Si aún no hay ningún admin, el usuario más antiguo lo es.
UPDATE users
SET profile_id = (SELECT id FROM profiles WHERE name = 'admin')
WHERE id = (SELECT id FROM users ORDER BY created_at ASC LIMIT 1)
  AND NOT EXISTS (
    SELECT 1 FROM users u
    JOIN profiles p ON p.id = u.profile_id
    WHERE p.name = 'admin'
  );
