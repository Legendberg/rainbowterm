//! Performance benchmarks for RainbowTerm
//!
//! Run with: cargo bench
//! View HTML reports at: target/criterion/report/index.html

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rainbowterm::config::Config;
use rainbowterm::matching::{apply_patterns, compile_patterns};

/// Embedded default config (same as used in main.rs)
const DEFAULT_CONFIG: &str = include_str!("../config.toml");

/// Sample network device output for benchmarks
const JUNIPER_SAMPLE: &str = include_str!("../tests/networking/juniper/common/sample1.conf");

/// Single line samples for micro-benchmarks
const SAMPLE_LINES: &[&str] = &[
    // Interface status line
    "ge-0/0/0                up    up   inet     192.168.1.1/24",
    // BGP summary line
    "10.0.0.1              65001     123456     123456       0       2     2:15:23 Establ",
    // Interface extensive line with counters
    "  Input rate     : 1234567890 bps (1234567 pps)",
    // Log line
    "Dec 11 10:15:23  router rpd[1234]: RPD_OSPF_NBRDOWN: OSPF neighbor 10.0.0.2 state changed",
    // MAC address table
    "    default             00:25:90:35:bb:00   D             -   ae0.0                  0",
    // Simple line without patterns
    "This is a line with no patterns to match",
    // Error counters (zero - healthy)
    "  Input errors:  0",
    // Error counters (non-zero)
    "  Input errors:  1234567890",
    // IPv6 address
    "                                   inet6    2001:db8::1/64",
    // Complex config diff line
    "+   neighbor 172.16.0.1 { peer-as 65003; }",
];

fn bench_pattern_compilation(c: &mut Criterion) {
    let config = Config::parse(DEFAULT_CONFIG).expect("Failed to parse config");

    let mut group = c.benchmark_group("pattern_compilation");

    for profile_name in ["base", "juniper", "cisco", "versa"] {
        if let Some(profile) = config.get_profile(profile_name) {
            group.bench_with_input(
                BenchmarkId::new("compile", profile_name),
                &profile,
                |b, profile| {
                    b.iter(|| compile_patterns(black_box(profile), black_box(&config)));
                },
            );
        }
    }

    group.finish();
}

fn bench_single_line_matching(c: &mut Criterion) {
    let config = Config::parse(DEFAULT_CONFIG).expect("Failed to parse config");
    let juniper_profile = config.get_profile("juniper").expect("juniper profile");
    let compiled = compile_patterns(&juniper_profile, &config);

    let mut group = c.benchmark_group("single_line");

    for (i, line) in SAMPLE_LINES.iter().enumerate() {
        group.throughput(Throughput::Bytes(line.len() as u64));
        group.bench_with_input(BenchmarkId::new("match", i), line, |b, line| {
            b.iter(|| apply_patterns(black_box(line), black_box(&compiled)));
        });
    }

    group.finish();
}

fn bench_full_output_processing(c: &mut Criterion) {
    let config = Config::parse(DEFAULT_CONFIG).expect("Failed to parse config");

    let mut group = c.benchmark_group("full_output");
    group.throughput(Throughput::Bytes(JUNIPER_SAMPLE.len() as u64));

    for profile_name in ["base", "juniper"] {
        if let Some(profile) = config.get_profile(profile_name) {
            let compiled = compile_patterns(&profile, &config);

            group.bench_with_input(
                BenchmarkId::new("process", profile_name),
                &compiled,
                |b, compiled| {
                    b.iter(|| {
                        // Process each line individually (simulates real usage)
                        for line in JUNIPER_SAMPLE.lines() {
                            let _ = apply_patterns(black_box(line), black_box(compiled));
                        }
                    });
                },
            );
        }
    }

    group.finish();
}

fn bench_config_parsing(c: &mut Criterion) {
    c.bench_function("config_parse", |b| {
        b.iter(|| Config::parse(black_box(DEFAULT_CONFIG)));
    });
}

fn bench_throughput_scaling(c: &mut Criterion) {
    let config = Config::parse(DEFAULT_CONFIG).expect("Failed to parse config");
    let juniper_profile = config.get_profile("juniper").expect("juniper profile");
    let compiled = compile_patterns(&juniper_profile, &config);

    let mut group = c.benchmark_group("throughput_scaling");

    // Test with different input sizes (1x, 10x, 100x the sample)
    for multiplier in [1, 10, 100] {
        let input: String = std::iter::repeat(JUNIPER_SAMPLE)
            .take(multiplier)
            .collect::<Vec<_>>()
            .join("\n");

        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::new("lines", multiplier), &input, |b, input| {
            b.iter(|| {
                for line in input.lines() {
                    let _ = apply_patterns(black_box(line), black_box(&compiled));
                }
            });
        });
    }

    group.finish();
}

fn bench_pattern_count_impact(c: &mut Criterion) {
    let config = Config::parse(DEFAULT_CONFIG).expect("Failed to parse config");

    let mut group = c.benchmark_group("pattern_count_impact");

    // Compare base (fewer patterns) vs juniper (more patterns)
    let profiles = [
        ("base", config.get_profile("base").expect("base")),
        ("juniper", config.get_profile("juniper").expect("juniper")),
    ];

    let test_line = "ge-0/0/0.0              up    up   inet     192.168.1.1/24";
    group.throughput(Throughput::Bytes(test_line.len() as u64));

    for (name, profile) in profiles {
        let compiled = compile_patterns(&profile, &config);
        let pattern_count = compiled.len();

        group.bench_with_input(
            BenchmarkId::new(format!("patterns_{}", pattern_count), name),
            &compiled,
            |b, compiled| {
                b.iter(|| apply_patterns(black_box(test_line), black_box(compiled)));
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_config_parsing,
    bench_pattern_compilation,
    bench_single_line_matching,
    bench_full_output_processing,
    bench_throughput_scaling,
    bench_pattern_count_impact,
);

criterion_main!(benches);
