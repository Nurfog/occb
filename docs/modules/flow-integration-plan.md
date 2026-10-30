# Plan de integración: módulo `flow` (Automatizaciones estilo n8n)

Adapta `/home/juan/dev/occb-flow.md` al monorepo + sistema DLC
(`docs/modules/overview.md`). El módulo vive **dentro** de este repo;
DB/Redis/Caddy siguen en sus proyectos satélite.

## 1. Alcance del módulo

- Backend: `backend/src/handlers/flow/*` (reemplaza el stub
  `handlers/flow.rs`), tablas en schema propio `flowcraft` dentro de
  `occb_db` (rol mínimo aparte si se quiere aislar: `flowcraft_app`).
- Worker: servicio `flow-worker` en el mismo compose (polling + WebSocket de
  runs), sin puertos públicos (Caddy lo alcanza en red interna si hace falta).
- Frontend: app canvas (`frontend/app/flow/`) con `@xyflow/react`, gateada
  con `<ModuleGate slug="flow">`.
- Rutas: `/api/v1/flows/*`, `/api/v1/executions/*`, `POST /webhooks/flow/{id}`
  (HMAC, público con rate-limit estricto), `CRUD /api/v1/admin/flow/keys`.

## 2. Checklist DLC (obligatorio antes del merge)

1. Migración `0xx_flowcraft.sql`: schema + tablas (`workflows`, `nodes`,
   `connections`, `credentials` cifradas AES-GCM, `executions`,
   `execution_logs`, `api_keys` solo-hash, `webhook_secrets` solo-hash),
   RLS `app_all`, trigger `updated_at`. Idempotente + re-aplicable (CI).
2. `UPDATE modules SET enabled = TRUE WHERE slug = 'flow'` en la misma
   migración (el catálogo ya trae la fila apagada desde `044`).
3. **Todas** las rutas del módulo tras `require_module("flow")` + JWT
   (admin donde corresponda). Sin excepción: ni una ruta sin gate.
4. API keys por integración `ocfl_<id>_<rand>` solo-hash + scopes +
   expiración (patrón `auth.rs` + `models/license.rs`).
5. Credenciales de terceros cifradas AES-GCM (`services/crypto.rs`);
   ninguna en claro ni en logs.
6. Worker endurecido como el backend (`USER 10000`, `read_only`, sin
   puertos, healthcheck) + servicio en `docker-compose.dev/prod.yml`.
7. Caddy: `@wh path /webhooks/* → worker` (ver `occb-flow.md §6`); en prod
   lo aplica el proyecto satélite `occb-caddy`.
8. Frontend canvas gateado + estados de run; `npm run build` verde en CI.
9. Tests: engine (topológico, ramas, reintentos), auth keys, HMAC webhooks,
   `cargo test` + e2e licenciado/no-licenciado (402 → emitir → 200).
10. Docs de amenazas (spoofing/tampering/elevación por módulo) + backup del
    schema en `scripts/`.

## 3. MVP (de `occb-flow.md §7`, sin cambios)

Engine DAG topológico, ejecución secuencial + ramas If, reintentos con
backoff, timeout por nodo, logs por ejecución. Nodos v1: Manual, Webhook,
Cron, HTTP, Code (sandbox sin FS/red salvo allowlist), If/Switch, Delay.

## 4. Fases

- P0: inventario n8n (módulo → contrato propio) + OpenAPI del módulo.
- P1: migración `flowcraft/*` + matriz keys/scopes + este checklist en verde.
- P2: backend Axum + worker Tokio + canvas React Flow.
- P3: compose/Caddy/Makefile + licencia demo en dev.
- P4: Trivy en CI (extender `ci.yml`), backup schema, doc amenazas.

## 5. Licenciamiento del módulo

- Emitir en dev: `POST /api/v1/admin/licenses {"label":"demo-flow",
  "modules":["flow"]}` → probar `GET /api/v1/flow/ping` (402 sin key
  asociada no aplica aquí: el gate es por licencia del despliegue, no por
  usuario — una licencia `flow` activa abre el DLC para todo el despliegue).
- Revocar corta el acceso de inmediato (el worker deja de tomar runs nuevos;
  los en curso terminan).
