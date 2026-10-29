#!/usr/bin/env bash
# Aplica backend/migrations/*.sql en orden contra el contenedor Postgres.
# Uso: ./scripts/migrate.sh [dev|prod]
# - dev:  usa docker compose (servicio db de docker-compose.db.yml).
# - prod: usa `docker exec occb-db-prod` (la DB prod vive en su proyecto).
# No requiere psql en el host.
set -euo pipefail

MODE="${1:-dev}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="$ROOT/.env.$MODE"
DB_CONTAINER="occb-db-$MODE"

[[ -f "$ENV_FILE" ]] || { echo "No existe $ENV_FILE. Ejecuta ./scripts/gen-secrets.sh $MODE"; exit 1; }

set -a; # shellcheck disable=SC1090
source "$ENV_FILE"; set +a

if ! docker ps --format '{{.Names}}' | grep -qx "$DB_CONTAINER"; then
  if [[ "$MODE" == "dev" ]]; then
    echo "El contenedor $DB_CONTAINER no está en marcha. Levántalo con: make db-up"
  else
    echo "El contenedor $DB_CONTAINER no está en marcha (proyecto occb-db en el servidor prod)."
  fi
  exit 1
fi

echo "Esperando a que Postgres esté healthy..."
for _ in $(seq 1 60); do
  status="$(docker inspect --format '{{.State.Health.Status}}' "$DB_CONTAINER" 2>/dev/null || echo starting)"
  [[ "$status" == "healthy" ]] && break
  sleep 2
done
[[ "$(docker inspect --format '{{.State.Health.Status}}' "$DB_CONTAINER")" == "healthy" ]] \
  || { echo "Postgres no llegó a healthy a tiempo."; exit 1; }

if [[ "$MODE" == "dev" ]]; then
  # shellcheck disable=SC2086
  PSQLEXEC="docker compose -f $ROOT/docker-compose.db.yml --env-file $ENV_FILE exec -T db psql"
else
  PSQLEXEC="docker exec -i $DB_CONTAINER psql"
fi

for f in "$ROOT"/backend/migrations/*.sql; do
  echo "== aplicando $(basename "$f")"
  $PSQLEXEC -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
    --quiet --set ON_ERROR_STOP=on --single-transaction -f - < "$f"
done
echo "Migraciones OK."
