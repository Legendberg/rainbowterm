use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use regex::Regex;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

#[cfg(unix)]
extern crate libc;

mod config;
mod context;
mod matching;
mod versions;
#[cfg(feature = "convert")]
mod convert;

use config::{parse_hex_color, ColoredRange, Config};
use context::ContextEngine;

/// Regex pattern for ANSI escape sequences
/// Matches: CSI sequences (\x1b[...m), OSC sequences (\x1b]...\x07), and other escapes
static ANSI_REGEX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn get_ansi_regex() -> &'static Regex {
    ANSI_REGEX.get_or_init(|| {
        // Match ANSI escape sequences:
        // - CSI: \x1b[ followed by params and a letter (most common, like colors)
        // - OSC: \x1b] followed by data and BEL (\x07) or ST (\x1b\\)
        // - Simple escapes: \x1b followed by single char
        // - 8-bit CSI: \x9b followed by params and letter
        Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07|\x1b\][^\x1b]*\x1b\\|\x1b[^\[0-9]|\x9b[0-9;]*[A-Za-z]")
            .expect("ANSI regex pattern should be valid")
    })
}

/// Regex for cursor save/restore pairs (used to remove RPROMPT content)
static CURSOR_SAVE_RESTORE_REGEX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn get_cursor_save_restore_regex() -> &'static Regex {
    CURSOR_SAVE_RESTORE_REGEX.get_or_init(|| {
        // Match cursor save followed by anything until cursor restore
        // Save: \x1b[s or \x1b7
        // Restore: \x1b[u or \x1b8
        // This captures RPROMPT content which is written between save/restore
        // Bounded to 10000 chars to prevent ReDoS on malformed input
        Regex::new(r"(?:\x1b\[s|\x1b7)[\s\S]{0,10000}?(?:\x1b\[u|\x1b8)")
            .expect("cursor save/restore regex pattern should be valid")
    })
}

/// Regex for cursor forward/backward movement (powerlevel10k RPROMPT style)
static CURSOR_MOVEMENT_REGEX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn get_cursor_movement_regex() -> &'static Regex {
    CURSOR_MOVEMENT_REGEX.get_or_init(|| {
        // Match cursor forward (large jump, >=20 cols) followed by content until cursor backward
        // \x1b[<n>C = cursor forward n columns
        // \x1b[<n>D = cursor backward n columns
        // This pattern is used by powerlevel10k for RPROMPT
        // Bounded to 10000 chars to prevent ReDoS on malformed input
        Regex::new(r"\x1b\[(?:[2-9][0-9]|[1-9][0-9]{2,})C[\s\S]{0,10000}?\x1b\[[0-9]+D")
            .expect("cursor movement regex pattern should be valid")
    })
}

/// Regex for cursor backward (used to detect end of RPROMPT in streaming)
static CURSOR_BACKWARD_REGEX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn get_cursor_backward_regex() -> &'static Regex {
    CURSOR_BACKWARD_REGEX.get_or_init(|| {
        // Match cursor backward by large amount (>50 cols) - indicates RPROMPT was printed
        // Everything from cursor forward to this point should be removed
        Regex::new(r"\x1b\[[5-9][0-9]+D|\x1b\[1[0-9]{2,}D")
            .expect("cursor backward regex pattern should be valid")
    })
}

/// Regex for cursor forward (used to detect start of RPROMPT region)
static CURSOR_FORWARD_REGEX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn get_cursor_forward_regex() -> &'static Regex {
    CURSOR_FORWARD_REGEX.get_or_init(|| {
        // Match cursor forward by large amount (>50 cols) - indicates RPROMPT positioning
        Regex::new(r"\x1b\[[5-9][0-9]+C|\x1b\[1[0-9]{2,}C")
            .expect("cursor forward regex pattern should be valid")
    })
}

/// Regex for powerlevel10k RPROMPT text pattern (fallback when escape sequences are chunked)
static P10K_RPROMPT_REGEX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

/// Regex for detecting Linux/Unix server banners (to auto-enable ANSI preservation)
static LINUX_SERVER_REGEX: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn get_p10k_rprompt_regex() -> &'static Regex {
    P10K_RPROMPT_REGEX.get_or_init(|| {
        // Match common p10k RPROMPT patterns:
        // "with <user>@<host>" and "at HH:MM:SS"
        // This is a fallback for when escape sequences are split across chunks
        Regex::new(r"with\s+\w+@[\w\-\.]+(\s+at\s+\d{1,2}:\d{2}(:\d{2})?)?")
            .expect("p10k RPROMPT regex pattern should be valid")
    })
}

/// Strip only the p10k RPROMPT text pattern (for use with --preserve-ansi)
fn strip_p10k_rprompt_text(text: &str) -> String {
    get_p10k_rprompt_regex().replace_all(text, "").to_string()
}

fn get_linux_server_regex() -> &'static Regex {
    LINUX_SERVER_REGEX.get_or_init(|| {
        // Match Linux/Unix server indicators in SSH banners
        // These indicate an interactive shell session that benefits from ANSI preservation
        Regex::new(r"(?i)Linux\s+\w+\s+\d|Debian|Ubuntu|CentOS|Red\s*Hat|RHEL|Fedora|Arch\s+Linux|GNU/Linux|FreeBSD|OpenBSD|NetBSD|Darwin|macOS")
            .expect("Linux server regex pattern should be valid")
    })
}

/// Detect if the input looks like a Linux/Unix server (interactive shell session)
/// Returns true if ANSI codes should be preserved for this session
fn is_linux_server(text: &str) -> bool {
    get_linux_server_regex().is_match(text)
}

/// Strip ANSI escape codes from input text and handle terminal positioning
///
/// This handles RPROMPT and other terminal positioning:
/// - Removes content between cursor save/restore pairs (RPROMPT method 1)
/// - Removes content between cursor forward/backward movements (RPROMPT method 2, powerlevel10k)
/// - Removes content after large cursor forward if no backward found (streaming chunks)
/// - Strips all ANSI escape sequences
/// - Handles `\r` (carriage return) by discarding text before it on the same line
/// - Preserves `\r\n` as normal line endings
fn strip_ansi_codes(text: &str) -> String {
    // First, remove content between cursor save/restore pairs
    // This handles RPROMPT which is rendered: save cursor, move right, print, restore cursor
    let without_rprompt = get_cursor_save_restore_regex().replace_all(text, "");

    // Also remove content between cursor forward/backward movements (powerlevel10k style)
    // Pattern: \x1b[96C ... \x1b[130D (jump right, print RPROMPT, jump back left)
    let without_rprompt = get_cursor_movement_regex().replace_all(&without_rprompt, "");

    // Handle streaming case: if we see cursor forward (large) without matching backward,
    // remove everything from cursor forward to end of text (RPROMPT in partial chunk)
    let without_rprompt = if get_cursor_forward_regex().is_match(&without_rprompt)
        && !get_cursor_backward_regex().is_match(&without_rprompt)
    {
        // Has cursor forward but no cursor backward - truncate at cursor forward
        get_cursor_forward_regex()
            .split(&without_rprompt)
            .next()
            .unwrap_or("")
            .to_string()
            .into()
    } else if get_cursor_backward_regex().is_match(&without_rprompt)
        && !get_cursor_forward_regex().is_match(&without_rprompt)
    {
        // Has cursor backward but no cursor forward - this chunk is RPROMPT content, skip it
        get_cursor_backward_regex()
            .splitn(&without_rprompt, 2)
            .nth(1)
            .unwrap_or("")
            .to_string()
            .into()
    } else {
        without_rprompt
    };

    // Strip powerlevel10k RPROMPT text patterns (before ANSI stripping so it works in both modes)
    // This handles cases where escape sequences were split across chunks
    let without_rprompt = get_p10k_rprompt_regex().replace_all(&without_rprompt, "");

    // Then strip remaining ANSI escape sequences
    let stripped = get_ansi_regex().replace_all(&without_rprompt, "");

    // Then handle carriage returns that aren't part of \r\n
    // When \r appears alone, it means "go back to start of line" -
    // text after \r overwrites text before it
    let mut result = String::with_capacity(stripped.len());
    let mut line_buffer = String::new();
    let mut chars = stripped.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    // \r\n - normal line ending, flush buffer and add both
                    result.push_str(&line_buffer);
                    result.push('\r');
                    result.push(chars.next().unwrap()); // consume \n
                    line_buffer.clear();
                } else {
                    // \r alone - discard everything before it (simulates overwrite)
                    line_buffer.clear();
                }
            }
            '\n' => {
                // Newline - flush buffer
                result.push_str(&line_buffer);
                result.push('\n');
                line_buffer.clear();
            }
            _ => {
                line_buffer.push(c);
            }
        }
    }

    // Don't forget remaining content
    result.push_str(&line_buffer);
    result
}

#[derive(Parser)]
#[command(name = "rainbowterm")]
#[command(about = "Context-aware terminal colorizer for network device output", long_about = None)]
#[command(version)]
struct Cli {
    /// Don't use colors
    #[arg(long)]
    no_color: bool,

    /// Configuration file path (default: platform config dir/rainbowterm/config.toml)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Profile to use (e.g., juniper, cisco, base)
    #[arg(short, long)]
    profile: Option<String>,

    /// Disable auto-detection of profile from input content
    #[arg(long)]
    no_auto_detect: bool,

    /// List available profiles and exit
    #[arg(long)]
    list_profiles: bool,

    /// Disable context-aware state machine (pure regex mode)
    #[arg(long)]
    no_context: bool,

    /// Preserve ANSI escape codes from input (default: strip them for cleaner pattern matching)
    #[arg(long)]
    preserve_ansi: bool,

    /// Update user config with embedded defaults (smart merge for custom configs)
    #[arg(long)]
    update_config: bool,

    /// Force replace config with stock version (use with --update-config)
    #[arg(long)]
    force: bool,

    /// Show config hash and version info
    #[arg(long)]
    config_hash: bool,

    /// Suppress info messages (profile detection, etc.)
    #[arg(short, long)]
    quiet: bool,

    #[command(subcommand)]
    subcommand: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,

        /// Install completions to standard location (default: print to stdout)
        #[arg(long)]
        install: bool,
    },

    /// Setup shell integration for automatic SSH colorization
    Init {
        /// Actually install the shell function (default: just show what would be added)
        #[arg(long)]
        install: bool,
    },

    /// Convert ChromaTerm YAML to RainbowTerm TOML (DEPRECATED - requires 'convert' feature)
    #[cfg(feature = "convert")]
    Convert {
        /// Input YAML file
        input: PathBuf,

        /// Output TOML file (optional, defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() {
    if let Err(e) = run() {
        // Silently ignore broken pipe (downstream closed)
        if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
            if io_err.kind() == std::io::ErrorKind::BrokenPipe {
                std::process::exit(0);
            }
        }
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Handle subcommands first
    if let Some(cmd) = &cli.subcommand {
        match cmd {
            Commands::Completions { shell, install } => {
                return handle_completions(*shell, *install);
            }
            Commands::Init { install } => {
                return handle_init(*install);
            }
            #[cfg(feature = "convert")]
            Commands::Convert { input, output } => {
                let yaml_content = std::fs::read_to_string(input)?;
                let toml_content = convert::convert_yaml_to_toml(&yaml_content)?;

                if let Some(output_path) = output {
                    std::fs::write(output_path, toml_content)?;
                    println!("Converted {} to {}", input.display(), output_path.display());
                } else {
                    println!("{}", toml_content);
                }
                return Ok(());
            }
        }
    }

    // Load configuration
    let config_path = cli.config.clone().unwrap_or_else(|| {
        let mut path = dirs::config_dir().expect("Could not find config directory");
        path.push("rainbowterm");
        std::fs::create_dir_all(&path).ok();
        path.push("config.toml");
        path
    });

    // Embedded default config
    const DEFAULT_CONFIG: &str = include_str!("../config.toml");

    // Verify CURRENT_VERSION matches embedded config (compile-time check for consistency)
    debug_assert_eq!(
        versions::parse_config_version(DEFAULT_CONFIG).as_deref(),
        Some(versions::CURRENT_VERSION),
        "CURRENT_VERSION in versions.rs doesn't match config.toml header!"
    );

    // Handle --config-hash: show hash and version info
    if cli.config_hash {
        let embedded_version = versions::parse_config_version(DEFAULT_CONFIG)
            .unwrap_or_else(|| "unknown".to_string());
        let embedded_hash = versions::hash_config(DEFAULT_CONFIG);
        println!("Embedded config version: {}", embedded_version);
        println!("Embedded config hash: {}", embedded_hash);

        if config_path.exists() {
            let user_config = std::fs::read_to_string(&config_path)?;
            let user_version = versions::parse_config_version(&user_config)
                .unwrap_or_else(|| "unknown".to_string());
            let user_hash = versions::hash_config(&user_config);
            let user_date = versions::parse_config_date(&user_config)
                .unwrap_or_else(|| "unknown".to_string());
            println!("User config version: {} ({})", user_version, user_date);
            println!("User config hash: {}", user_hash);

            if let Some(stock_ver) = versions::is_stock_config(&user_config) {
                println!("User config status: stock (unmodified v{})", stock_ver);
            } else {
                println!("User config status: modified (custom changes detected)");
            }
        } else {
            println!("User config: not yet created");
        }
        return Ok(());
    }

    // Handle --update-config: smart update with merge support
    if cli.update_config {
        return handle_update_config(&config_path, DEFAULT_CONFIG, cli.force);
    }

    // Create default config if it doesn't exist (only for default path)
    let first_run = cli.config.is_none() && !config_path.exists();
    if first_run {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&config_path, DEFAULT_CONFIG)?;
        eprintln!("Created default config at {}", config_path.display());
    }

    // Check for shell integration (once per install, on first run)
    if first_run {
        check_shell_integration_hint(&config_path);
    }

    // Check for stale config and warn (once per version)
    if config_path.exists() {
        check_config_version_warning(&config_path, DEFAULT_CONFIG);
    }

    // Load config from file or use embedded default
    let config = if config_path.exists() {
        Config::from_file(&config_path)?
    } else {
        Config::parse(DEFAULT_CONFIG)?
    };

    // Handle --list-profiles
    if cli.list_profiles {
        println!("Available profiles:");
        for (name, profile) in &config.profiles {
            println!("  {} - {}", name, profile.description);
        }
        return Ok(());
    }

    // If explicit profile specified, use it directly (no auto-detection)
    if let Some(profile_name) = cli.profile.as_ref() {
        let profile = config.get_profile(profile_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Profile '{}' not found. Use --list-profiles to see available profiles.",
                profile_name
            )
        })?;
        if !cli.quiet {
            eprintln!("Using profile: {}", profile_name);
        }
        return run_colorizer(&config, &profile, cli.no_color, cli.no_context, !cli.preserve_ansi, None);
    }

    // Auto-detect profile from input (default behavior)
    if !cli.no_auto_detect {
        return run_with_auto_detect(&config, cli.no_color, cli.no_context, !cli.preserve_ansi, cli.quiet);
    }

    // Fallback: use default profile (when --no-auto-detect is set)
    let default_name = config.default_profile.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "No profile specified and no default_profile set in config.\n\
             Use --profile <name> or set default_profile in config.toml"
        )
    })?;

    let profile = config.get_profile(default_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Profile '{}' not found. Use --list-profiles to see available profiles.",
            default_name
        )
    })?;

    if !cli.quiet {
        eprintln!("Using default profile: {}", default_name);
    }
    run_colorizer(&config, &profile, cli.no_color, cli.no_context, !cli.preserve_ansi, None)
}

/// Run with auto-detection: buffer initial input, detect profile, then process
fn run_with_auto_detect(
    config: &Config,
    no_color: bool,
    no_context: bool,
    strip_ansi: bool,
    quiet: bool,
) -> anyhow::Result<()> {
    use io::Read;

    const DETECT_BUFFER_SIZE: usize = 4096; // Buffer size for detection
    const DETECT_TIMEOUT_MS: u64 = 100; // Wait time to accumulate data

    let stdin = io::stdin();
    let mut stdin_handle = stdin.lock();
    let mut buffer = Vec::new();

    // Read initial chunk for detection
    let mut chunk = vec![0u8; DETECT_BUFFER_SIZE];

    // Give some time for data to arrive (helps with slow SSH connections)
    std::thread::sleep(std::time::Duration::from_millis(DETECT_TIMEOUT_MS));

    let bytes_read = stdin_handle.read(&mut chunk)?;
    if bytes_read > 0 {
        buffer.extend_from_slice(&chunk[..bytes_read]);
    }

    // Convert buffer to string for detection
    let initial_text = String::from_utf8_lossy(&buffer);

    // Detect profile from content
    let detected_profile = config.detect_profile(&initial_text);

    // Auto-detect Linux/Unix servers and preserve ANSI for interactive shells
    let effective_strip_ansi = if strip_ansi && is_linux_server(&initial_text) {
        if !quiet {
            eprintln!("Detected Linux/Unix server, preserving ANSI codes");
        }
        false // Don't strip ANSI for interactive shell sessions
    } else {
        strip_ansi
    };

    let (_profile_name, profile) = if let Some((name, prof)) = detected_profile {
        if !quiet {
            eprintln!("Auto-detected profile: {}", name);
        }
        (name, prof)
    } else {
        // Fall back to default profile
        let default_name = config.default_profile.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Could not auto-detect profile and no default_profile set in config.\n\
                 Use --profile <name> to specify explicitly."
            )
        })?;
        let prof = config.get_profile(default_name).ok_or_else(|| {
            anyhow::anyhow!("Default profile '{}' not found", default_name)
        })?;
        if !quiet {
            eprintln!("Auto-detect: no match, using default profile: {}", default_name);
        }
        (default_name.clone(), prof)
    };

    drop(stdin_handle); // Release lock before running colorizer

    // Run colorizer with buffered data
    run_colorizer(config, &profile, no_color, no_context, effective_strip_ansi, Some(buffer))
}

/// Helper function to process and output a single chunk
fn process_and_output_chunk(
    data: &str,
    separator: &str,
    stdout: &mut StandardStream,
    compiled_patterns: &[matching::CompiledPattern],
    context_engine: &mut Option<ContextEngine>,
    config: &Config,
    strip_ansi: bool,
) -> anyhow::Result<()> {
    // Always strip p10k RPROMPT text pattern (works with or without --preserve-ansi)
    let data_without_rprompt = strip_p10k_rprompt_text(data);

    // Strip ANSI codes if requested (for SSH sessions with terminal emulation)
    let clean_data: std::borrow::Cow<str> = if strip_ansi {
        std::borrow::Cow::Owned(strip_ansi_codes(&data_without_rprompt))
    } else {
        std::borrow::Cow::Owned(data_without_rprompt)
    };

    // Update context state first (before applying patterns)
    if let Some(ref mut engine) = context_engine {
        engine.process_line(&clean_data);
    }

    // Collect colored ranges from context rules and patterns
    let mut colored_parts: Vec<ColoredRange> = Vec::new();

    // Context-aware rules (highest priority)
    if let Some(ref engine) = context_engine {
        colored_parts.extend(engine.apply_rules(&clean_data, &|c| config.resolve_color(c)));
    }

    // Regular pattern matching (lower priority)
    colored_parts.extend(matching::apply_patterns(&clean_data, compiled_patterns));

    // Sort and remove overlaps
    colored_parts.sort_by_key(|k| k.start);
    let final_parts = remove_overlapping_ranges(colored_parts);

    // Render colored output (use clean_data which has ANSI stripped)
    render_colored_output(stdout, &clean_data, &final_parts)?;
    write!(stdout, "{}", separator)?;

    Ok(())
}

/// Remove overlapping color ranges (keeps first/higher priority)
fn remove_overlapping_ranges(ranges: Vec<ColoredRange>) -> Vec<ColoredRange> {
    let mut result = Vec::new();
    for range in ranges {
        let overlaps = result.iter().any(|r: &ColoredRange| {
            // Check all overlap cases:
            // 1. New range starts within existing range
            // 2. New range ends within existing range
            // 3. New range completely encloses existing range
            (range.start >= r.start && range.start < r.end)
                || (range.end > r.start && range.end <= r.end)
                || (range.start <= r.start && range.end >= r.end)
        });
        if !overlaps {
            result.push(range);
        }
    }
    result
}

/// Render text with color ranges to stdout
fn render_colored_output(
    stdout: &mut StandardStream,
    data: &str,
    ranges: &[ColoredRange],
) -> anyhow::Result<()> {
    let mut last_pos = 0;
    for range in ranges {
        write!(stdout, "{}", &data[last_pos..range.start])?;
        if let Some((r, g, b)) = parse_hex_color(&range.color) {
            stdout.set_color(ColorSpec::new().set_fg(Some(Color::Rgb(r, g, b))))?;
        }
        write!(stdout, "{}", &data[range.start..range.end])?;
        stdout.reset()?;
        last_pos = range.end;
    }
    write!(stdout, "{}", &data[last_pos..])?;
    Ok(())
}

/// Main colorizer with configuration support
fn run_colorizer(
    config: &Config,
    profile: &config::Profile,
    no_color: bool,
    no_context: bool,
    strip_ansi: bool,
    initial_data: Option<Vec<u8>>,
) -> anyhow::Result<()> {
    let color_choice = if no_color { ColorChoice::Never } else { ColorChoice::Always };
    let mut stdout = StandardStream::stdout(color_choice);

    // Compile patterns once at startup
    let compiled_patterns = matching::compile_patterns(profile, config);
    let mut context_engine = setup_context_engine(profile, no_context);

    // Process stdin in chunks (with optional initial data from auto-detect)
    process_stdin(&mut stdout, &compiled_patterns, &mut context_engine, config, strip_ansi, initial_data)
}

/// Setup context engine if enabled
fn setup_context_engine(profile: &config::Profile, no_context: bool) -> Option<ContextEngine> {
    if no_context {
        return None;
    }
    let mut engine = ContextEngine::new();
    for context in &profile.contexts {
        if let Err(e) = engine.add_context(context) {
            eprintln!("Warning: Failed to compile context '{}': {}", context.name, e);
        }
    }
    Some(engine)
}

/// Process stdin in chunks
fn process_stdin(
    stdout: &mut StandardStream,
    patterns: &[matching::CompiledPattern],
    context_engine: &mut Option<ContextEngine>,
    config: &Config,
    strip_ansi: bool,
    initial_data: Option<Vec<u8>>,
) -> anyhow::Result<()> {
    use io::Read;

    const READ_SIZE: usize = 8192;
    const BATCH_DELAY_MS: u64 = 10;
    const PROMPT_TIMEOUT_MS: u64 = 50; // Flush incomplete lines after this timeout

    // Only split on actual line endings (\r\n or \n), NOT bare \r
    // Bare \r (carriage return) is handled in strip_ansi_codes as terminal overwrite
    let split_regex = Regex::new(r"(\r?\n)")?;
    let stdin = io::stdin();
    let mut buffer = Vec::new();

    // Set stdin to non-blocking mode on Unix for prompt detection
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = stdin.as_raw_fd();
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags != -1 {
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
            // Silently continue if fcntl fails - prompt detection is optional
        }
    }

    let mut stdin_handle = stdin.lock();

    // Process initial data if provided (from auto-detect)
    if let Some(initial) = initial_data {
        buffer.extend(initial);
        let text = String::from_utf8_lossy(&buffer);
        let chunks = split_text_chunks(&text, &split_regex);

        // Process only complete lines (those with a separator)
        // Keep incomplete lines in buffer for next iteration
        let mut processed_bytes = 0;
        for (data, sep) in &chunks {
            if sep.is_empty() {
                // Incomplete line - keep in buffer
                break;
            }
            process_and_output_chunk(data, sep, stdout, patterns, context_engine, config, strip_ansi)?;
            processed_bytes += data.len() + sep.len();
        }

        // Keep only the unprocessed part in buffer
        if processed_bytes > 0 {
            buffer = buffer[processed_bytes..].to_vec();
        }
        io::stdout().flush()?;
    }

    loop {
        let mut chunk = vec![0u8; READ_SIZE];
        let read_result = stdin_handle.read(&mut chunk);

        let bytes_read = match read_result {
            Ok(0) => {
                // EOF - process remaining data (even incomplete lines)
                if !buffer.is_empty() {
                    let text = String::from_utf8_lossy(&buffer);
                    process_and_output_chunk(&text, "", stdout, patterns, context_engine, config, strip_ansi)?;
                }
                break;
            }
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available right now (non-blocking mode)
                // If we have buffered incomplete data, it's likely a prompt - output it
                if !buffer.is_empty() {
                    let text = String::from_utf8_lossy(&buffer);
                    process_and_output_chunk(&text, "", stdout, patterns, context_engine, config, strip_ansi)?;
                    buffer.clear();
                    io::stdout().flush()?;
                }
                // Small sleep before trying again to avoid busy-waiting
                std::thread::sleep(std::time::Duration::from_millis(PROMPT_TIMEOUT_MS));
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        buffer.extend_from_slice(&chunk[..bytes_read]);
        std::thread::sleep(std::time::Duration::from_millis(BATCH_DELAY_MS));

        // Split and process only complete lines
        let text = String::from_utf8_lossy(&buffer);
        let chunks = split_text_chunks(&text, &split_regex);

        // Process only complete lines (those with a separator)
        // Keep incomplete lines in buffer for next iteration
        let mut processed_bytes = 0;
        for (data, sep) in &chunks {
            if sep.is_empty() {
                // Incomplete line - keep in buffer for next chunk
                break;
            }
            process_and_output_chunk(data, sep, stdout, patterns, context_engine, config, strip_ansi)?;
            processed_bytes += data.len() + sep.len();
        }

        // Keep only the unprocessed part in buffer
        if processed_bytes > 0 {
            buffer = buffer[processed_bytes..].to_vec();
        }
        io::stdout().flush()?;
    }

    Ok(())
}

/// Split text into (data, separator) chunks on line boundaries
fn split_text_chunks(text: &str, regex: &Regex) -> Vec<(String, String)> {
    let mut chunks = Vec::new();
    let mut last_end = 0;
    for mat in regex.find_iter(text) {
        chunks.push((text[last_end..mat.start()].to_string(), mat.as_str().to_string()));
        last_end = mat.end();
    }
    if last_end < text.len() {
        chunks.push((text[last_end..].to_string(), String::new()));
    }
    chunks
}

// =============================================================================
// SHELL COMPLETIONS (rt completions)
// =============================================================================

/// Handle `rt completions` command - generate or install shell completions
fn handle_completions(shell: Shell, install: bool) -> anyhow::Result<()> {
    if !install {
        // Just print to stdout
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, "rt", &mut io::stdout());
        return Ok(());
    }

    // Get install path for this shell
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

    let (install_path, setup_instructions) = match shell {
        Shell::Bash => {
            // Standard bash-completion location
            let path = home.join(".local/share/bash-completion/completions/rt");
            let instructions = if cfg!(windows) || std::env::var("MSYSTEM").is_ok() {
                // GitBash on Windows doesn't auto-load completions
                "GitBash detected. After install, add to ~/.bashrc:\n\
                 \n  source ~/.local/share/bash-completion/completions/rt\n\n\
                 (Create ~/.bashrc first if it doesn't exist: touch ~/.bashrc)"
            } else {
                "Completions will be loaded automatically on new shells.\n\
                 If not working, ensure bash-completion is installed."
            };
            (path, instructions)
        }
        Shell::Zsh => {
            // ~/.zfunc is a common convention, but user needs to add to fpath
            let path = home.join(".zfunc/_rt");
            let instructions = "Add to your ~/.zshrc (if not already present):\n\
                               \n  fpath+=~/.zfunc\n  autoload -Uz compinit && compinit\n";
            (path, instructions)
        }
        Shell::Fish => {
            // Fish auto-loads from this directory
            let path = home.join(".config/fish/completions/rt.fish");
            let instructions = "Completions will be loaded automatically on new shells.";
            (path, instructions)
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Auto-install not supported for {:?}. Use 'rt completions {:?}' to print to stdout.",
                shell, shell
            ));
        }
    };

    // Check if already exists
    if install_path.exists() {
        eprintln!("Completions already exist at {}", install_path.display());
        eprintln!("To reinstall, delete the file first and run again.");
        return Ok(());
    }

    // Show what we'll do
    eprintln!("Shell Completions Setup");
    eprintln!("=======================");
    eprintln!("  Shell: {:?}", shell);
    eprintln!("  Path:  {}", install_path.display());
    eprintln!("\n{}", setup_instructions);

    // Interactive confirmation
    if !is_terminal() {
        eprintln!("\nNon-interactive mode. Run interactively or redirect output:");
        eprintln!("  rt completions {:?} > {}", shell, install_path.display());
        return Ok(());
    }

    eprint!("\nInstall completions? [y/N]: ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;

    if input.trim().to_lowercase() != "y" {
        eprintln!("Cancelled.");
        return Ok(());
    }

    // Create parent directory if needed
    if let Some(parent) = install_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Generate completions to file
    let mut file = std::fs::File::create(&install_path)?;
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "rt", &mut file);

    eprintln!("\nInstalled successfully!");
    eprintln!("Restart your shell or source the completions to activate.");

    Ok(())
}

// =============================================================================
// SHELL INTEGRATION (rt init)
// =============================================================================

/// Handle `rt init` command - setup shell integration for automatic SSH colorization
fn handle_init(install: bool) -> anyhow::Result<()> {
    // Detect shell from $SHELL environment variable
    let shell_path = std::env::var("SHELL").unwrap_or_default();
    let shell_name = Path::new(&shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    // Determine rc file path
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let rc_file = match shell_name {
        "zsh" => home.join(".zshrc"),
        "bash" => {
            // Prefer .bashrc, fall back to .bash_profile on macOS
            let bashrc = home.join(".bashrc");
            let bash_profile = home.join(".bash_profile");
            if bashrc.exists() {
                bashrc
            } else if bash_profile.exists() {
                bash_profile
            } else {
                bashrc // Default to .bashrc
            }
        }
        _ => {
            eprintln!("Unsupported shell: {}", shell_name);
            eprintln!("Supported shells: zsh, bash");
            eprintln!("\nManual setup: Add this to your shell's rc file:");
            eprintln!("  ssh() {{ /usr/bin/ssh \"$@\" | rt; }}");
            return Ok(());
        }
    };

    // Find ssh binary path
    let ssh_path = find_ssh_path();

    // Build the shell function
    let shell_function = format!(
        r#"
# RainbowTerm: Automatic SSH colorization
ssh() {{ {} "$@" | rt; }}"#,
        ssh_path
    );

    // Check if already installed
    let rc_content = std::fs::read_to_string(&rc_file).unwrap_or_default();
    let already_installed = rc_content.contains("| rt;")
        || rc_content.contains("| rt --")
        || rc_content.contains("|rt;")
        || rc_content.contains("| rt }");

    if already_installed {
        eprintln!("Shell integration already detected in {}", rc_file.display());
        eprintln!("\nIf you want to reinstall, remove the existing ssh() function first.");
        return Ok(());
    }

    // Show what we found and what we'll do
    eprintln!("Shell Integration Setup");
    eprintln!("=======================");
    eprintln!("  Shell:    {} ({})", shell_name, shell_path);
    eprintln!("  RC file:  {}", rc_file.display());
    eprintln!("  SSH path: {}", ssh_path);
    eprintln!("\nThis will add the following to {}:", rc_file.display());
    eprintln!("{}", shell_function);

    if !install {
        eprintln!("\nTo install, run:");
        eprintln!("  rt init --install");
        eprintln!("\nOr manually add the function above to your shell config.");
        return Ok(());
    }

    // Interactive confirmation
    if !is_terminal() {
        eprintln!("\nNon-interactive mode. Run interactively or add manually.");
        return Ok(());
    }

    eprint!("\nInstall to {}? [y/N]: ", rc_file.display());
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;

    if input.trim().to_lowercase() != "y" {
        eprintln!("Cancelled.");
        return Ok(());
    }

    // Append to rc file
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&rc_file)?;

    writeln!(file, "{}", shell_function)?;

    eprintln!("\nInstalled successfully!");
    eprintln!("\nTo activate, either:");
    eprintln!("  1. Restart your terminal, or");
    eprintln!("  2. Run: source {}", rc_file.display());
    eprintln!("\nThen just type 'ssh <host>' - colorization is automatic!");

    Ok(())
}

/// Find the ssh binary path, avoiding any shell function
fn find_ssh_path() -> String {
    // Try common locations in order of preference
    let candidates = [
        "/usr/bin/ssh",
        "/usr/local/bin/ssh",
        "/opt/homebrew/bin/ssh",
        "/bin/ssh",
    ];

    for candidate in candidates {
        if Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }

    // Fall back to which command
    if let Ok(output) = std::process::Command::new("which").arg("ssh").output() {
        if output.status.success() {
            if let Ok(path) = String::from_utf8(output.stdout) {
                let path = path.trim();
                if !path.is_empty() {
                    return path.to_string();
                }
            }
        }
    }

    // Ultimate fallback
    "/usr/bin/ssh".to_string()
}

/// Check if shell integration is installed, show hint if not (once per install)
fn check_shell_integration_hint(config_path: &Path) {
    // Get rc file path
    let shell_path = std::env::var("SHELL").unwrap_or_default();
    let shell_name = Path::new(&shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return,
    };

    let rc_file = match shell_name {
        "zsh" => home.join(".zshrc"),
        "bash" => {
            let bashrc = home.join(".bashrc");
            let bash_profile = home.join(".bash_profile");
            if bashrc.exists() { bashrc } else { bash_profile }
        }
        _ => return, // Unsupported shell, skip hint
    };

    // Check if already installed
    let rc_content = std::fs::read_to_string(&rc_file).unwrap_or_default();
    if rc_content.contains("| rt;") || rc_content.contains("|rt;") || rc_content.contains("| rt }") {
        return; // Already installed
    }

    // Check if we've already shown this hint
    let hint_shown_path = config_path.with_file_name(".init_hint_shown");
    if hint_shown_path.exists() {
        return;
    }

    // Show hint
    eprintln!("\nTip: Run 'rt init' to setup automatic SSH colorization.");
    eprintln!("     Then just type 'ssh <host>' - no '| rt' needed!");

    // Mark hint as shown
    std::fs::write(&hint_shown_path, "1").ok();
}

// =============================================================================
// CONFIG UPDATE AND VERSION MANAGEMENT
// =============================================================================

/// Handle --update-config with smart merge support
fn handle_update_config(config_path: &Path, embedded_config: &str, force: bool) -> anyhow::Result<()> {
    let embedded_version = versions::parse_config_version(embedded_config)
        .unwrap_or_else(|| "unknown".to_string());

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Check if user config exists
    if !config_path.exists() {
        std::fs::write(config_path, embedded_config)?;
        eprintln!("Created config v{} at {}", embedded_version, config_path.display());
        return Ok(());
    }

    // Read user's current config
    let user_config = std::fs::read_to_string(config_path)?;
    let user_version = versions::parse_config_version(&user_config)
        .unwrap_or_else(|| "unknown".to_string());

    // Handle --force: backup and replace without prompting
    if force {
        let backup_path = config_path.with_extension("toml.user");
        std::fs::copy(config_path, &backup_path)?;
        std::fs::write(config_path, embedded_config)?;
        eprintln!("Forced update to v{}. Backup saved: {}", embedded_version, backup_path.display());
        clear_version_warning(config_path);
        return Ok(());
    }

    // Check if user config matches a known stock version
    if let Some(matched_version) = versions::is_stock_config(&user_config) {
        // User has unmodified stock config
        if matched_version == embedded_version {
            eprintln!("Config is already at v{} (no update needed)", embedded_version);
            return Ok(());
        }

        // Safe to auto-update (stock -> stock)
        std::fs::write(config_path, embedded_config)?;
        eprintln!("Updated config from v{} to v{}", matched_version, embedded_version);
        clear_version_warning(config_path);
        return Ok(());
    }

    // User has modified config - need interactive handling
    eprintln!("Your config has custom modifications.");
    eprintln!("  Your version: {}", user_version);
    eprintln!("  New version:  {}", embedded_version);

    // Create backup
    let backup_path = config_path.with_extension("toml.user");
    std::fs::copy(config_path, &backup_path)?;
    eprintln!("  Backup saved: {}", backup_path.display());

    // Check if we're in interactive mode
    if !is_terminal() {
        eprintln!("\nNon-interactive mode: keeping your custom config.");
        eprintln!("To update, run interactively or use --force to replace.");
        return Ok(());
    }

    // Show diff automatically so user can make informed decision
    show_config_diff(&user_config, embedded_config);

    // Interactive prompt
    eprintln!("\nOptions:");
    eprintln!("  [M]erge   - Keep your custom changes, add new stock patterns");
    eprintln!("  [R]eplace - Use new stock config (your backup saved at .toml.user)");
    eprintln!("  [K]eep    - Keep your current config, cancel update");
    eprint!("\nChoice [M/R/K]: ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;

    match input.trim().to_lowercase().as_str() {
        "m" | "merge" => {
            eprint!("Confirm merge? [y/N]: ");
            io::stderr().flush()?;
            input.clear();
            io::stdin().lock().read_line(&mut input)?;
            if input.trim().to_lowercase() == "y" {
                let merged = merge_configs(&user_config, embedded_config)?;
                std::fs::write(config_path, &merged)?;
                eprintln!("Merged config saved. Your custom changes preserved, new patterns added.");
                clear_version_warning(config_path);
            } else {
                eprintln!("Cancelled. Keeping your current config.");
                std::fs::remove_file(&backup_path).ok();
            }
        }
        "r" | "replace" => {
            eprint!("Confirm replace? This will overwrite your changes. [y/N]: ");
            io::stderr().flush()?;
            input.clear();
            io::stdin().lock().read_line(&mut input)?;
            if input.trim().to_lowercase() == "y" {
                std::fs::write(config_path, embedded_config)?;
                eprintln!("Replaced with v{}. Your backup: {}", embedded_version, backup_path.display());
                clear_version_warning(config_path);
            } else {
                eprintln!("Cancelled. Keeping your current config.");
                std::fs::remove_file(&backup_path).ok();
            }
        }
        _ => {
            eprintln!("Keeping your current config.");
            // Clean up backup since we didn't change anything
            std::fs::remove_file(&backup_path).ok();
        }
    }

    Ok(())
}

/// Check if stdin is a terminal (for interactive prompts)
/// Can be overridden with RAINBOWTERM_FORCE_INTERACTIVE=1 for testing
fn is_terminal() -> bool {
    if std::env::var("RAINBOWTERM_FORCE_INTERACTIVE").is_ok() {
        return true;
    }
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// Show diff between user config and stock config
fn show_config_diff(user_config: &str, stock_config: &str) {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(user_config, stock_config);

    eprintln!("\n--- Your Config (user)");
    eprintln!("+++ New Stock Config");
    eprintln!();

    let mut shown_lines = 0;
    const MAX_DIFF_LINES: usize = 100;

    for change in diff.iter_all_changes() {
        if change.tag() != ChangeTag::Equal {
            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            eprint!("{}{}", sign, change);
            shown_lines += 1;
            if shown_lines >= MAX_DIFF_LINES {
                eprintln!("\n... (diff truncated, {} more lines)", diff.iter_all_changes().count() - shown_lines);
                break;
            }
        }
    }

    if shown_lines == 0 {
        eprintln!("(no differences found - configs are identical)");
    }
}

/// Merge user config with new stock config using TOML-aware merging
fn merge_configs(user_config: &str, new_stock: &str) -> anyhow::Result<String> {
    use toml_edit::DocumentMut;

    let mut user_doc: DocumentMut = user_config.parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse user config: {}", e))?;
    let new_doc: DocumentMut = new_stock.parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse stock config: {}", e))?;

    // Update the version header in the merged result
    // We'll prepend the new header to the user's config
    let new_version = versions::parse_config_version(new_stock)
        .unwrap_or_else(|| "unknown".to_string());

    // Merge strategy:
    // 1. For top-level keys in new_stock that don't exist in user: ADD them
    // 2. For [profiles.X] in new_stock that don't exist in user: ADD them
    // 3. For patterns in profiles: merge arrays (add new patterns, keep user's)
    // 4. Keep user's existing values for keys they've modified

    // Merge top-level tables
    for (key, new_value) in new_doc.iter() {
        if !user_doc.contains_key(key) {
            // New top-level key - add it
            user_doc[key] = new_value.clone();
            eprintln!("  + Added new section: [{}]", key);
        } else if key == "profiles" {
            // Special handling for profiles - merge nested tables
            if let (Some(user_profiles), Some(new_profiles)) = (
                user_doc[key].as_table_mut(),
                new_value.as_table(),
            ) {
                merge_profiles_table(user_profiles, new_profiles);
            }
        } else if key == "hostname_prefixes" {
            // Merge hostname prefixes
            if let (Some(user_prefixes), Some(new_prefixes)) = (
                user_doc[key].as_table_mut(),
                new_value.as_table(),
            ) {
                for (prefix_key, prefix_value) in new_prefixes.iter() {
                    if !user_prefixes.contains_key(prefix_key) {
                        user_prefixes[prefix_key] = prefix_value.clone();
                        eprintln!("  + Added hostname prefix: {}", prefix_key);
                    }
                }
            }
        }
        // For other existing keys, keep user's value
    }

    // Update the version comment in the output
    let mut result = user_doc.to_string();

    // Replace the old version line with new version (match entire line to avoid accumulating dates)
    let today = chrono_lite_date();
    let new_line = format!("# Config version: {} ({})", new_version, today);

    // Use regex to match the entire version line including any existing date(s)
    let version_line_regex = regex::Regex::new(r"# Config version: [^\n]+").unwrap();
    result = version_line_regex.replace(&result, new_line.as_str()).to_string();

    Ok(result)
}

/// Merge profiles tables
fn merge_profiles_table(user_profiles: &mut toml_edit::Table, new_profiles: &toml_edit::Table) {
    for (profile_name, new_profile) in new_profiles.iter() {
        if !user_profiles.contains_key(profile_name) {
            // New profile - add it entirely
            user_profiles[profile_name] = new_profile.clone();
            eprintln!("  + Added new profile: [profiles.{}]", profile_name);
        } else if let (Some(user_profile), Some(new_profile_table)) = (
            user_profiles[profile_name].as_table_mut(),
            new_profile.as_table(),
        ) {
            // Existing profile - merge patterns array
            merge_profile_patterns(profile_name, user_profile, new_profile_table);
        }
    }
}

/// Merge patterns within a profile
fn merge_profile_patterns(
    profile_name: &str,
    user_profile: &mut toml_edit::Table,
    new_profile: &toml_edit::Table,
) {
    // Get or create patterns array
    if let Some(new_patterns) = new_profile.get("patterns").and_then(|p| p.as_array_of_tables()) {
        if let Some(user_patterns) = user_profile.get_mut("patterns").and_then(|p| p.as_array_of_tables_mut()) {
            // Collect existing pattern descriptions for dedup
            let existing_descriptions: std::collections::HashSet<String> = user_patterns
                .iter()
                .filter_map(|p| p.get("description").and_then(|d| d.as_str()).map(String::from))
                .collect();

            // Add new patterns that don't exist
            let mut added = 0;
            for new_pattern in new_patterns.iter() {
                if let Some(desc) = new_pattern.get("description").and_then(|d| d.as_str()) {
                    if !existing_descriptions.contains(desc) {
                        user_patterns.push(new_pattern.clone());
                        added += 1;
                    }
                }
            }
            if added > 0 {
                eprintln!("  + Added {} new patterns to [profiles.{}]", added, profile_name);
            }
        } else {
            // User doesn't have patterns array - add the whole thing
            user_profile["patterns"] = new_profile["patterns"].clone();
            eprintln!("  + Added patterns array to [profiles.{}]", profile_name);
        }
    }

    // Merge contexts similarly
    if let Some(new_contexts) = new_profile.get("contexts").and_then(|c| c.as_array_of_tables()) {
        if let Some(user_contexts) = user_profile.get_mut("contexts").and_then(|c| c.as_array_of_tables_mut()) {
            let existing_names: std::collections::HashSet<String> = user_contexts
                .iter()
                .filter_map(|c| c.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect();

            let mut added = 0;
            for new_context in new_contexts.iter() {
                if let Some(name) = new_context.get("name").and_then(|n| n.as_str()) {
                    if !existing_names.contains(name) {
                        user_contexts.push(new_context.clone());
                        added += 1;
                    }
                }
            }
            if added > 0 {
                eprintln!("  + Added {} new contexts to [profiles.{}]", added, profile_name);
            }
        } else if new_profile.contains_key("contexts") {
            user_profile["contexts"] = new_profile["contexts"].clone();
            eprintln!("  + Added contexts to [profiles.{}]", profile_name);
        }
    }
}

/// Simple date string without external chrono dependency
fn chrono_lite_date() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    // Approximate: days since epoch
    let days = secs / 86400;
    // Calculate year/month/day (simplified, not accounting for leap seconds)
    let mut year = 1970;
    let mut remaining_days = days;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let days_in_months: [u64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for days_in_month in days_in_months.iter() {
        if remaining_days < *days_in_month {
            break;
        }
        remaining_days -= days_in_month;
        month += 1;
    }

    let day = remaining_days + 1;
    format!("{:04}-{:02}-{:02}", year, month, day)
}

fn is_leap_year(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Check config version and warn if stale (once per version)
fn check_config_version_warning(config_path: &Path, embedded_config: &str) {
    let embedded_version = match versions::parse_config_version(embedded_config) {
        Some(v) => v,
        None => return,
    };

    let user_config = match std::fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let user_version = match versions::parse_config_version(&user_config) {
        Some(v) => v,
        None => return,
    };

    // Only warn if user version is older than embedded
    use std::cmp::Ordering;
    match versions::compare_versions(&user_version, &embedded_version) {
        Ordering::Equal | Ordering::Greater => return, // Same or newer, no warning
        Ordering::Less => {} // Older, continue to warn
    }

    // Check if we've already warned about this version
    let warned_path = config_path.with_file_name(".warned_versions");
    if let Ok(warned) = std::fs::read_to_string(&warned_path) {
        if warned.lines().any(|line| line == embedded_version) {
            return; // Already warned
        }
    }

    // Show warning
    eprintln!(
        "Note: Config is v{}, binary is v{}. Run 'rt --update-config' for new patterns.",
        user_version, embedded_version
    );

    // Record that we've warned about this version
    let mut warned_versions = std::fs::read_to_string(&warned_path).unwrap_or_default();
    if !warned_versions.is_empty() && !warned_versions.ends_with('\n') {
        warned_versions.push('\n');
    }
    warned_versions.push_str(&embedded_version);
    warned_versions.push('\n');
    std::fs::write(&warned_path, warned_versions).ok();
}

/// Clear version warnings after successful update
fn clear_version_warning(config_path: &Path) {
    let warned_path = config_path.with_file_name(".warned_versions");
    std::fs::remove_file(warned_path).ok();
}
