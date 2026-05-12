# Contributing to RainbowTerm

Thanks for your interest in contributing. This document covers the parts of the
workflow that aren't obvious from the source tree alone.

## Development loop

```bash
cargo build                     # Fast iteration
cargo test                      # Unit + integration (~1s)
cargo clippy --lib --bin rt     # Lint the code we ship
cargo bench                     # Criterion benchmarks
```

For changes that affect colorization, also run the sample files visually:

```bash
for f in tests/networking/juniper/common/*.conf \
         tests/networking/juniper/ex/*/*.conf \
         tests/networking/juniper/qfx/*/*.conf \
         tests/networking/cisco/ios/*.conf \
         tests/networking/versa/sdwan/*.conf; do
    echo "=== $f ==="
    cat "$f" | target/debug/rt
done
```

The `tests/networking/` samples are realistic vendor output; they are the
source of truth for visual correctness. Add to them whenever you introduce a
new pattern or fix a regression.

## Modifying `config.toml`

The embedded config is baked into the binary via `include_str!()` at compile
time. After any change:

1. **Bump the version header** at `config.toml:3`:

   ```toml
   # Config version: X.Y.Z (YYYY-MM-DD)
   ```

2. **Register the new hash** in `src/versions.rs::KNOWN_HASHES` so the
   `--update-config` smart-merge flow recognizes it as a known stock version:

   ```bash
   cargo install --path .
   rt --config-hash
   # Copy the "Embedded config hash" value
   ```

   Add the entry:

   ```rust
   m.insert("X.Y.Z", "the-hash-you-just-printed");
   ```

3. **Sync your local install** so subsequent testing uses the new embedded
   patterns (the installed config at the platform path takes precedence over
   the project's `config.toml`):

   ```bash
   cargo build --release && cargo install --path .
   rt --update-config        # or --force to skip prompts
   ```

4. **Keep `Cargo.toml` version in sync** if this is a release.

### Why the version bump matters

Users have their own `config.toml` at a platform-specific path:

| OS       | Path                                                  |
|----------|-------------------------------------------------------|
| macOS    | `~/Library/Application Support/rainbowterm/config.toml` |
| Linux    | `~/.config/rainbowterm/config.toml`                   |
| Windows  | `%APPDATA%\rainbowterm\config.toml`                   |

`rt --update-config` uses the blake3 hash registry in `versions.rs` to detect
whether the user is running an unmodified stock config (auto-update safe) or
a customized one (prompts with merge/replace/keep). A missing hash entry means
every user is prompted even if their config is untouched.

## Adding a new pattern

1. Run existing sample files to establish baseline coloring.
2. Add the pattern to `config.toml` under the appropriate profile.
3. Set priority (see `config.toml` comments; 200+ = critical, 100 = interfaces,
   90 = protocol states, ...).
4. Rebuild, run `rt --update-config`, rerun samples.
5. Visually compare before/after — automated tests don't catch color regressions.
6. Add realistic output to the sample file that exercises the new pattern.

## Adding a new profile

Profiles are pure config — no Rust changes needed:

1. Add `[profiles.<vendor>]` at the top level of `config.toml`.
2. Set `inherits = ["base"]` to pick up universal patterns (IPs, MACs, ...).
3. Add auto-detection rules under `[[profiles.<vendor>.auto_detect]]` with
   types `banner`, `prompt`, `interface_pattern`, or `hostname_prefix`.
4. Create `tests/networking/<vendor>/<model>/sample1.conf` with realistic
   device output.
5. Add an integration test block in `tests/integration_tests.rs`.

## Error handling

Follow the project error style: use `anyhow::Result` with `.with_context()`
for anything a user could plausibly hit. Include the path, the operation, and
enough detail that the user can act:

```rust
std::fs::read_to_string(&path)
    .with_context(|| format!("reading user config {}", path.display()))?
```

For invalid regex in user config, fail fast at startup (`anyhow::bail!`)
rather than warn — warnings are easy to miss in piped output.

## Commit style

- Imperative subject line, under 70 chars
- Body explains *why* (the what is visible in the diff)
- Co-author Claude contributions when relevant

## Running the full pre-push check

```bash
cargo build --release           # LTO, stripped, benchmarking build
cargo test                      # All tests green
cargo clippy --lib --bin rt     # Clean on shipped code
cargo install --path .          # Local install
rt --update-config              # Sync installed config
# visual pass across tests/networking/
```

## Reporting security issues

See [SECURITY.md](SECURITY.md).
