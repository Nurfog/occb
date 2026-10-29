# OCCB · Frontend

UI del proyecto OCCB (Next.js 15, App Router, TypeScript, CSS propio mínimo).
Este repositorio contiene SOLO el frontend; el backend vive en el proyecto
`occb` (Rust + Axum) y se consume por HTTP.

## Contrato consumido (backend `occb`, todo bajo `/api/v1`)

Base URL configurable con `NEXT_PUBLIC_API_URL`:

- dev: `http://127.0.0.1:8000`
- prod: `https://api.tudominio.com`

| Método | Ruta | Auth | Notas |
|---|---|---|---|
| POST | `/api/v1/auth/register` | no | `{email,name,password}` → 201 `{user,access_token,refresh_token,token_type,expires_in}`; 409 email duplicado; 422 validación |
| POST | `/api/v1/auth/login` | no | `{email,password}` → 200 mismo formato; 401 genérico |
| POST | `/api/v1/auth/refresh` | no | `{refresh_token}` → 200 par nuevo (rota; reuso → 401) |
| POST | `/api/v1/auth/logout` | sí | `{refresh_token}` → 204 |
| GET | `/api/v1/auth/me` | sí | `UserPublic {id,email,name,created_at}` |
| GET | `/api/v1/users?q=&page=&per_page=&sort=&dir=` | sí | `{data:[UserPublic],page,per_page,total}` |
| GET | `/api/v1/users/{id}` | sí | `UserPublic` o 404 (UUID inválido → 400) |

Errores: `{error:{code,message}}` (`unauthorized` 401, `conflict` 409,
`validation_error` 422, `rate_limited` 429 con `Retry-After`).

Reglas de tokens (aplicadas en `lib/auth.tsx`):

- Access (15 min) y refresh SOLO en memoria React (`useState`/`useRef`).
  NUNCA `localStorage`/`sessionStorage`.
- Ante 401 con access en memoria: un `refresh`, un reintento, y nada más.
- `logout` limpia la memoria aunque el servidor falle.

## Desarrollo

```bash
cp .env.development.example .env.development.local  # NEXT_PUBLIC_API_URL=http://127.0.0.1:8000
npm install
npm run dev    # http://localhost:3000
```

El backend dev debe escuchar en `127.0.0.1:8000` con CORS para
`http://localhost:3000` (ver `CORS_ORIGINS` en el `.env.dev` del backend).
Prueba: registra un usuario en `/`, revisa el perfil (`/auth/me`) y abre
`/users` (listado + detalle por id).

## Producción

```bash
npm run build
npm run start   # PORT 3000
```

Docker (multi-stage, runner no-root `nextjs:1001`, standalone):

```bash
docker build --build-arg NEXT_PUBLIC_API_URL=https://api.tudominio.com -t occb-frontend .
docker run -p 3000:3000 occb-frontend
```

## Despliegue

- Este contenedor sirve la web en el puerto 3000.
- Dominio sugerido: `app.tudominio.com` → este contenedor (vía Caddy con TLS).
- El API va por `api.tudominio.com` → backend (vía el mismo Caddy, proyecto aparte).
- `NEXT_PUBLIC_API_URL` se fija en build: reconstruye la imagen para cambiarlo.
