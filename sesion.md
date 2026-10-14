# Sesión de trabajo — OCCB backend

Para continuar otro día: abre una sesión en `/home/juan/dev/occb` y menciona
este fichero. ID de la sesión donde se construyó este proyecto:

```
ses_ec78ede79ffeX8Omz6SmwHy5X0
```

- Título: "Separar OpenCCB en backend y frontend con Docker seguro para dev y prod"
- Rama/commits: `main` en `https://github.com/Nurfog/occb.git`
  (último: `72af201` + commits posteriores de stack/deps — ver `git log`).
- Stack: Rust edition 2024 (toolchain 1.98), Axum + SQLx, Postgres 18 +
  pgvector, Redis 7, Caddy 2.

## Estado al cerrar (2026-10-14)

- Stack dev LEVANTADO y verificado: `occb-db-dev` y `occb-redis-dev`
  (healthy), `occb-backend-dev-backend-1` (healthy), `occb-caddy-dev` (up),
  red `occb-dev`. Puertos estándar: `:8000` directo, `:80` vía Caddy.
- `install-deps.sh --check`: todo OK.
- Tests: `cargo test` 8/8, clippy sin avisos propios, 4 compose validan.
- Purga hecha: stacks viejos (`opencrmccb-*`, `opencmrccb-*`,
  `schoolccb-*`, `openccbagent-*`) eliminados con sus volúmenes/redes.
  Imágenes Docker NO tocadas. OJO: al purgar cayó también el stack occb
  (filtro mal formado) y se reconstruyó con `make dev` sin pérdida
  (mismo volumen `pgdata-dev`).
- `.env.dev` / `.env.prod` generados localmente (gitignorados, permisos 600).

## Pendiente / próximos pasos sugeridos

1. Proyecto DB prod y proyecto Caddy prod (red externa `occb-edge`).
2. Proyecto frontend (contrato en README: base URL, Bearer, CORS, errores).
3. RBAC por perfiles (el backend trae `Forbidden` reservado para ello).
4. Backups programados en prod + `RUST_LOG=info` + `CORS_ORIGINS` sin localhost.
5. Limpieza opcional: `docker builder prune`, imágenes viejas
   (`opencrmccb-*`, `postgres:17`, `pgvector:pg17`, `rust:1.8x`...).
