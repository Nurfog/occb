-- 046: aislamiento por empresa, fase 2 (scoping a nivel app).
--
-- Cada fila CRM lleva el tenant creador. Regla de visibilidad (ver
-- `auth::tenant_scope`): usuario platform (tenant NULL, incl. superadmin)
-- ve todo; usuario de empresa solo `tenant_id = su tenant`. Las filas
-- legacy con NULL quedan visibles solo para platform (nunca para tenants).
-- Sin backfill destructivo: ningún dato cambia de dueño en esta migración.

ALTER TABLE contacts ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants (id) ON DELETE SET NULL;
ALTER TABLE companies ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants (id) ON DELETE SET NULL;
ALTER TABLE deals ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants (id) ON DELETE SET NULL;
ALTER TABLE activities ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants (id) ON DELETE SET NULL;
ALTER TABLE leads ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants (id) ON DELETE SET NULL;
ALTER TABLE documents ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants (id) ON DELETE SET NULL;
ALTER TABLE privacy_requests ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants (id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_contacts_tenant ON contacts (tenant_id);
CREATE INDEX IF NOT EXISTS idx_companies_tenant ON companies (tenant_id);
CREATE INDEX IF NOT EXISTS idx_deals_tenant ON deals (tenant_id);
CREATE INDEX IF NOT EXISTS idx_activities_tenant ON activities (tenant_id);
CREATE INDEX IF NOT EXISTS idx_leads_tenant ON leads (tenant_id);
CREATE INDEX IF NOT EXISTS idx_documents_tenant ON documents (tenant_id);
CREATE INDEX IF NOT EXISTS idx_privacy_requests_tenant ON privacy_requests (tenant_id);
