<div align="center">
  <img src="https://lh3.googleusercontent.com/d/1_MrK6BBbaqqx0lf_0NDC8F6UDU6WyWUF" alt="Made with Vibe Coding" width="50%" height="" />
</div>

# nodewolf

Rust TUI that walks a drive, finds every `node_modules` folder, and lets you delete the ones you don't need. Built for cleaning up orphaned `node_modules` left behind after projects were moved/deleted.

```
nodewolf [PATH]          # interactive TUI (PATH default: current dir)
nodewolf --list [PATH]   # print results and exit
nodewolf --help
```

## Build

```nu
cargo build --release
# binary: target/release/nodewolf.exe
```

No global install needed. If you want it on PATH, copy the exe somewhere in `$env.PATH`.

## Keys

| key                | action                                          |
| ------------------ | ----------------------------------------------- |
| `j` / `k` / arrows | move cursor                                     |
| `space`            | toggle select (and move down)                   |
| `a` / `n`          | select all visible / none                       |
| `d`                | delete selected (asks `y`)                      |
| `o`                | show only folders with no `package.json`        |
| `s`                | cycle sort: size desc/asc, oldest, newest, path |
| `/`                | filter by text, `enter`/`esc` to close          |
| `r`                | rescan                                          |
| `q`                | quit                                            |

## What it shows

- `SIZE` - real on-disk size (sum of file sizes inside `node_modules`), not link count
- `AGE` - newest file modification inside the folder
- `PROJECT` - parent folder name
- `PATH` - relative to scan root
- `PKG` - `NO-PKG` (red) means the parent has no `package.json` → probably orphaned

Nothing is deleted automatically. You pick rows, press `d`, confirm.

## Delete behavior (important)

Deletes are **permanent** — `std::fs::remove_dir_all`, no Recycle Bin, no undo. The folder is gone the moment you press `y`. Selection is confirmed first, but there is no second chance.

## Portability

Built as `x86_64-pc-windows-msvc` with a **static CRT** (`.cargo/config.toml` → `+crt-static`), so the exe only imports `KERNEL32.dll`. It runs on any 64-bit Windows 10/11 without the VC++ redistributable.

- 32-bit Windows: needs `rustup target add i686-pc-windows-msvc` and `cargo build --release --target i686-pc-windows-msvc`.
- Windows 7/8: not tested, UCRT/console API assumptions from `crossterm` may bite; assume Win10+.

## Safety

- Only folders literally named `node_modules` can be deleted (checked in code, even in the delete worker thread).
- One confirmation for the whole selection, showing count + space to be freed.
- Read-only files are unlocked before retry, so Windows permission junk doesn't fail the delete.
- Delete runs on a background thread; the UI stays alive. Failures show as red rows with the error instead of dying silently.
- Symlinks/junctions are not followed, depth is capped at 48, and `$RECYCLE.BIN` / `System Volume Information` / `.git` are skipped.

## Testing without touching real projects

```nu
nu scripts/mock-tree.nu                         # builds $env.TEMP/nodewolf-mock
cargo run --release -- --list $env.TEMP/nodewolf-mock
cargo run --release -- $env.TEMP/nodewolf-mock  # play with the TUI here
cargo test                                      # 8 tests, all on mock dirs in $env.TEMP
```

The mock tree contains 3 orphans, 2 healthy projects, and a `node_modules` nested inside another `node_modules` (which must not be double-counted).

## Limits

- Scanning a whole drive is IO-bound: expect minutes on a busy HDD; `SIZE` requires reading every file entry inside each `node_modules`.
- Very deep trees are not indexed; the scan is a plain DFS, results stream in as found (sorted live in the UI).
- No undo. `node_modules` is regenerable, that's the point.
