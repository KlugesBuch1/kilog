# Contributing

Bug fixes, event-template work, and small features are welcome.

## Setup

kilog builds on Windows with [Rust](https://rustup.rs/) and the MSVC toolchain.

```powershell
cargo build --release
cargo test
```

The release binary is `target\release\kilog.exe`.

To open the UI without a Microsoft sign-in:

```powershell
cargo run -- --dev-mock
```

Spoofing and auto unlock still need the Xbox PC app open and signed in.

## Layout

| Path | What lives there |
| --- | --- |
| `src/ui` | Screens and theme |
| `src/auth` | Microsoft sign-in and the saved session |
| `src/xbox` | Achievements, stats, presence, spoofing, and event unlocks |
| `src/config.rs` | Settings stored in `Documents\kilog` |

## Event-based games

On startup, kilog fetches the event catalog into `Documents\kilog\events`. A title can be unlocked only when both its template and `data.json` entry are present. The unlock path is `src/xbox/events.rs`.

Templates are largely the work of [ethanwp28](https://github.com/ethanwp28).

## Pull requests

- Keep each change focused.
- Match the style of the surrounding code.
- Run `cargo test` before opening the request.
- Do not commit `session.json`, tokens, logs, or anything from `Documents\kilog`.

## License

kilog is [GPL-3.0](LICENSE). Contributions are licensed under the same terms.
