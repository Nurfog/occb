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

## Fase 2: aislamiento por filas (IMPLEMENTADO)

Filas CRM con `tenant_id` (migración `046`; sin backfill destructivo: las
filas legacy con NULL quedan visibles solo para platform, nunca para
tenants). Regla única en `auth::tenant_scope`:

- Usuario platform (`tenant_id NULL`, incl. superadmin): sin filtro (todo).
- Usuario de empresa: `AND ($N::uuid IS NULL OR tenant_id = $N)` en cada
  listado/detalle/búsqueda/export/count; escrituras sellan el tenant del
  creador; updates/deletes exigen la fila visible (0 filas -> 404, sin
  filtrar existencia); cross-links (`contact_id`/`company_id`) se validan
  visibles (404 si no).
- Alcance: contacts, companies, deals, activities, leads (+ sus
  actividades), documents, privacy_requests, related (vía padre), search,
  dashboard, reports, audit (vía usuarios), email logs (vía `sent_by`),
  calendar (vía usuarios del tenant), admin users.
- Licencias por empresa: `module_licensed` filtra por tenant del JWT;
  superadmin bypass; `/modules` reporta cobertura por usuario.
- Fuera de alcance (documentado): tags globales (las asignaciones se
  validan por entidad visible), webhooks platform, notificaciones ya
  user-scoped.
- RLS como segunda capa queda pendiente: exige roles DB por tenant
  (hoy un solo rol `occb_app`); el enforcement app está verificado e2e
  (tests de fugas A/B en cada área).
