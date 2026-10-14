# Galaxy backend

The Axum server (`crates/server`) serves only `web/dist/` and the built-in demo
galaxy. There is no live task integration. Both clients share serializable
models from `crates/shared` and engine-independent generation from
`crates/simulation`. See [lists and tasks](../shared/information-overlay.md)
for the demo's information overlay.

## Serving and configuration

- The server binds to `127.0.0.1:4321` by default. `SPACE_BIND` overrides the
  address; `SPACE_ROOT` overrides the project root used to locate `web/dist/`.
- Binding to a non-loopback address exposes the service to that network.
  Configure access controls and HTTPS separately for public deployment.
- `GET /health` reports service health.
- `GET /galaxy` serves the demo snapshot, with ETags and HTTP 304 for
  unchanged data.
- The native client polls every two seconds. It can run alone with
  `cargo run --locked -p space-client`, retaining demo data without a backend.
- Browser requests use same-origin `/galaxy`; never bake a LAN IP or token
  into WASM. Any future credentials belong only in the backend, never API
  models or logs.
- `scripts/run.sh` writes backend output to `data/server.log`.

The [native developer API](../client/developer-api.md) is a different server,
inside the client, fixed to loopback and normally on port 4322.
Build/run prerequisites are in [development](../development.md).
