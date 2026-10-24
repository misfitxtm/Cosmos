# Security audit

Date: 2026-10-23. Scope: everything in `crates/` (core, out, cli, ui) at Steps 0-7. Method: threat model, read every
path that touches outside input, grep for panics, unbounded reads and link-following, then test each claim.
Not covered: `hypora-theme` itself (a separate project), GTK and the image decoders' own code (relied on, see "Residual risk").

## Threat model

| Who feeds Cosmos | What they control | What they want |
|---|---|---|
| A wallpaper author | image bytes, file name | crash, hang or exhaust memory; escape the config dir; inject into a config file |
| Someone who shares a theme, config or template pack | `colors.toml` used as `--base`, `config.toml`, templates, hook commands | overwrite files, run commands |
| A script calling `cosmos` | theme name, flags | traversal or injection through the name |
| A supply-chain attacker | a dependency | code running as the user |

What an attacker gains is the user's own privileges. Cosmos never runs as root and never asks for sudo.

## Findings

| # | Severity | Finding | Status |
|---|---|---|---|
| 1 | Medium | A shared `config.toml` could set a template `dest` anywhere (`/etc/...`, `~/.ssh/authorized_keys`, `~/../x`) and overwrite it with template output | Fixed: `dest` must be inside home, no `..`, not home itself. Tests in `config.rs` and `cli.rs` |
| 2 | Medium | Reads of config-controlled files had no size cap and no regular-file check: template `src` (`/dev/zero` would eat all memory), `config.toml`, `colors.toml.bak`, `current/name`, the wallpaper pointer | Fixed: one helper, `fileio::read_limited`, used everywhere. Tests in `fileio.rs`, `cli.rs` |
| 3 | Low | `File::open` on a FIFO blocks forever, and the "regular files only" check ran after the open. `cosmos some.fifo` hung | Fixed: stat first, then open, then re-check the handle. FIFO tests for wallpaper, template and `read_limited` |
| 4 | Low | The decoder memory cap (512 MiB) allowed a 432 MB image costing ~500 MB RAM and 3.4 s | Fixed: cap is 256 MiB. Measured after: 12000x12000 refused in 0.00 s; worst accepted (268 MB gray) 2.5 s, 368 MB |
| 5 | Low | Template scanner searched to the end of the text for a `}` after every `{`: quadratic on a hostile template | Fixed: looks ahead 64 bytes only. Test: 2 million `{` in under 5 s |
| 6 | Low | Replacing a theme used `exists()` (follows links) then read it: a symlinked `colors.toml` was read and copied to `.bak` | Fixed: `symlink_metadata`; a link is refused (`Unsafe`). Test added |
| 7 | Low | File names and error text could carry terminal escape codes into the user's terminal | Fixed: control characters are shown as `?`. Test with a real ESC file name |
| 8 | Low | `--base` accepted an empty font or icon theme | Fixed: rejected |
| 9 | Info | Hooks were not shown before running | Fixed: `--dry-run` lists them, a real run prints them first |
| 10 | Info | Only PNG was fuzzed; JPEG and WebP had no tests with real files | Fixed: all three formats tested; 3x3000 mutations per run, plus an opt-in 450,000-mutation run (`cargo test --release -- --ignored`), no panics |
| 11 | Info | Icon test only banned scripts and links | Extended: `url(http`, `@import`, `data:`, `<use` |

No critical or high findings. No panics reachable from input: there is no `unwrap`, `expect` or `panic!` outside tests, and `unsafe_code` is forbidden in every crate.

## Checked and found sound

- **Theme names:** `ThemeName` (1-64 chars, alphanumeric start, `[A-Za-z0-9 ._-]`) is the only way a name reaches a path, the TOML file or `hypora-theme`'s argument. Hostile names (`../x`, `a;b`, `$(...)`, newlines, `x|e`) are refused before anything is read or written.
- **Values in `colors.toml`:** only `#rrggbb` (a `Hex` type that cannot hold anything else) or plain names reach the file `hypora-theme` feeds to `sed`. The wallpaper file name is never written into it.
- **Subprocesses:** `Command` with separate arguments, never a shell; stdin closed so nothing waits on a hidden prompt or sudo; hooks only from the user's own config.
- **Writes:** temp file with `O_EXCL`, fsync, rename; symlinks at the destination are replaced, never followed; directories are checked with `symlink_metadata`; `colors.toml` goes last so a failure leaves no listed theme; the base theme is validated before anything is created.
- **Wallpaper bytes** are read once, bounded, decoded behind dimension and memory caps, and the same bytes are stored.
- **Rollback:** a failed apply restores the previous theme (and the old `colors.toml` and wallpaper pointer when needed); tested with stand-ins and once for real.
- **Dependencies:** `cargo deny` clean (advisories, licences, sources, bans); lockfile committed, builds with `--locked`; decoders limited to jpeg, png, webp (all pure Rust).

## Residual risk (accepted or open)

1. **`config.toml` is code.** A hook is an arbitrary command, and a template can write any file inside your home. That is the point of hooks. Treat a config someone else wrote like a shell script: read it first. Never auto-import one. Cosmos now shows every hook before it runs.
2. **Image decoder bugs.** Cosmos is memory-safe, but a bug in `image`, `png`, `zune-jpeg` or `image-webp` could still panic or hang. Mitigations: size and memory caps, 450k-mutation fuzzing. Not mitigated: a pathological but legal image costing CPU (up to ~3 s measured), and no timeout. Real coverage-guided fuzzing (`cargo fuzz`) is still worth doing before a release.
3. **Same-user races.** Between a directory check and a write, another process running as you could swap a directory for a link. That needs the same privileges Cosmos already has, so it is not a boundary crossing.
4. **`hypora-theme` is trusted.** Cosmos finds it on `PATH`. Whoever controls your `PATH` controls it. Its own `sed`-based substitution is outside this audit; Cosmos only gives it validated values.
5. **Hooks have no timeout.** A hook that never exits hangs Cosmos (marked `shortcut:` in `run.rs`).
6. **GUI has no automated tests.** Its logic lives in tested crates, but window code was checked by hand (screenshots, bad-image path).
7. **Update path not built.** Step 6 (signed repo and updater) is on hold; until then there is no signature check on installs. When it is built, pin the key fingerprint in the installer.
8. **Template escape syntax missing.** A literal `{name}` cannot be written in a template (marked `shortcut:`).
