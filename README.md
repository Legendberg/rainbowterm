# RainbowTerm

Context-aware terminal colorizer with magnitude spectrum visualization for network device output.

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)
[![Crates.io](https://img.shields.io/crates/v/rainbowterm.svg)](https://crates.io/crates/rainbowterm)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

> **📸 View Screenshots:** Visit the [GitHub repository](https://github.com/Legendberg/rainbowterm#screenshots) to see the dual spectrum coloring system in action!

## Screenshots

### Dual Spectrum Coloring in Action
![Interface Statistics](images/interface.png)
*Juniper interface output showing dual spectrum: neutral colors for traffic stats, warm colors for errors*

### Configuration Diff Highlighting
![Configuration Compare](images/show-compare.png)
*JunOS configuration diff with syntax highlighting*

## Overview

RainbowTerm is a high-performance Rust-based terminal colorizer designed for network engineers. It provides intelligent syntax highlighting for network device output with advanced features like dual magnitude spectrum visualization (neutral vs. error-based) and context-aware coloring.

## ✨ Key Features

### 🌈 Dual Spectrum Coloring System

RainbowTerm uses **context-aware coloring** with two distinct spectrum systems:

#### Neutral Spectrum (Cool Colors) - Informational Magnitude
For traffic counters, packet counts, and other metrics where bigger isn't bad:

```
Input bytes: 1298458             # Green → Blue → Purple (millions)
Output packets: 1234567890       # Orange → Yellow → Green → Blue → Purple (billions)
Input bytes: 2342779625172       # Orange → Yellow → Green → Blue → Purple (trillions)
Input bytes: 0                   # Gray (idle)
```

- **Rightmost 3 digits**: Purple (base color)
- **Next group (thousands)**: Blue → Purple
- **Millions**: Green → Blue → Purple
- **Billions**: Yellow → Green → Blue → Purple
- **Trillions+**: Orange → Yellow → Green → Blue → Purple
- **Zero**: Gray (idle/no traffic)

#### Error Spectrum (Warm Colors) - Severity-Based Problems
For errors, drops, and problems where bigger IS bad:

```
Errors: 724                      # Yellow (minor - hundreds)
Drops: 1750520                   # Orange → Yellow (moderate - millions)
Errors: 1234567890               # Magenta → Dark Red → Crimson → Yellow (billions)
Errors: 0                        # Green (healthy - no errors!)
```

- **Rightmost 3 digits**: Yellow (base color)
- **Thousands**: Orange → Yellow
- **Ten thousands**: Red → Orange → Yellow
- **Hundred thousands**: Dark Red → Red → Orange → Yellow
- **Millions**: Crimson → Dark Red → Red → Orange → Yellow
- **Billions+**: Magenta/Violet → Crimson → Dark Red → Red → Orange → Yellow
- **Zero errors**: Green (healthy state!)

**Same number, different meaning, different color** - The philosophy behind context-aware coloring.

### 🔧 Network Protocol Support

#### Juniper JunOS
- ✅ Interface names by speed (ge, xe, et, mge, vcp, ae)
- ✅ BGP states (Established/Idle)
- ✅ OSPF states (Full/Down)
- ✅ STP states (FWD/BLK) and roles (DESG/DIS)
- ✅ STP port costs with quality indicators
- ✅ Physical link status (Up/Down)
- ✅ Duplex modes (Full-duplex/Half-duplex)
- ✅ Log severity levels (Critical/Warning/Info)
- ✅ Active alarms and defects
- ✅ Routing table markers (* and >)
- ✅ Configuration diff output (+/-/!)

#### Generic Patterns
- ✅ IPv4 addresses
- ✅ MAC addresses (colon and dot formats)
- ✅ Serial numbers and model numbers
- ✅ Status keywords (up/down, error/warning)
- ✅ Packet/byte counters with magnitude spectrum

### 🎯 Context-Aware Coloring

Multi-line state machine tracks context across output:
- Interface state (up/down) affects duplex coloring
- Half-duplex on UP link = red warning
- Half-duplex on DOWN link = gray (irrelevant)

### ⚙️ Advanced Features

- **Group-based coloring**: Different colors for each regex capture group
- **Priority system**: Fine-grained control over pattern precedence
- **Profile inheritance**: Extend base patterns with vendor-specific rules
- **TOML configuration**: Human-readable, version-controllable config
- **ChromaTerm converter**: Migrate existing YAML configs to TOML

## 🚀 Installation

### From crates.io (Recommended)

```bash
# Install from crates.io - creates 'rt' command
cargo install rainbowterm

# Config automatically created at ~/.config/rainbowterm/config.toml on first run
# No manual setup needed!
```

### From Source

```bash
# Clone the repository
git clone https://github.com/Legendberg/rainbowterm.git
cd rainbowterm

# Build and install
cargo install --path .
```

### Requirements

- Rust 1.70+ (for building)
- macOS, Linux, or WSL2

## 📖 Usage

### Basic Usage

```bash
# Pipe any command through RainbowTerm
ssh router "show interfaces" | rt

# Use with specific profile
cat output.txt | rt --profile juniper

# Disable context awareness
tail -f /var/log/messages | rt --no-context

# List available profiles
rt --list-profiles
```

### Testing Profiles

Comprehensive test files are included for each vendor:

```bash
# Test Juniper profile
cat tests/networking/juniper/common/sample.txt | rt --profile juniper

# Test Cisco profile
cat tests/networking/cisco/ios/sample.txt | rt --profile cisco

# Run integration tests
cargo test --test integration_tests

# Run all tests (unit + integration)
cargo test
```

Test files include realistic output from various commands: interfaces, BGP, OSPF, STP, logging, and more.

### Real-World Example: Context-Aware Dual Spectrum

Same output, different colors based on context:

```
Physical interface: ge-0/0/0, Enabled, Physical link is Up
  
  Traffic statistics (NEUTRAL SPECTRUM - cool colors):
   Input  bytes  :              1298458                    0 bps
   Output bytes  :            909181029                32112 bps
  
  Input errors (ERROR SPECTRUM - warm colors, same magnitudes!):
    Errors: 1298458, Drops: 909181029, Framing errors: 0
    
  Queue counters:       Queued packets  Transmitted packets      Dropped packets
    0                          1644910              1644910                  724
    1                         14362000             14362000              1750520
    ^^^                       ^^^^^^^^             ^^^^^^^^             ^^^^^^^^^
                            neutral/cool         neutral/cool          error/warm
```

Notice how `1298458` appears twice with different colors - once as traffic (neutral spectrum) and once as errors (error spectrum). Context determines meaning!

### Configuration

Default config auto-created at: `~/.config/rainbowterm/config.toml`

```toml
# Set default profile
default_profile = "juniper"

# Add custom colors to palette
[palette]
my-blue = "#0080ff"

# Create custom patterns
[[profiles.juniper.patterns]]
description = "Custom pattern"
regex = '''my-regex-here'''
color = "my-blue"
priority = 100
```

### Profile System

- **base**: Universal patterns (IPs, MACs, dates, status)
- **juniper**: JunOS-specific patterns (inherits from base)
- Easily extensible for Cisco, Arista, etc.

## 🎨 Color Schemes

### Status Colors
- 🟢 Green: Good/Active (FWD, Established, Up)
- 🟡 Orange: Warning (BLK, threshold)
- 🔴 Red: Critical (Down, Error, Idle)
- ⚪ Gray: Inactive (DIS, zero counters)
- 🔵 Cyan: Info (DESG, configuration)
- 🟣 Purple: Virtual interfaces (ae, irb, lo)

### Interface Speed Colors
- 💚 Bright green: 100G+ (et, fte)
- 🌿 Green-lime: 10G (xe)
- 💙 Cyan: 2.5G (mge)
- 🧡 Orange: 1G (ge)
- 🟠 Amber: <1G (fe)

## 🔬 Technical Details

### Architecture

```
src/
├── main.rs      # CLI interface, stdin processing, output rendering
├── config.rs    # TOML parsing, profile management, shared types
├── matching.rs  # Pattern compilation and application
├── context.rs   # State machine for context-aware rules
└── convert.rs   # ChromaTerm YAML converter (optional feature)
```

### Performance

- Patterns compiled at startup with `regex` crate
- O(n) processing with efficient overlap detection
- Chunk-based reading (8192 bytes) for SSH compatibility
- ANSI escape sequence preservation
- No performance degradation on large outputs

### Pattern Priority System

Higher priority = applied first:
- **200+**: Critical keywords (error, warning, failure)
- **168**: Error counter zeros (green = healthy)
- **166-160**: Error spectrum (trillions down to hundreds)
- **155-150**: Dropped packets column (error spectrum)
- **155-145**: Neutral spectrum (trillions down to hundreds) and queue counters
- **100**: Interface names and service patterns
- **90**: Protocol states (BGP, OSPF, STP)
- **85**: Routing markers and roles
- **80**: Speed indicators
- **10**: Generic up/down keywords

## 🤝 Contributing

Contributions are welcome! Please feel free to submit issues or pull requests on [GitHub](https://github.com/Legendberg/rainbowterm).

## 🗺️ Roadmap

- [x] Add Cisco IOS/IOS-XE/NX-OS profile
- [x] Comprehensive test files for Juniper, Cisco, Arista
- [x] Dual spectrum system (neutral vs. error-based coloring)
- [x] Context-aware coloring philosophy
- [x] Auto-create config on first run
- [x] Public release to crates.io (v0.1.0)
- [x] v0.2.0 release with dual spectrum and screenshots
- [x] v0.2.3 improved documentation for both platforms
- [x] Unit test suite (15 tests for config and matching)
- [x] Integration test suite (Juniper, Cisco profiles)
- [ ] Complete Arista EOS profile implementation
- [ ] Shell completions (bash, zsh, fish)
- [ ] Performance benchmarks
- [ ] Additional vendor profiles (Palo Alto, F5, etc.)

## 📝 License

Dual licensed under MIT OR Apache-2.0

## 🙏 Acknowledgments

- Inspired by [ChromaTerm](https://github.com/hSaria/ChromaTerm)
- Built with [Rust](https://www.rust-lang.org/)
- Developed with assistance from [Claude Code](https://claude.ai/claude-code)

---

**Note:** RainbowTerm is designed for network engineers working with CLI output from routers, switches, and firewalls. It significantly improves readability and reduces eye strain during troubleshooting sessions.
