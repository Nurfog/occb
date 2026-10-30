# SaaS multi-empresa: tenants + superadmin

## Modelo actual (fase 1)

- **Superadmin**: el root creado por el instalador (`users.is_superadmin`,
  `tenant_id NULL`). Único nivel por encima de todos los tenants; no usa
  perfiles (un admin de empresa nunca gestiona tenants).
- **Tenant** (`tenants`): empresa cliente con `slug` único y estado
  `pendiente -> activa <-> suspendida`. Alta en `pendiente`; se habilita o
  suspende con `PATCH` (transiciones estrictas, resto 422).
- **Usuarios de empresa**: `users.tenant_id`; sus admins se crean con perfil
  `Administrador` **solo** si la empresa está `activa`.
- **Enforcement de acceso**: `auth_middleware` devuelve 401 a usuarios de
  tenants `suspendida` (genérico, sin fugas). Superadmin y usuarios sin
  tenant pasan siempre.
- **Licencias**: `licenses.tenant_id` opcional ata una key a una empresa
  (registro y trazabilidad; la exigencia sigue a nivel despliegue).

## Endpoints (superadmin)

| Método | Ruta | Efecto |
|---|---|---|
| GET | `/api/v1/admin/tenants` | lista con conteo de usuarios |
| POST | `/api/v1/admin/tenants` | alta en `pendiente` (slug auto si falta) |
| PATCH | `/api/v1/admin/tenants/{id}` | `activa`/`suspendida` |
| DELETE | `/api/v1/admin/tenants/{id}` | solo vacía (si no, 422: suspender) |
| GET | `/api/v1/admin/tenants/{id}/users` | usuarios de la empresa |
| POST | `/api/v1/admin/tenants/{id}/admins` | crea su admin (empresa `activa`) |

Frontend: `/admin` (gateado por `me.is_superadmin`).

## Fase 2: aislamiento por filas (pendiente, plan)

Hoy las filas CRM son globales: un usuario de una empresa ve los mismos
contactos/deals que otra. Para aislamiento real por empresa:

1. Migración: `tenant_id UUID NULL REFERENCES tenants(id)` en `contacts`,
   `companies`, `deals`, `activities`, `leads`, `documents`,
   `privacy_requests`, `webhooks` (+ índice). Backfill: filas existentes al
   tenant `principal` (o NULL = visible global, decisión a tomar).
2. App: `AND (tenant_id = $X OR tenant_id IS NULL)` —o estricto sin NULL—
   en cada listado/detalle/export/búsqueda, con `tenant_id` tomado del
   `Claims` (extender claims con tenant en el login, no consultar por
   request). `POST` fija `tenant_id` del creador; superadmin opera
   cross-tenant con `?tenant=` explícito auditado.
3. RLS: `CREATE POLICY tenant_isolation ... USING (tenant_id = current_setting('app.tenant')::uuid)` con `SET LOCAL` por request como segunda capa (defensa en profundidad, igual que `040`).
4. Licencias por tenant: `module_licensed` filtra además por `tenant_id` del
   JWT, de modo que el DLC `flow` se vende por empresa.
5. Tests: e2e dos tenants (A no ve filas de B en lists/search/exports;
   suspender bloquea API; superadmin ve todo).

No implementar la fase 2 a medias: o global documentado (hoy) o aislado
verificado e2e.
