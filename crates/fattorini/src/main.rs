/// `fattorini` with no arguments is the UCI engine. `--lichess`, which lichess-bot passes through its
/// `config.yml` (`engine_options`), pins each game's engine to its own physical cores
/// (`core_pinning.rs`). `about` says what the build embeds; `licenses` prints the third-party notices
/// a release embeds. `bench` searches a fixed set of positions and prints `<n> nodes <n> nps`
/// last (`bench::bench`); `bench <depth>` searches them to that depth instead of the default.
///
/// Any other argument stops the engine, so a typo is never silently ignored.
fn main() {
    refuse_an_older_cpu();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match arguments.as_slice() {
        [] => fattorini::uci::run(false),
        ["--lichess"] => fattorini::uci::run(true),
        ["about"] => about(),
        ["licenses"] => licenses(),
        ["bench"] => run_bench(fattorini::bench::BENCH_DEPTH),
        ["bench", depth] => match depth.parse::<u32>() {
            Ok(depth) if depth > 0 => run_bench(depth),
            _ => {
                eprintln!("fattorini bench: the depth must be a positive integer, not {depth}");
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!("usage: fattorini [--lichess] | fattorini about | fattorini licenses | fattorini bench [<depth>]");
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
            "fattorini needs an x86-64-v3 CPU (BMI2, AVX2, FMA): Intel Haswell or later, AMD Zen or later; this one lacks {}",
            missing.join(", ")
        );
        std::process::exit(1);
    }
}

/// `fattorini bench [<depth>]`, with what the build embeds, as the engine plays.
fn run_bench(depth: u32) {
    let evaluator = fattorini::uci::embedded_evaluator();
    let parameters = fattorini::uci::embedded_parameters();
    let (nodes, seconds) = fattorini::bench::bench_with(evaluator.as_deref(), parameters, depth);
    println!("{}", fattorini::bench::bench_line(nodes, seconds));
}

/// `fattorini licenses`: the third-party notices, which only a release embeds.
fn licenses() {
    let notices = fattorini::uci::embedded_notices();
    if notices.is_empty() {
        println!("no notices embedded: a development build");
    } else {
        print!("{notices}");
    }
}

/// `fattorini about`: what the build embeds, as its description says.
fn about() {
    let description = fattorini::uci::embedded_description();
    if description.is_empty() {
        println!("Fattorini {}: a development build, PeSTO, untuned", env!("CARGO_PKG_VERSION"));
    } else {
        println!("Fattorini {} {description}", env!("CARGO_PKG_VERSION"));
    }
}
