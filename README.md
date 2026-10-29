# OCCB · Backend

![CI](https://github.com/Nurfog/occb/actions/workflows/ci.yml/badge.svg)

API del proyecto OCCB. **Este repositorio contiene SOLO el backend** (Rust + Axum + SQLx).

| Proyecto | Contenido | Dónde |
|---|---|---|
| `occb` (este) | API backend (contenedor principal) | aquí |
| satélite DB | Postgres 18 + pgvector + Redis (dev) | `docker-compose.db.yml` (prod: proyecto aparte) |
| satélite Caddy | reverse proxy + TLS (dev) | `docker-compose.caddy.yml` (prod: proyecto aparte) |
| proyecto frontend | UI | aparte |

DB y Caddy corren en sus propios contenedores, integrados a la **red
principal del backend** (`occb-dev` en dev, `occb-edge` en prod).
El backend se conecta a la DB por `DATABASE_URL` y se expone al exterior
únicamente a través de Caddy. Nunca publica su puerto en prod.

## Arquitectura

```
internet ──TLS──▶ Caddy ──HTTP──▶ backend:8000 ──▶ db:5432 (PG18+pgvector) + redis:6379
   ▲              (red occb-dev/dev u occb-edge/prod, todos en la misma red)
frontend (aparte)   :80/:443        127.0.0.1:8000 (solo dev, debug)
```

## Estructura

```
backend/
  src/
    main.rs            arranque, capas, shutdown ordenado
    config.rs          env + validación fail-fast (secretos nunca en logs)
    db.rs              pool + statement_timeout defensivos
    error.rs           errores tipados sin fugas (4xx útil / 5xx genérico)
    auth.rs            JWT HS256 + refresh opacos (SHA-256, rotación)
    password.rs        Argon2id + política 12..128
    models.rs          validación + paginación acotada + escape_like
    state.rs           estado compartido
    routes.rs          tabla de rutas
    handlers/          health, ready, auth, users
    middleware/        auth (Bearer), rate-limit, cabeceras seguridad
  migrations/          esquema SQL (lo aplica ESTE proyecto sobre la DB externa)
  Dockerfile           multi-stage, no-root (10000), fs solo-lectura
docker-compose.dev.yml    backend (crea la red "occb-dev")
docker-compose.db.yml     satélite DB dev (Postgres 18 + pgvector + Redis, red externa occb-dev)
docker-compose.caddy.yml  satélite Caddy dev (red externa occb-dev)
docker-compose.prod.yml   SOLO backend (sin puertos; red externa `occb-edge`)
caddy/Caddyfile.dev       servidor web dev (proxy /api/* → backend:8000)
scripts/              install-deps, gen-secrets, migrate, backup-db, restore-db
```

## Requerimientos

```bash
./scripts/install-deps.sh --check        # ver qué falta (sin instalar)
sudo ./scripts/install-deps.sh           # instala según tu distro
sudo ./scripts/install-deps.sh --with-rust  # + Rust (solo para cargo local)
```

Soporta Arch (+derivados como CachyOS/EndeavourOS/Manjaro), Debian/Ubuntu
y derivados, Fedora/RHEL/Rocky/Alma y openSUSE. Instala: Docker + Compose v2,
`openssl`, `curl`, `make`, `git`. **Postgres NO se instala en el host**: vive
en un contenedor, y `--check` lo verifica (red `occb-dev`, `occb-db-dev`
healthy, `occb-caddy-dev` en marcha). El instalador además crea la red y
levanta los contenedores DB y Caddy integrados a ella. Sin Docker no
necesitas Rust local.

## Inicio rápido (dev)

```bash
./scripts/install-deps.sh --check   # audita SO + stack
sudo ./scripts/install-deps.sh      # instala SO y levanta DB + Caddy en la red del backend
make dev                            # red + db + migraciones + backend + caddy
curl http://127.0.0.1:8000/api/v1/health   # directo
curl http://localhost/api/v1/health        # vía Caddy
```

Sin Docker: `cd backend && DATABASE_URL=... cargo run` (lee `../.env.dev`).

## Contratos con los otros proyectos

### Satélite DB (Postgres 18 + pgvector)

- En dev: `docker-compose.db.yml` (contenedores `occb-db-dev` / `occb-redis-dev`
  en la red del backend). En prod: proyecto aparte unido a `occb-edge`.
- Debe exponer `postgres://occb_app:<pass>@<host>:5432/occb_db`.
- Extensiones requeridas: `pgcrypto`, `citext`, `pg_trgm`, `vector`
  (las crea `backend/migrations/001_extensions.sql`, idempotente).
- El esquema lo versiona ESTE proyecto (`backend/migrations/`, orden numérico,
  nunca renombrar aplicadas) vía `scripts/migrate.sh` (usa `docker exec`,
  sin cliente postgres en el host).
- Redis (opcional): `REDIS_URL=redis://:<pass>@<host>:6379`. Sin él, el
  rate-limit usa memoria local y `/ready` lo informa como `disabled`.

### Satélite Caddy (borde)

- En dev: `docker-compose.caddy.yml` (`caddy/Caddyfile.dev`). En prod:
  proyecto aparte unido a `occb-edge`.
- Proxy de `/api/*` → `backend:8000` (resolución por DNS interno de la red).
- TLS automático + HSTS + las mismas cabeceras de seguridad (defensa en
  profundidad; el backend ya las emite).
- `request_body max_size 2MB`; límite real de 1 MB en el backend.
- Reenvía `X-Forwarded-For` (el backend usa el ÚLTIMO salto con
  `TRUSTED_PROXY=true`).
- Healthcheck del backend: `GET /api/v1/health`.

### Proyecto frontend

- Base URL dev: `http://localhost/api/v1` (vía Caddy) o directo `:8000`.
- Auth: `Authorization: Bearer <access>` (15 min) + refresh opaco (30 d).
  Tokens en memoria del frontend, nunca en `localStorage`.
- CORS cerrado a `CORS_ORIGINS`. Preflight permitido solo desde esos orígenes.
- Respuestas de error: `{ "error": { "code": "...", "message": "..." } }`.

### Backups programados (prod)

En el servidor prod (repo en `/opt/occb`, usuario `occb` en grupo `docker`):

```bash
sudo cp deploy/systemd/occb-backup.* /etc/systemd/system/
sudo mkdir -p /var/backups/occb && sudo chown occb:docker /var/backups/occb
sudo systemctl daemon-reload
sudo systemctl enable --now occb-backup.timer
systemctl list-timers occb-backup.timer
```

Alternativa con cron: `0 3 * * * /opt/occb/scripts/backup-db.sh prod`.
Los backups quedan en `/var/backups/occb` (o `BACKUP_DIR`), retención de 7.

## Endpoints

| Método | Ruta | Auth | Descripción |
|---|---|---|---|
| GET | `/api/v1/health` | no | liveness |
| GET | `/api/v1/ready` | no | readiness (db + redis) |
| GET | `/api/v1/version` | no | versión |
| POST | `/api/v1/auth/register` | no | alta (201 / 409 duplicado / 422) |
| POST | `/api/v1/auth/login` | no | 401 genérico (anti-enumeración) |
| POST | `/api/v1/auth/refresh` | no | rotación; reuso → 401 + alerta en log |
| POST | `/api/v1/auth/logout` | sí | revoca refresh |
| GET | `/api/v1/auth/me` | sí | perfil |
| GET | `/api/v1/users?q=&page=&per_page=&sort=&dir=` | sí | listado paginado |
| GET | `/api/v1/users/{id}` | sí | detalle (UUID estricto) |

## Modelo de seguridad

**Anti-inyección SQL** (verificado con pruebas reales contra PG 18):

- 100 % consultas parametrizadas (`$1…` + `.bind`). Ningún valor de usuario
  se concatena en SQL.
- `ORDER BY` solo desde lista blanca (`email|name|created_at`, resto →
  `created_at`); dirección `ASC|DESC`.
- `LIKE` con `escape_like()` (`% _ \` → escapados) + `ESCAPE '\'`.
- `LIMIT/OFFSET` enteros acotados (`per_page` 1–100) y bindeados.
- `statement_timeout 5s` por conexión + `idle_in_transaction 10s`.
- `23505` → `409` sin exponer SQL; resto de errores DB → `500` genérico.

**AuthN/Z**:

- Argon2id, política 12–128, 401 idéntico exista o no el usuario (+ coste
  dummy anti-timing).
- JWT HS256 explícito (rechaza `none`), `typ=access`, `iss/aud` validados,
  expiración corta. Token solo por header `Authorization` (nunca en URL).
- Refresh opaco 256 bits, solo SHA-256 en DB, rotación atómica en
  transacción, detección de reuso con `warn` en log.
- CORS: lista cerrada, credenciales solo con orígenes explícitos, preflight
  `max-age` 600 s.

**Borde y red**:

- Rate-limit global 429 + `Retry-After` + `X-RateLimit-*`; IP del último
  salto XFF solo con proxy de confianza.
- Body ≤ 1 MB (413), timeout 10 s (408), HSTS solo prod, CSP
  `default-src 'none'`, `nosniff/DENY/COOP/COEP/CORP`.
- Contenedor no-root, fs solo-lectura, `cap_drop: ALL`,
  `no-new-privileges`, logs acotados, healthchecks.
- Secretos solo por entorno (`.env.*`, permisos 600, gitignorados);
  `JWT_SECRET` ≥ 32 chars, en prod se rechazan valores de ejemplo;
  password de DB nunca en logs (redactada).

**Checklist prod**: `.env.prod` real, `CORS_ORIGINS` sin localhost,
`occb-edge` compartida con Caddy, `TRUSTED_PROXY=true`, backups
programados (`backup-db.sh`), `RUST_LOG=info`.

## Operación

```bash
make deps-check | make deps          # auditar / instalar SO + stack
make dev | make dev-logs | make dev-down   # stack dev completo
make db-up | make db-down | make db-logs   # solo satélite DB
make caddy-up | make caddy-down            # solo satélite Caddy
make stack-up | make stack-down            # alias de dev / dev-down+db-down
make migrate-dev | make migrate-prod
make backup-dev  | make backup-prod
./scripts/restore-db.sh prod backups/occb_prod_....dump.gz
make check       # valida los 4 compose
make test        # cargo test (8 tests, sin DB)
```

### Puertos en dev

Publicados en loopback/borde, configurables por entorno (útil con stacks
en paralelo). Valores por defecto:

| Variable | Defecto | Servicio |
|---|---|---|
| `BACKEND_PORT` | 8000 | backend (debug directo) |
| `POSTGRES_PORT` | 5432 | postgres |
| `REDIS_PORT` | 6379 | redis |
| `CADDY_HTTP_PORT` / `CADDY_HTTPS_PORT` | 80 / 443 | caddy |

```bash
BACKEND_PORT=8001 POSTGRES_PORT=5433 REDIS_PORT=6380 \
CADDY_HTTP_PORT=8080 CADDY_HTTPS_PORT=8443 make dev
```

## Tests

`cargo test`: JWT roundtrip + rechazo, refresh aleatorio/hash, Argon2 +
política, paginación/lista blanca, `escape_like`, redacción de URL,
parseo de `REDIS_URL`. Verificado además end-to-end contra
`pgvector/pgvector:pg18` (registro, login, refresh+reuso, logout,
SQLi en UUID/sort/LIKE, CORS, `alg:none`, body 413, rate-limit, 409/422).
