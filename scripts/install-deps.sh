#!/usr/bin/env bash
# Requerimientos del sistema + stack de contenedores para OCCB backend.
# Soporta: Arch (+CachyOS/EndeavourOS/Manjaro), Debian/Ubuntu y derivados,
# Fedora/RHEL/Rocky/Alma, openSUSE.
#
# Uso:
#   ./scripts/install-deps.sh [--check] [--with-rust] [--yes]
#     --check       audita SO + stack sin instalar nada (no necesita root)
#     --with-rust   instala también Rust vía rustup (solo para cargo local)
#     --yes         no pregunta confirmación
#
# Postgres vive en un CONTENEDOR (no se instala cliente en el host: los
# scripts operan vía docker exec). El instalador crea la red "occb-dev"
# (la principal, del backend) y levanta en ella los contenedores DB y Caddy.
# El backend se levanta con: make dev

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NET_DEV="occb-dev"
DB_CONTAINER="occb-db-dev"
CADDY_CONTAINER="occb-caddy-dev"

CHECK=0
WITH_RUST=0
ASSUME_YES=0
for arg in "$@"; do
  case "$arg" in
    --check) CHECK=1 ;;
    --with-rust) WITH_RUST=1 ;;
    --yes|-y) ASSUME_YES=1 ;;
    -h|--help)
      sed -n '2,15p' "$0"
      exit 0 ;;
    *) echo "Opción desconocida: $arg (usa --help)"; exit 1 ;;
  esac
done

OS_ID="$(grep -E '^ID=' /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"' || echo unknown)"
OS_LIKE="$(grep -E '^ID_LIKE=' /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"' || echo "")"

have() { command -v "$1" >/dev/null 2>&1; }

# ---------- chequeo de stack (contenedores) ----------
check_stack() {
  local fail=0
  if docker network inspect "$NET_DEV" >/dev/null 2>&1; then
    echo "OK      red $NET_DEV"
  else
    echo "FALTA   red $NET_DEV (la crea el backend o make net)"; fail=1
  fi
  if docker ps --format '{{.Names}}' 2>/dev/null | grep -qx "$DB_CONTAINER"; then
    local health
    health="$(docker inspect --format '{{.State.Health.Status}}' "$DB_CONTAINER" 2>/dev/null || echo "?")"
    if [[ "$health" == "healthy" ]]; then echo "OK      $DB_CONTAINER (healthy)";
    else echo "FALTA   $DB_CONTAINER está '$health', no healthy"; fail=1; fi
  else
    echo "FALTA   contenedor $DB_CONTAINER en marcha"; fail=1
  fi
  if docker ps --format '{{.Names}}' 2>/dev/null | grep -qx "$CADDY_CONTAINER"; then
    echo "OK      $CADDY_CONTAINER (en marcha)"
  else
    echo "FALTA   contenedor $CADDY_CONTAINER en marcha"; fail=1
  fi
  return $fail
}

# ---------- modo check ----------
if [[ "$CHECK" -eq 1 ]]; then
  missing=0
  echo "--- SO ---"
  for tool in docker openssl curl make git; do
    if have "$tool"; then echo "OK      $tool";
    else echo "FALTA   $tool"; missing=1; fi
  done
  if docker compose version >/dev/null 2>&1; then echo "OK      docker compose";
  else echo "FALTA   docker compose (plugin v2)"; missing=1; fi
  if [[ "$WITH_RUST" -eq 1 ]]; then
    if have cargo && have rustc; then echo "OK      rust ($(rustc --version 2>/dev/null || echo ?))";
    else echo "FALTA   rust/cargo"; missing=1; fi
  fi
  echo "--- stack (contenedores en red $NET_DEV) ---"
  check_stack || missing=1
  if [[ "$missing" -eq 0 ]]; then echo "Todo instalado y en marcha.";
  else echo "Falta algo: ejecuta $0 [--with-rust] para instalar y levantar el stack."; exit 1; fi
  exit 0
fi

# ---------- instalación ----------
SUDO=""
if [[ "$(id -u)" -ne 0 ]]; then
  if have sudo; then SUDO="sudo";
  else echo "Se necesita root o sudo para instalar."; exit 1; fi
fi

confirm() {
  [[ "$ASSUME_YES" -eq 1 ]] && return 0
  read -r -p "$1 (y/N) " ans
  [[ "$ans" == "y" || "$ans" == "Y" ]]
}

enable_docker() {
  if have systemctl; then
    $SUDO systemctl enable --now docker 2>/dev/null || true
  fi
  if ! docker info >/dev/null 2>&1; then
    echo "AVISO: no puedes usar docker aún. Añádete al grupo y reingresa:"
    echo "  $SUDO usermod -aG docker \$USER  &&  newgrp docker"
  fi
}

install_rust() {
  if have cargo; then echo "Rust ya instalado: $(rustc --version)"; return 0; fi
  if [[ "$OS_ID" == "arch" ]] || [[ "$OS_LIKE" == *"arch"* ]]; then
    $SUDO pacman -S --needed --noconfirm rustup && rustup toolchain install stable
  else
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    echo "Añade a tu PATH: \$HOME/.cargo/bin (o reingresa a la shell)."
  fi
}

# NOTA: no se instala postgresql-client en el host. Postgres vive en un
# contenedor y los scripts (migrate/backup/restore) operan vía docker exec.

if [[ "$OS_ID" == "arch" ]] || [[ "$OS_LIKE" == *"arch"* ]]; then
  confirm "Instalar con pacman: docker docker-compose openssl curl make git?" || exit 0
  $SUDO pacman -Syu --needed --noconfirm docker docker-compose openssl curl make git
elif [[ "$OS_ID" == "debian" || "$OS_ID" == "ubuntu" ]] || [[ "$OS_LIKE" == *"debian"* || "$OS_LIKE" == *"ubuntu"* ]]; then
  confirm "Instalar con apt: docker.io docker-compose-plugin openssl curl make git?" || exit 0
  $SUDO apt-get update
  $SUDO apt-get install -y docker.io docker-compose-plugin openssl curl make git
elif [[ "$OS_ID" == "fedora" ]] || [[ "$OS_LIKE" == *"fedora"* || "$OS_LIKE" == *"rhel"* ]] || [[ "$OS_ID" =~ ^(rhel|rocky|almalinux|centos)$ ]]; then
  if [[ "$OS_ID" == "fedora" ]]; then
    confirm "Instalar con dnf: moby-engine docker-compose openssl curl make git?" || exit 0
    $SUDO dnf install -y moby-engine docker-compose openssl curl make git
  else
    echo "RHEL/Rocky/Alma: instala Docker desde https://docs.docker.com/engine/install/rhel/"
    confirm "¿Continuar instalando el resto (openssl curl make git)?" || exit 0
    $SUDO dnf install -y openssl curl make git
  fi
elif [[ "$OS_ID" == *"suse"* || "$OS_LIKE" == *"suse"* ]]; then
  confirm "Instalar con zypper: docker docker-compose openssl curl make git?" || exit 0
  $SUDO zypper install -y docker docker-compose openssl curl make git
else
  echo "Distro no reconocida (ID=$OS_ID LIKE=$OS_LIKE)."
  echo "Instala manualmente: docker + docker compose v2, openssl, curl, make, git."
  exit 1
fi

enable_docker
[[ "$WITH_RUST" -eq 1 ]] && install_rust

# ---------- stack: red + DB + Caddy en la red del backend ----------
cd "$ROOT"
echo "== Red principal del backend: $NET_DEV"
docker network inspect "$NET_DEV" >/dev/null 2>&1 || docker network create "$NET_DEV"

[[ -f .env.dev ]] || ./scripts/gen-secrets.sh dev

echo "== Contenedor Postgres 18 + pgvector (+ Redis) en red $NET_DEV"
docker compose -f docker-compose.db.yml --env-file .env.dev up -d
echo "== Caddy (servidor web) en red $NET_DEV"
docker compose -f docker-compose.caddy.yml --env-file .env.dev up -d

echo "Listo. El backend se levanta con: make dev"
