-- ─── Embudos (Pipelines) ─────────────────────────────────────────
CREATE TABLE IF NOT EXISTS pipelines (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(100) NOT NULL UNIQUE,
    description TEXT,
    entity_type VARCHAR(50) NOT NULL DEFAULT 'person',
    is_default BOOLEAN DEFAULT false,
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS pipeline_stages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    pipeline_id UUID NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    position INT NOT NULL DEFAULT 0,
    color VARCHAR(50) DEFAULT '#6B7280',
    probability INT DEFAULT 0,
    is_default BOOLEAN DEFAULT false,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Link deals to pipelines
ALTER TABLE deals ADD COLUMN IF NOT EXISTS pipeline_id UUID REFERENCES pipelines(id);
ALTER TABLE deals ADD COLUMN IF NOT EXISTS pipeline_stage_id UUID REFERENCES pipeline_stages(id);

-- ─── Perfiles y permisos ─────────────────────────────────────────
CREATE TABLE IF NOT EXISTS profiles (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    is_system BOOLEAN DEFAULT false,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS profile_permissions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    profile_id UUID NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    permission VARCHAR(255) NOT NULL,
    UNIQUE(profile_id, permission)
);

ALTER TABLE users ADD COLUMN IF NOT EXISTS profile_id UUID REFERENCES profiles(id);

-- ─── Branding ────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS branding (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    company_name VARCHAR(255),
    logo_url VARCHAR(500),
    primary_color VARCHAR(50) DEFAULT '#2563eb',
    secondary_color VARCHAR(50) DEFAULT '#1e40af',
    accent_color VARCHAR(50) DEFAULT '#10b981',
    favicon_url VARCHAR(500),
    custom_domain VARCHAR(255),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- ─── Insert pipelines por defecto ────────────────────────────────
INSERT INTO pipelines (name, slug, description, entity_type, is_default) VALUES
  ('Personas Naturales', 'persons', 'Embudo para leads individuales y clientes persona natural', 'person', true),
  ('Personas Jurídicas', 'companies', 'Embudo para empresas y clientes persona jurídica', 'company', false)
ON CONFLICT DO NOTHING;

-- Stages for Personas Naturales
DO $$
DECLARE
    p_id UUID;
BEGIN
    SELECT id INTO p_id FROM pipelines WHERE slug = 'persons';

    INSERT INTO pipeline_stages (pipeline_id, name, position, color, probability, is_default) VALUES
      (p_id, 'Nuevo contacto',    0, '#6B7280', 10,  true),
      (p_id, 'Calificado',        1, '#3B82F6', 25,  false),
      (p_id, 'Propuesta enviada', 2, '#8B5CF6', 50,  false),
      (p_id, 'Negociación',       3, '#F59E0B', 75,  false),
      (p_id, 'Ganado',            4, '#10B981', 100, false),
      (p_id, 'Perdido',           5, '#EF4444', 0,   false)
    ON CONFLICT DO NOTHING;
END $$;

-- Stages for Personas Jurídicas
DO $$
DECLARE
    p_id UUID;
BEGIN
    SELECT id INTO p_id FROM pipelines WHERE slug = 'companies';

    INSERT INTO pipeline_stages (pipeline_id, name, position, color, probability, is_default) VALUES
      (p_id, 'Prospección',       0, '#6B7280', 5,   true),
      (p_id, 'Contacto inicial',  1, '#3B82F6', 15,  false),
      (p_id, 'Reunión',           2, '#8B5CF6', 35,  false),
      (p_id, 'Propuesta',         3, '#F59E0B', 60,  false),
      (p_id, 'Negociación',       4, '#F97316', 80,  false),
      (p_id, 'Cerrado',           5, '#10B981', 100, false),
      (p_id, 'Descartado',        6, '#EF4444', 0,   false)
    ON CONFLICT DO NOTHING;
END $$;

-- Profiles por defecto
INSERT INTO profiles (name, description, is_system) VALUES
  ('Administrador', 'Acceso completo al sistema', true),
  ('Vendedor', 'Gestión de contactos, deals y actividades', true),
  ('Ejecutivo de cuentas', 'Visión general sin edición', true)
ON CONFLICT DO NOTHING;

-- Branding por defecto (sin UNIQUE: evitar duplicados en re-runs)
INSERT INTO branding (company_name, primary_color, secondary_color, accent_color)
SELECT 'OpenCRM', '#2563eb', '#1e40af', '#10b981'
WHERE NOT EXISTS (SELECT 1 FROM branding LIMIT 1);
