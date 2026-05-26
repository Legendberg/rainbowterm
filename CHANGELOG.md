# Changelog

All notable changes to RainbowTerm will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.26] - 2026-05-26

### Fixed
- **Hostname-prefix auto-detect missed non-interactive command output** - The
  output of `ssh host "show ddos-protection ..."` (or any non-interactive
  remote command) typically contains no banner and no prompt — only the
  command's output. `[hostname_prefixes]` rules in `config.toml` are
  text-content based, so they had nothing to match against and detection
  fell back to `base`. Detection now also takes a hostname hint from
  `--hostname <HOST>` or the `RT_HOSTNAME` environment variable, so
  wrappers like `ossh js0391-mdf-2-b "show ..."` correctly auto-detect
  as `juniper`.

### Added
- **`--hostname <HOST>` flag** - Pass the target hostname into auto-detection
  explicitly. Useful for non-interactive SSH command output and any other
  case where the output stream itself contains no hostname signal.
- **`RT_HOSTNAME` environment variable** - Same hint as `--hostname`. Picked
  up automatically by the rt pipeline; useful for shell wrappers that
  pre-resolve the target hostname (e.g. `ossh`, `opssh`).

## [0.2.25] - 2026-05-13

### Fixed
- **UTF-8 buffer slicing corruption** - `String::from_utf8_lossy` replaces each
  invalid byte with a 3-byte U+FFFD, so byte-length arithmetic on the lossy
  string could not index the raw buffer correctly. Any non-UTF-8 input on
  stdin (SSH banners, partial escape sequences, Latin-1 hostnames) could
  silently drop or double-feed bytes at line boundaries. Now uses
  `regex::bytes::Regex` so buffer cursor stays in raw-byte space.
- **Auto-detect fallback** - When profile auto-detection fails, rt now falls
  back to the `base` profile (universal patterns only) instead of silently
  applying `juniper` to arbitrary input. The fallback emits a visible
  stderr warning so users know vendor-specific patterns aren't being applied.
- **Linux shell detection is mid-stream and sticky** - The initial 4KB/100ms
  detection window is often too small for corporate jumpboxes where the
  shell prompt arrives after legal MOTD chunks. rt now re-checks every
  incoming chunk and flips to ANSI-preserve mode the first time a shell
  signal appears — the rest of the session keeps `clear`, readline
  backspace, and `ls --color` working.
- **Explicit `-c` with missing path now errors** - Previously `-c` with a
  nonexistent path silently fell back to the embedded default, hiding typos
  and wrong-working-directory mistakes. Now hard-errors with an actionable
  message.

### Changed
- **Invalid regex is a hard error at startup**, not a silent warning. Error
  includes the profile, pattern identifier, and the regex engine's position
  marker pointing at the offending character.
- **Error messages include file paths** - Every config I/O operation
  (`fs::read_to_string`, `toml::from_str`, `fs::write`, ...) is now wrapped
  with `.with_context()`, so TOML parse errors and permission failures
  report the offending path. The top-level error printer uses `{:#}` to
  walk the full cause chain.
- **Bash/zsh prompt detection** - The Linux-shell detector now matches
  `user@host:path$` and `user@host:path#` prompts in addition to OS names in
  the banner. Corporate jumpboxes with scrubbed banners are now detected
  via their PS1.
- **Default `default_profile` is now `base`** (was `juniper`), matching the
  new fallback behavior.

### Removed
- **`convert` feature** - The deprecated ChromaTerm YAML-to-TOML migration
  module (and its unmaintained `serde_yaml` dependency) has been removed.
  Migrations are one-way; keeping the code in a permanent deprecated state
  wasn't earning its rent.
- **`CURRENT_VERSION` constant** in `versions.rs` - drift-prone. Callers
  already derive the version from `parse_config_version(DEFAULT_CONFIG)`.

### Added
- **`SECURITY.md`** at repo root - documents the trust model (config = code),
  ReDoS posture, and every filesystem side effect.
- **`CONTRIBUTING.md`** - the config version-bump + `KNOWN_HASHES` registration
  workflow is now in a human-visible file (previously in agent-only docs).
- **`// WRITE:` doc comments** on `handle_init`, `handle_completions`,
  `handle_update_config` so the write surface is obvious at the callsite.
- **Test coverage** for UTF-8 buffer handling (4 tests) and Linux shell
  detection (3 tests). Total 37 lib tests (up from 30).

### Security
- `SECURITY.md` enumerates the 7 filesystem write paths, the regex-as-code
  trust model, and the ReDoS posture (regex crate guarantees linear time;
  but large/alternation-heavy patterns in user config are still reviewable
  code).

## [0.2.23] - 2025-01-19

### Changed
- Version bump for crates.io publication (0.2.22 bug fixes were already published)

## [0.2.22] - 2025-01-18

### Fixed
- **Overlap detection bug** - Fixed edge case where color ranges that completely enclose existing ranges were not detected as overlaps
- **Circular inheritance protection** - Profile inheritance now detects and warns about circular dependencies instead of infinite looping
- **Transitive inheritance** - Profile inheritance now correctly resolves multi-level inheritance chains (A inherits B inherits C)
- **Version comparison** - Fixed comparison of versions with different lengths ("1.0" now equals "1.0.0")
- **fcntl error handling** - Non-blocking stdin setup now checks for errors before applying flags
- **Config validation** - Added validation that `default_profile` references an existing profile

### Security
- **ReDoS protection** - Added bounds to regex patterns that match variable-length content to prevent denial of service on malformed input

### Changed
- Replaced `.unwrap()` with `.expect()` for regex compilation with descriptive error messages
- Refactored `is_leap_year()` to use `is_multiple_of()` for clarity
- Integration tests now skip gracefully when test data files are missing instead of failing

## [0.2.20] - 2024-12-30

### Fixed
- Context rule `default_color` placement in configuration

## [0.2.17] - 2024-12-28

### Added
- Quiet flag (`-q`, `--quiet`) to suppress info messages

### Fixed
- Hostname prefix detection no longer matches common words (e.g., "SWITCH")

## [0.2.15] - 2024-12-27

### Added
- Auto-detect Linux/Unix servers and preserve ANSI codes for interactive shell sessions
- Powerlevel10k RPROMPT stripping for cleaner output

## [0.2.14] - 2024-12-26

### Added
- Shell completions (`rt completions <shell> --install`)
- Shell integration (`rt init --install`) for automatic SSH colorization

## [0.2.12] - 2024-12-25

### Added
- Versa SD-WAN profile with full VNF support
- Automatic profile detection from content/banners
- User-configurable hostname prefixes
- Smart config update with merge support (`rt --update-config`)

## [0.2.0] - 2024-12-20

### Added
- Dual spectrum coloring system (neutral vs. error-based)
- Context-aware coloring with state tracking
- Cisco IOS/IOS-XE/NX-OS profile

## [0.1.0] - 2024-12-15

### Added
- Initial release
- Juniper JunOS profile with comprehensive pattern support
- Base profile with universal patterns (IPs, MACs, status keywords)
- TOML configuration with profile inheritance
- ChromaTerm YAML converter (optional feature)
