# kilog

Achievement Unlocker for Xbox & PC Games.

## Features

- Sign in with Microsoft (you'll stay logged in)
- Browse your games
- Search any game by Title ID or name
- Browse and filter a title's achievements, and unlock them from their cards
- Build an unlock queue from a reference gamertag, edit the delays and skips, and auto unlock while that game is spoofed
- Spoof played time for a title

## Settings

- Force region (recommended): On, Xbox requests use `en-GB`. Off, they use the Windows locale.
- Randomize Delays: Each tier can be turned on and sets its own range. Delays over 1 minute, 10 minutes, or 1 hour are shifted up or down by a random amount in that range. Delays over 5 hours are replaced by a random time inside the range.

On Auto Unlock, **Stop unlocking when spoofing fails** and **Use randomized delays** are saved in `config.json` as well.

## Data

Session and settings are stored under `Documents\Kilog`:

- `session.json` holds the Microsoft refresh token to keep you logged in
- `config.json` holds your settings config
- `queue.json` holds the auto unlock queue so a stopped run can resume

## Build

Install Rust with the MSVC toolchain, then run the following:

```
cargo build --release
```

The executable is `target\release\kilog.exe`.

## License

[GPL-3.0](LICENSE). If you distribute kilog or changes to it, you must share the source under the same license.
