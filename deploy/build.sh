#!/usr/bin/env bash
# Compile le serveur et le front sur l'hôte, puis construit l'image du studio sans étape de
# compilation dans Docker (le disque système est trop petit pour une chaîne Rust dans une image).
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

cargo build --release -p dijkstravel-server
# Le front est facultatif : sans lui, l'API seule répond.
web_dist=${WEB_DIST:-web/dist/web/browser}
if [[ -f web/package.json && -z ${SKIP_WEB:-} ]]; then
    (cd web && npm ci && npx ng build)
fi
if [[ ! -d $web_dist ]]; then
    web_dist=$(mktemp -d)
    echo "Pas de build Angular : image construite sans interface."
fi

docker build -f deploy/Dockerfile \
    --build-context bin="$(realpath target/release)" \
    --build-context web="$(realpath "$web_dist")" \
    -t "dijkstravel-studio:${STUDIO_TAG:-local}" deploy
