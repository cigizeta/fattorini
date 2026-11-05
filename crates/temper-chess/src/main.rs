/// `temper-chess` with no arguments is the UCI engine. `--lichess`, which lichess-bot passes through its
/// `config.yml` (`engine_options`), pins each game's engine to its own physical cores
/// (`core_pinning.rs`). `bench` searches a fixed set of positions and prints `<n> nodes <n> nps`
/// last (`bench::bench`); `bench <depth>` searches them to that depth instead of the default.
///
/// Any other argument stops the engine, so a typo is never silently ignored.
fn main() {
    refuse_an_older_cpu();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match arguments.as_slice() {
        [] => temper_chess::uci::run(false),
        ["--lichess"] => temper_chess::uci::run(true),
        ["bench"] => run_bench(temper_chess::bench::BENCH_DEPTH),
        ["bench", depth] => match depth.parse::<u32>() {
            Ok(depth) if depth > 0 => run_bench(depth),
            _ => {
                eprintln!("temper-chess bench: the depth must be a positive integer, not {depth}");
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!("usage: temper-chess [--lichess] | temper-chess bench [<depth>]");
            std::process::exit(2);
        }
    }
}

/// Exits with one line on stderr when the CPU lacks what the build is compiled for (x86-64-v3,
/// `.cargo/config.toml`), before any UCI exchange: without it, the first move generation's PEXT or
/// the first AVX2 kernel dies on an illegal instruction, which a GUI reports as a crash. First in
/// `main`, since the compiler may use those instructions anywhere; in principle it may already have
/// before this check, but the check itself is a CPUID query and a print.
fn refuse_an_older_cpu() {
    let mut missing = Vec::new();
    if !is_x86_feature_detected!("bmi2") {
        missing.push("BMI2");
    }
    if !is_x86_feature_detected!("avx2") {
        missing.push("AVX2");
    }
    if !is_x86_feature_detected!("fma") {
        missing.push("FMA");
    }
    if !missing.is_empty() {
        eprintln!(
            "temper-chess needs an x86-64-v3 CPU (BMI2, AVX2, FMA): Intel Haswell or later, AMD Zen or later; this one lacks {}",
            missing.join(", ")
        );
        std::process::exit(1);
    }
}

/// `temper-chess bench [<depth>]`, with the network the engine itself would start with.
fn run_bench(depth: u32) {
    let evaluator = temper_chess::uci::evaluator_for(&temper_chess::uci::installed_eval_file());
    let (nodes, seconds) = temper_chess::bench::bench_at_depth(evaluator.as_deref(), depth);
    println!("{}", temper_chess::bench::bench_line(nodes, seconds));
}
