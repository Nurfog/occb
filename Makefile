.PHONY: setup-dev deps deps-check net db-up db-down db-logs caddy-up caddy-down dev dev-down dev-logs frontend-logs stack-up stack-down migrate-dev migrate-prod backup-dev backup-prod check test fmt

# Requerimientos del sistema + stack (Arch, Debian/Ubuntu, Fedora/RHEL, openSUSE)
deps:
	./scripts/install-deps.sh --with-rust

deps-check:
	./scripts/install-deps.sh --check

setup-dev:
	./scripts/gen-secrets.sh dev

# Red principal del backend (db y caddy se le integran)
net:
	docker network inspect occb-dev >/dev/null 2>&1 || docker network create occb-dev

# Satélite DB: Postgres 18 + pgvector + Redis (red del backend)
db-up: net
	docker compose -f docker-compose.db.yml --env-file .env.dev up -d

db-down:
	docker compose -f docker-compose.db.yml --env-file .env.dev down

db-logs:
	docker compose -f docker-compose.db.yml --env-file .env.dev logs -f --tail=200 db

# Satélite Caddy: servidor web (red del backend)
caddy-up: net
	docker compose -f docker-compose.caddy.yml --env-file .env.dev up -d

caddy-down:
	docker compose -f docker-compose.caddy.yml --env-file .env.dev down

# Stack dev completo: red + db + migraciones + backend + caddy
dev: net db-up
	./scripts/migrate.sh dev
	docker compose -f docker-compose.dev.yml --env-file .env.dev up -d --build
	docker compose -f docker-compose.caddy.yml --env-file .env.dev up -d

stack-up: dev

dev-down:
	docker compose -f docker-compose.dev.yml --env-file .env.dev down
	docker compose -f docker-compose.caddy.yml --env-file .env.dev down

stack-down: dev-down db-down

dev-logs:
	docker compose -f docker-compose.dev.yml --env-file .env.dev logs -f --tail=200

frontend-logs:
	docker compose -f docker-compose.dev.yml --env-file .env.dev logs -f --tail=200 frontend

migrate-dev:
	./scripts/migrate.sh dev

migrate-prod:
	./scripts/migrate.sh prod

backup-dev:
	./scripts/backup-db.sh dev

backup-prod:
	./scripts/backup-db.sh prod

# Valida los compose (requiere .env.*)
check:
	docker compose -f docker-compose.dev.yml --env-file .env.dev config -q && echo "backend OK"
	docker compose -f docker-compose.db.yml --env-file .env.dev config -q && echo "db OK"
	docker compose -f docker-compose.caddy.yml --env-file .env.dev config -q && echo "caddy OK"
	docker compose -f docker-compose.prod.yml --env-file .env.prod config -q && echo "prod OK"

test:
	cd backend && cargo test --locked

fmt:
	cd backend && cargo fmt --all
