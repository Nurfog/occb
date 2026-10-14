#!/usr/bin/env bash
# Backup de Postgres (contenedor occb-db-<modo>) en formato custom + gzip.
# Uso: ./scripts/backup-db.sh [dev|prod]
# Guarda en ./backups y conserva las últimas 7 copias. Sin pg_dump en el host.
set -euo pipefail

MODE="${1:-dev}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="$ROOT/.env.$MODE"
DB_CONTAINER="occb-db-$MODE"
DB_COMPOSE="$ROOT/docker-compose.db.yml"
BACKUP_DIR="${BACKUP_DIR:-$ROOT/backups}"

[[ -f "$ENV_FILE" ]] || { echo "No existe $ENV_FILE"; exit 1; }
mkdir -p "$BACKUP_DIR"
set -a; # shellcheck disable=SC1090
source "$ENV_FILE"; set +a

if ! docker ps --format '{{.Names}}' | grep -qx "$DB_CONTAINER"; then
  echo "El contenedor $DB_CONTAINER no está en marcha."
  exit 1
fi

TS="$(date +%Y%m%d_%H%M%S)"
OUT="$BACKUP_DIR/occb_${MODE}_${TS}.dump.gz"

docker compose -f "$DB_COMPOSE" --env-file "$ENV_FILE" exec -T db \
  pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Fc | gzip > "$OUT"
chmod 600 "$OUT"

# Retención: últimas 7.
ls -1t "$BACKUP_DIR"/occb_"${MODE}"_*.dump.gz 2>/dev/null | tail -n +8 | xargs -r rm -f
echo "Backup OK: $OUT"
