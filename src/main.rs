use std::io::{self, Write};
use std::path::PathBuf;
use regex::Regex;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

mod config;
mod context;
mod matching;
#[cfg(feature = "convert")]
mod convert;

use config::{parse_hex_color, ColoredRange, Config};
use context::ContextEngine;

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

    /// Update user config with embedded defaults (overwrites existing config)
    #[arg(long)]
    update_config: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Handle subcommands first
    if let Some(command) = &cli.command {
        match command {
            Commands::Completions { shell } => {
                let mut cmd = Cli::command();
                generate(*shell, &mut cmd, "rt", &mut io::stdout());
                return Ok(());
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

    // Handle --update-config: overwrite user config with embedded defaults
    if cli.update_config {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&config_path, DEFAULT_CONFIG)?;
        eprintln!("Updated config at {}", config_path.display());
        return Ok(());
    }

    // Create default config if it doesn't exist (only for default path)
    if cli.config.is_none() && !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&config_path, DEFAULT_CONFIG)?;
        eprintln!("Created default config at {}", config_path.display());
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
        eprintln!("Using profile: {}", profile_name);
        return run_colorizer(&config, &profile, cli.no_color, cli.no_context, None);
    }

    // Auto-detect profile from input (default behavior)
    if !cli.no_auto_detect {
        return run_with_auto_detect(&config, cli.no_color, cli.no_context);
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

    eprintln!("Using default profile: {}", default_name);
    run_colorizer(&config, &profile, cli.no_color, cli.no_context, None)
}

/// Run with auto-detection: buffer initial input, detect profile, then process
fn run_with_auto_detect(
    config: &Config,
    no_color: bool,
    no_context: bool,
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

    let (_profile_name, profile) = if let Some((name, prof)) = detected_profile {
        eprintln!("Auto-detected profile: {}", name);
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
        eprintln!("Auto-detect: no match, using default profile: {}", default_name);
        (default_name.clone(), prof)
    };

    drop(stdin_handle); // Release lock before running colorizer

    // Run colorizer with buffered data
    run_colorizer(config, &profile, no_color, no_context, Some(buffer))
}

/// Helper function to process and output a single chunk
fn process_and_output_chunk(
    data: &str,
    separator: &str,
    stdout: &mut StandardStream,
    compiled_patterns: &[matching::CompiledPattern],
    context_engine: &mut Option<ContextEngine>,
    config: &Config,
) -> anyhow::Result<()> {
    // Update context state first (before applying patterns)
    if let Some(ref mut engine) = context_engine {
        engine.process_line(data);
    }

    // Collect colored ranges from context rules and patterns
    let mut colored_parts: Vec<ColoredRange> = Vec::new();

    // Context-aware rules (highest priority)
    if let Some(ref engine) = context_engine {
        colored_parts.extend(engine.apply_rules(data, &|c| config.resolve_color(c)));
    }

    // Regular pattern matching (lower priority)
    colored_parts.extend(matching::apply_patterns(data, compiled_patterns));

    // Sort and remove overlaps
    colored_parts.sort_by_key(|k| k.start);
    let final_parts = remove_overlapping_ranges(colored_parts);

    // Render colored output
    render_colored_output(stdout, data, &final_parts)?;
    write!(stdout, "{}", separator)?;

    Ok(())
}

/// Remove overlapping color ranges (keeps first/higher priority)
fn remove_overlapping_ranges(ranges: Vec<ColoredRange>) -> Vec<ColoredRange> {
    let mut result = Vec::new();
    for range in ranges {
        let overlaps = result.iter().any(|r: &ColoredRange| {
            (range.start >= r.start && range.start < r.end) || (range.end > r.start && range.end <= r.end)
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
    initial_data: Option<Vec<u8>>,
) -> anyhow::Result<()> {
    let color_choice = if no_color { ColorChoice::Never } else { ColorChoice::Always };
    let mut stdout = StandardStream::stdout(color_choice);

    // Compile patterns once at startup
    let compiled_patterns = matching::compile_patterns(profile, config);
    let mut context_engine = setup_context_engine(profile, no_context);

    // Process stdin in chunks (with optional initial data from auto-detect)
    process_stdin(&mut stdout, &compiled_patterns, &mut context_engine, config, initial_data)
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
    initial_data: Option<Vec<u8>>,
) -> anyhow::Result<()> {
    use io::Read;

    const READ_SIZE: usize = 8192;
    const BATCH_DELAY_MS: u64 = 10;

    let split_regex = Regex::new(r"(\r\n?|\n)")?;
    let stdin = io::stdin();
    let mut stdin_handle = stdin.lock();
    let mut buffer = Vec::new();

    // Process initial data if provided (from auto-detect)
    if let Some(initial) = initial_data {
        buffer.extend(initial);
        let text = String::from_utf8_lossy(&buffer);
        for (data, sep) in split_text_chunks(&text, &split_regex) {
            process_and_output_chunk(&data, &sep, stdout, patterns, context_engine, config)?;
        }
        buffer.clear();
        io::stdout().flush()?;
    }

    loop {
        let mut chunk = vec![0u8; READ_SIZE];
        let bytes_read = stdin_handle.read(&mut chunk)?;

        if bytes_read == 0 {
            // EOF - process remaining data
            if !buffer.is_empty() {
                let text = String::from_utf8_lossy(&buffer);
                process_and_output_chunk(&text, "", stdout, patterns, context_engine, config)?;
            }
            break;
        }

        buffer.extend_from_slice(&chunk[..bytes_read]);
        std::thread::sleep(std::time::Duration::from_millis(BATCH_DELAY_MS));

        // Split and process chunks
        let text = String::from_utf8_lossy(&buffer);
        for (data, sep) in split_text_chunks(&text, &split_regex) {
            process_and_output_chunk(&data, &sep, stdout, patterns, context_engine, config)?;
        }

        buffer.clear();
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
