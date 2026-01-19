# Changelog

All notable changes to RainbowTerm will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
