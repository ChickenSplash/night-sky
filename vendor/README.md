# Vendored crates

Local copies of upstream crates carrying a small fix. `[patch.crates-io]` in the
root `Cargo.toml` points at them. Search a copy for `TASK SPACE PATCH` to find
the renderer change; the CEF changes use `NIGHT SKY PATCH`.

## bevy_cef_core 0.11.0

- **Problem:** CSP and DOM click handlers do not reliably block top-level
  navigation. The upstream plugin exposes load notifications but no pre-load
  navigation policy, so a post-load URL check is too late for a trusted UI.
- **Change:** the opt-in `trusted-inline-only` feature adds a CEF request handler
  that permits only `cef://localhost/__inline__/{id}` documents, denies opening
  URLs from a tab, and denies new browser windows with a lifespan handler.
  Enabled only by the native client's optional `cef-ui` dependency. CSP still
  blocks external subresources, frames, workers and forms in the bundled page.
  This is an experiment-specific policy, not an arbitrary-site browser sandbox.
- **Changed files:** `Cargo.toml`, `src/browser_process/client_handler.rs`, and
  new `src/browser_process/client_handler/trusted_inline.rs`. The remaining files
  are the crates.io 0.11.0 source (Cargo cache metadata omitted). The renderer helper does not need this patch:
  navigation callbacks run in the main application's browser process.
- **Regression check:** feature-enabled client tests and the core policy unit
  test. Manual focus, paste and cross-boundary drag checks remain user-run.
- **Upgrading:** revalidate callback signatures, the inline URL format and
  navigation denial before changing the pinned CEF/plugin versions.

## bevy_render 0.18.1

- **Problem:** `prepare_windows` panics with "Couldn't get swap chain texture,
  operation unrecoverable: A timeout was encountered" when a window returns from
  a hidden Hyprland workspace on NVIDIA (Wayland, driver 615). Upstream tolerates
  the timeout only on AMD and Intel, and still did on `main` in September 2026.
- **Change:** `Timeout` and `Other` skip the frame, and `Lost` recreates the
  surface, with a rate-limited warning. `OutOfMemory` still panics.
  Only `src/view/window/mod.rs` differs from the crates.io release.
- **Regression check:** `./scripts/test-hidden-window.sh` (Hyprland only).
- **Upgrading Bevy:** copy the new `bevy_render` release here and re-apply the
  patch, or delete this copy and the `[patch.crates-io]` entry once upstream
  stops panicking on NVIDIA.
