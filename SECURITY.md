# Security

## Trust model

RainbowTerm is a terminal colorizer. It reads text on stdin, applies regex
patterns from a config file, and writes colorized text to stdout. The security
model is worth understanding before running it against untrusted input or in
shared environments.

### Config is trusted code

Patterns in `config.toml` are compiled into live regular expressions and
applied to every line of input. **Treat the config file the same way you'd
treat a shell alias or a loaded shell function** — if an attacker can modify
it, they can influence how output is rendered and (via regex engine behavior)
potentially affect performance.

- The default config is embedded in the binary at compile time and is safe.
- The user config at the platform-specific path (see
  [CONTRIBUTING.md](CONTRIBUTING.md#modifying-configtoml)) is writable by the
  user — protect it with normal filesystem permissions.
- `rt --update-config` offers a diff + merge prompt before overwriting a
  customized config. `--force` skips the prompt.

### ReDoS posture

User-supplied regex is compiled with the [`regex`](https://crates.io/crates/regex)
crate, which guarantees linear-time matching for all supported syntax (no
backtracking, no catastrophic-backtracking vulnerabilities). A malicious
config cannot hang rendering via pathological regex.

However: the regex crate does not impose a compile-time or runtime timeout.
A sufficiently large or alternation-heavy pattern can still be expensive to
compile or match. If you accept a config from an untrusted source, review it
before running.

### Invalid regex fails fast

As of 0.2.24, a config with any un-compilable regex is a hard error at
startup, not a silent warning. The error includes the offending pattern and
the regex engine's position marker so you can fix it.

## File system side effects

RainbowTerm is primarily a stdin→stdout tool, but several commands write to
your filesystem. All write paths:

| Write                                       | Triggered by                       | Prompt?                 |
|---------------------------------------------|------------------------------------|-------------------------|
| `$CONFIG/rainbowterm/config.toml`           | First run (no existing config)     | No — eprints notice     |
| `$CONFIG/rainbowterm/config.toml`           | `rt --update-config`               | Yes (unless `--force`)  |
| `$CONFIG/rainbowterm/config.toml.user`      | `rt --update-config` (backup)      | No (auto-created)       |
| `$CONFIG/rainbowterm/.hint_shown`           | First run (shell-integration hint) | No — silent             |
| `$CONFIG/rainbowterm/.warned_versions`      | Version-mismatch warning           | No — silent             |
| `~/.zshrc`, `~/.bashrc` (append)            | `rt init --install`                | Yes                     |
| `~/.zfunc/_rt`, `~/.local/share/bash-completion/completions/rt` | `rt completions <shell> --install` | Yes                     |

`$CONFIG` resolves via the [`dirs`](https://crates.io/crates/dirs) crate:

- macOS: `~/Library/Application Support`
- Linux: `~/.config`
- Windows: `%APPDATA%`

### What RainbowTerm never does

- Execute shell commands from piped input or config
- Make network requests
- Read files outside `$CONFIG/rainbowterm/` (except when you pass `-c <path>`)
- Write outside the paths listed above

## Reporting a vulnerability

For security-relevant bugs, please open a [GitHub issue](https://github.com/Legendberg/rainbowterm/issues)
with the label `security`, or contact the maintainer privately.

Please do not file security-sensitive details in public issues if the bug is
actively exploitable against deployed users.
