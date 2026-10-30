# Módulos estilo DLC (SaaS)

El software funciona como un juego con DLCs: el despliegue trae un catálogo
de módulos y cada DLC se activa con una **licencia por key**. Sin licencia
válida, las rutas del módulo responden `402 module_locked` y el frontend
muestra estado "bloqueado" en vez de romper.

## Modelo

- **Catálogo** (`modules`): `slug` (PK, `^[a-z0-9-]{2,40}$`), nombre,
  descripción, versión, `enabled` (kill switch), `route_prefix`.
  - Base incluida, sin gate: `crm`, `privacy`.
  - DLC: `flow` (registrado apagado hasta implementarse).
- **Licencias** (`licenses`): `label`, `key_hash` (solo SHA-256; la key en
  claro formato `occm_<64 hex>` se muestra UNA vez al emitirla),
  `key_prefix` (soporte), `modules TEXT[]` (`'*'` = plan total), `seats`,
  `expires_at`, `revoked_at`, `metadata`.
- Cobertura = licencia no revocada + no expirada + (`slug ∈ modules` o
  `'*' ∈ modules`) + módulo `enabled`.

## Backend

- `GET /api/v1/modules` (JWT): catálogo con `licensed` por módulo.
- Admin (`admin.access`): `POST /api/v1/admin/licenses` (emite, devuelve la
  key una vez), `GET /api/v1/admin/licenses` (sin hashes),
  `DELETE /api/v1/admin/licenses/{id}` (revoca),
  `PUT /api/v1/admin/modules/{slug}` (kill switch).
- Gating en `routes.rs` por router DLC:
  ```rust
  let flow = Router::new()
      .route("/api/v1/flow/ping", get(handlers::flow::ping))
      .route_layer(from_fn({ /* enforce_module(state, "flow") */ }))
      .layer(from_fn_with_state(state, auth_middleware));
  ```
  Orden: JWT primero, licencia después. Ver `middleware/modules.rs`.
- Convención para un módulo nuevo: handlers propios + `require` de su slug en
  **todas** sus rutas + fila en `modules` (seed en la migración del módulo).

## Frontend

- `lib/modules.tsx`: `ModulesProvider` (carga `/api/v1/modules` tras login),
  `useModules()` (`open(slug)`, `licensed(slug)`), `<ModuleGate slug>` con
  tarjeta "bloqueado" (los base `crm`/`privacy` siempre open).
- Misma-origen: `NEXT_PUBLIC_API_URL=""` (Caddy sirve app + API en un
  dominio; sin CORS ni URLs absolutas).

## Operación

- Emitir: `POST /admin/licenses {"label":"Cliente X","modules":["flow"]}` →
  guardar la key fuera del sistema (no se recupera).
- Revocar: `DELETE /admin/licenses/{id}` (efecto inmediato; sin caché).
- Rotar: emitir nueva + revocar la vieja. Auditoría en `audit_log`
  (`license_issued` / `license_revoked`).
- Facturación (Stripe/MercadoPago) fuera de alcance: el cobro emite la key
  por este API manual o integrado después.
