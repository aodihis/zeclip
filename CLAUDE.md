# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

Run from the workspace root (`/workspace/rust/viewclipboard`):

```bash
cargo build                        # debug build
cargo build --release              # release build
cargo check                        # fast type-check, no binary
cargo clippy -- -D warnings        # lint — must be zero warnings
cargo fmt                          # format in-place
cargo fmt --check                  # format check (CI)
cargo test                         # run all tests
cargo test <test_name>             # run a single test
RUST_LOG=debug cargo tauri dev     # run app with debug logging (requires tauri-cli)
cargo tauri dev                    # run in development mode
cargo tauri build                  # build release bundle
```

## Project overview

`zeclip` is a Tauri desktop application (Windows). It reads the system clipboard, enumerates all available clipboard formats, detects each content type, and displays the formatted output in a window. The Rust backend handles clipboard access and parsing; the frontend is plain HTML/CSS/JS.

## Directory layout

```
src-tauri/
  src/
    main.rs       — Tauri entry point, register read_clipboard() command
    clipboard.rs  — ClipboardData + read() via clipboard-win (Windows only)
    parser.rs     — ParsedContent enum + detect_and_parse()
    formatter.rs  — format(ParsedContent) -> String
  Cargo.toml      — crate dependencies
  build.rs        — tauri-build
  tauri.conf.json — Tauri app configuration
  capabilities/
    default.json  — Tauri permission capabilities
index.html        — frontend (no bundler, uses window.__TAURI__.core.invoke)
Cargo.toml        — workspace root (members = ["src-tauri"])
```

**Rules:**
- `main.rs` contains only Tauri wiring and `#[tauri::command]` handlers — no business logic.
- Every public function returns `Result<T, anyhow::Error>` and never panics.
- No `unwrap()` or `expect()` anywhere except in `main()` for the Tauri runner; propagate errors with `?`.
- Use `tracing` for all logging (`tracing::debug!`, `tracing::info!`, `tracing::error!`).
- Add `#[instrument]` to public functions.
- This is a Windows-only application; `clipboard-win` uses Win32 APIs directly.

## Dependencies

| Crate | Purpose |
|---|---|
| `tauri` | Desktop app shell and JS ↔ Rust IPC |
| `clipboard-win` | Low-level Windows clipboard format enumeration |
| `quick-xml` | XML parsing and pretty-printing |
| `anyhow` | Error propagation |
| `tracing` + `tracing-subscriber` | Structured logging |
| `serde` + `serde_json` | Tauri command serialization |

Add new dependencies only when necessary. Prefer small, focused crates.

## Frontend

`index.html` is served as the Tauri frontend (`frontendDist: "../"` in `tauri.conf.json`).
It uses `window.__TAURI__.core.invoke('read_clipboard')` (enabled by `withGlobalTauri: true`).
No npm, no bundler — just plain HTML/CSS/JS.

## Phase implementation status

| Phase | Features | Status |
|---|---|---|
| 1 | Plain text, XML pretty-print, FileMaker formats | **Done** |
| 2 | HTML → Markdown, RTF | Pending |
| 3 | Image metadata, binary hex-dump | Pending |

## Adding Phase 2 (HTML + RTF)

1. Extend `parser.rs`:
   - New `ContentKind::Html(String)` variant.
   - In `parse_entry`, handle `entry.format_name == "HTML Format"` by converting HTML → Markdown using `htmd = "0.1"` or `html2md = "0.2"`.
2. Extend `formatter.rs` to handle the new variant.

## Adding Phase 3 (Image + binary hex-dump)

1. Extend `clipboard.rs` / `parser.rs`:
   - New `ContentKind::Image { format: String, width: u32, height: u32 }` variant.
   - Use `image = "0.25"` crate to detect format/dimensions from raw bytes.
2. Extend `formatter.rs`:
   - Image: one-line summary `[PNG 1920×1080]`.
   - Binary: MIME type, byte count, hex dump of first 256 bytes (16 bytes per row).

## Coding standards

- Rust edition 2024.
- Use `anyhow` for all error handling; use `thiserror` only if custom error types are needed.
- Run `cargo clippy -- -D warnings` and `cargo fmt --check` before every commit.
- Keep module responsibilities strictly separated — no clipboard logic in `parser.rs`, etc.
