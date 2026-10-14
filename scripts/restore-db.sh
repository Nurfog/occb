#!/usr/bin/env bash
# Restore de un backup generado con backup-db.sh. Uso:
#   ./scripts/restore-db.sh [dev|prod] <fichero.dump.gz>
set -euo pipefail

MODE="${1:-dev}"
FILE="${2:-}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="$ROOT/.env.$MODE"
DB_CONTAINER="occb-db-$MODE"
DB_COMPOSE="$ROOT/docker-compose.db.yml"

[[ -n "$FILE" && -f "$FILE" ]] || { echo "Uso: $0 [dev|prod] <fichero.dump.gz>"; ls -1 "$ROOT/backups" 2>/dev/null || true; exit 1; }
[[ -f "$ENV_FILE" ]] || { echo "No existe $ENV_FILE"; exit 1; }
set -a; # shellcheck disable=SC1090
source "$ENV_FILE"; set +a

if ! docker ps --format '{{.Names}}' | grep -qx "$DB_CONTAINER"; then
  echo "El contenedor $DB_CONTAINER no está en marcha."
  exit 1
fi

read -r -p "Restaurar $FILE sobre '$POSTGRES_DB' ($MODE). ¿Continuar? (y/N) " ans
[[ "$ans" == "y" || "$ans" == "Y" ]] || { echo "Cancelado."; exit 0; }

gunzip -c "$FILE" | docker compose -f "$DB_COMPOSE" --env-file "$ENV_FILE" exec -T db \
  pg_restore -U "$POSTGRES_USER" -d "$POSTGRES_DB" --clean --if-exists --no-owner
echo "Restore OK."
