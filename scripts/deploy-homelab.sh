#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
host=ecorreia@192.168.1.90
remote=/home/ecorreia/homelab/task-space
# Demo data by default. Pass --with-clickup to publish your credentials and cached tasks.
live=false
[[ ${1:-} == --with-clickup ]] && live=true
if $live; then
    test -f .env || { echo "Add ClickUp credentials to .env first." >&2; exit 1; }
fi
./scripts/build-web.sh
cargo build --locked --release -p space-server
mkdir -p deploy/bundle/web
cp target/release/space-server deploy/bundle/space-server
cp deploy/Dockerfile deploy/bundle/Dockerfile
cp -a web/dist/. deploy/bundle/web/
ssh "$host" "mkdir -p $remote/bundle $remote/data"
rsync -az --delete deploy/bundle/ "$host:$remote/bundle/"
scp -q deploy/compose.yaml "$host:$remote/compose.yaml"
if $live; then
    # Credentials travel over SSH and stay outside the Docker image/build context.
    scp -q .env "$host:$remote/.env"
    ssh "$host" "chmod 600 $remote/.env"
else
    ssh "$host" "rm -f $remote/.env $remote/data/space.sqlite3"
fi
if $live && [[ -f data/space.sqlite3 ]] && ! ssh "$host" "test -f $remote/data/space.sqlite3"; then
    python - <<'PY'
import sqlite3
with sqlite3.connect('data/space.sqlite3') as source:
    with sqlite3.connect('data/deploy-snapshot.sqlite3') as destination:
        source.backup(destination)
PY
    scp -q data/deploy-snapshot.sqlite3 "$host:$remote/data/space.sqlite3"
fi
ssh "$host" "docker compose --project-directory $remote up -d --build --wait"
curl --fail --silent --show-error http://192.168.1.90:4321/health
echo
echo 'Open http://192.168.1.90:4321/'
