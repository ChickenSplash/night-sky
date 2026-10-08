#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
host=ecorreia@192.168.1.90
remote=/home/ecorreia/homelab/night-sky
./scripts/build-web.sh
cargo build --locked --release -p space-server
rm -rf deploy/bundle/web
mkdir -p deploy/bundle/web
cp target/release/space-server deploy/bundle/space-server
cp deploy/Dockerfile deploy/bundle/Dockerfile
cp -a web/dist/. deploy/bundle/web/
ssh "$host" "mkdir -p $remote/bundle $remote/data"
rsync -az --delete deploy/bundle/ "$host:$remote/bundle/"
scp -q deploy/compose.yaml "$host:$remote/compose.yaml"
# Removes files left by the former ClickUp integration.
ssh "$host" "rm -f $remote/.env $remote/data/space.sqlite3"
ssh "$host" "docker compose --project-directory $remote up -d --build --wait"
curl --fail --silent --show-error http://192.168.1.90:4321/health
echo
echo 'Open http://192.168.1.90:4321/'
