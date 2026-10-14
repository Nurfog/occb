#!/usr/bin/env bash
# Aplica backend/migrations/*.sql en orden contra el contenedor Postgres.
# Uso: ./scripts/migrate.sh [dev|prod]
# No requiere psql en el host: opera vía docker exec en occb-db-<modo>.
set -euo pipefail

MODE="${1:-dev}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="$ROOT/.env.$MODE"
DB_CONTAINER="occb-db-$MODE"
DB_COMPOSE="$ROOT/docker-compose.db.yml"

[[ -f "$ENV_FILE" ]] || { echo "No existe $ENV_FILE. Ejecuta ./scripts/gen-secrets.sh $MODE"; exit 1; }
if [[ "$MODE" != "dev" && ! -f "$DB_COMPOSE" ]]; then
  echo "En prod la DB vive en su propio proyecto: aplica backend/migrations con psql hacia su DATABASE_URL."
  exit 1
fi

set -a; # shellcheck disable=SC1090
source "$ENV_FILE"; set +a

if ! docker ps --format '{{.Names}}' | grep -qx "$DB_CONTAINER"; then
  echo "El contenedor $DB_CONTAINER no está en marcha. Levántalo con: make db-up"
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

for f in "$ROOT"/backend/migrations/*.sql; do
  echo "== aplicando $(basename "$f")"
  docker compose -f "$DB_COMPOSE" --env-file "$ENV_FILE" exec -T db \
    psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
    --quiet --set ON_ERROR_STOP=on --single-transaction -f - < "$f"
done
echo "Migraciones OK."
