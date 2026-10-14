#!/usr/bin/env bash
# Genera .env.dev (o .env.prod con "prod") desde el .example con secretos aleatorios.
set -euo pipefail

MODE="${1:-dev}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ "$MODE" == "prod" ]]; then
  SRC="$ROOT/.env.prod.example"; DST="$ROOT/.env.prod"
else
  SRC="$ROOT/.env.dev.example"; DST="$ROOT/.env.dev"
fi

if [[ -f "$DST" && "${2:-}" != "--force" ]]; then
  echo "$DST ya existe. Usa '$0 $MODE --force' para regenerarlo."
  exit 0
fi

command -v openssl >/dev/null || { echo "openssl no encontrado"; exit 1; }

JWT="$(openssl rand -base64 48)"

sed -e "s|^JWT_SECRET=.*|JWT_SECRET=$JWT|" "$SRC" > "$DST.tmp"

if [[ "$MODE" == "dev" ]]; then
  # En dev todo queda usable sin edición manual: passwords hex (seguras en URLs).
  DB_PASS="$(openssl rand -hex 24)"
  REDIS_PASS="$(openssl rand -hex 24)"
  sed -i -E -e "s|^POSTGRES_PASSWORD=.*|POSTGRES_PASSWORD=$DB_PASS|" \
         -e "s|^REDIS_PASSWORD=.*|REDIS_PASSWORD=$REDIS_PASS|" \
         -e "s|^(DATABASE_URL=[^:]+://[^:/]+:)[^@]+@|\1$DB_PASS@|" \
         -e "s|^(REDIS_URL=[^:]+://:)[^@]+@|\1$REDIS_PASS@|" \
         "$DST.tmp"
else
  echo "AVISO: .env.prod generado. Rellena POSTGRES_PASSWORD, REDIS_PASSWORD,"
  echo "DATABASE_URL, REDIS_URL y CORS_ORIGINS antes de desplegar."
fi

mv "$DST.tmp" "$DST"
chmod 600 "$DST"
echo "Generado $DST (permisos 600)."
