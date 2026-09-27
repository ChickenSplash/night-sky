# Vendored crates

Local copies of upstream crates carrying a small fix. `[patch.crates-io]` in the
root `Cargo.toml` points at them. Search a copy for `TASK SPACE PATCH` to find
every change.

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
