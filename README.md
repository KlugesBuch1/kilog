# Kilog

Achievement Unlocker for Xbox & PC Games.

## Features

- Sign in with Microsoft (you'll stay logged in)
- Browse your games
- Search any game by Title ID or name
- Browse and filter a title's achievements, and unlock them from their cards
- Spoof played time for a title

## Settings

Currently, there's only one option:

- Force region (recommended): On, Xbox requests use `en-GB`. Off, they use the Windows locale.

## Data

Session and settings are stored under `Documents\Kilog`:

- `session.json` holds the Microsoft refresh token to keep you logged in
- `config.json` holds your settings config

## Build

Install Rust with the MSVC toolchain, then run the following:

```
cargo build --release
```

The executable is `target\release\kilog.exe`.

## License

[GPL-3.0](LICENSE). If you distribute Kilog or changes to it, you must share the source under the same license.
