// Many threads allocating at once, Lazy SMP's or `tm-probe`'s: the system allocator does not scale
// with them, the measurement beside `mimalloc` in Cargo.toml.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `fattorini` with no arguments is the UCI engine. `--lichess`, which lichess-bot passes through its
/// `config.yml` (`engine_options`), pins each game's engine to its own physical cores
/// (`core_pinning.rs`). `about` says what the build embeds; `licenses` prints Fattorini's license
/// and the third-party notices a release embeds. `pawn-scale` measures a network's pawn.
/// `translate-tune` converts a player's old, classical tune files into its network's unit. `bench` searches a fixed set of positions and prints `<n> nodes <n> nps`
/// last (`bench::bench`); `bench <depth>` searches them to that depth instead of the default.
/// `bench-network <network> [<depth>] [--aspiration <delta>] [--scale <factor>]` benches a network
/// file instead.
/// `tm-probe [<depth>]` measures the time management's factors on those positions.
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
        ["pawn-scale"] => pawn_scale(None),
        ["pawn-scale", path] => pawn_scale(Some(path)),
        ["translate-tune", player] => translate_tune(player),
        ["bench"] => run_bench(fattorini::bench::BENCH_DEPTH),
        ["bench", depth] => match depth.parse::<u32>() {
            Ok(depth) if depth > 0 => run_bench(depth),
            _ => {
                eprintln!("fattorini bench: the depth must be a positive integer, not {depth}");
                std::process::exit(2);
            }
        },
        ["bench-network", rest @ ..] => run_bench_network(rest),
        ["tm-probe"] => run_tm_probe(fattorini::bench::TM_PROBE_DEPTH),
        ["tm-probe", depth] => match depth.parse::<u32>() {
            Ok(depth) if depth > 0 => run_tm_probe(depth),
            _ => {
                eprintln!("fattorini tm-probe: the depth must be a positive integer, not {depth}");
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!("usage: fattorini [--lichess] | fattorini about | fattorini licenses | fattorini pawn-scale [<network>] | fattorini translate-tune <player> | fattorini bench [<depth>] | fattorini bench-network <network> [<depth>] [--aspiration <delta>] [--scale <factor>] | fattorini tm-probe [<depth>]");
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
            "fattorini needs an x86-64-v3 CPU (BMI2, AVX2, FMA): Intel Haswell or later, AMD Excavator or later; this one lacks {}",
            missing.join(", ")
        );
        std::process::exit(1);
    }
}

/// `fattorini bench [<depth>]`, with what the build embeds, as the engine plays.
fn run_bench(depth: u32) {
    let evaluator = fattorini::uci::embedded_evaluator();
    let parameters = fattorini::uci::embedded_parameters()
        .unwrap_or_else(|| fattorini::search::default_parameters_for(evaluator.as_deref()));
    let (nodes, seconds) = fattorini::bench::bench_with(evaluator.as_deref(), parameters, depth);
    println!("{}", fattorini::bench::bench_line(nodes, seconds));
}

/// `fattorini bench-network <network> [<depth>] [--aspiration <delta>] [--scale <factor>]`: `bench`
/// with the network file at `<network>` in place of the embedded one, its tuning read beside it as
/// the engine reads it, `aspiration_delta` overridden when asked, its output multiplied by `factor`
/// when asked (`Network::with_output_scale`: the same evaluation in another unit's numbers). Prints the pawn percent and the parameters, then
/// `bench`'s line, last. How two networks' cost per depth is compared (an `elo` network's narrower
/// aspiration window, in its unit, was the reason it was added).
fn run_bench_network(arguments: &[&str]) {
    let usage = || -> ! {
        eprintln!("usage: fattorini bench-network <network> [<depth>] [--aspiration <delta>] [--scale <factor>]");
        std::process::exit(2);
    };
    let [path, rest @ ..] = arguments else { usage() };
    let mut depth = fattorini::bench::BENCH_DEPTH;
    let mut aspiration = None;
    let mut scale = None;
    let mut rest = rest.iter();
    while let Some(&argument) = rest.next() {
        match argument {
            "--scale" => match rest.next().map(|value| value.parse::<f32>()) {
                Some(Ok(factor)) if factor > 0.0 && scale.is_none() => scale = Some(factor),
                _ => usage(),
            },
            "--aspiration" => match rest.next().map(|value| value.parse::<i32>()) {
                Some(Ok(delta)) if delta > 0 && aspiration.is_none() => aspiration = Some(delta),
                _ => usage(),
            },
            _ => match argument.parse::<u32>() {
                Ok(value) if value > 0 && depth == fattorini::bench::BENCH_DEPTH => depth = value,
                _ => usage(),
            },
        }
    }
    let network = match fattorini::nnue::Network::load(path) {
        Ok(network) => match scale {
            Some(factor) => network.with_output_scale(factor),
            None => network,
        },
        Err(error) => {
            eprintln!("fattorini bench-network: {error}");
            std::process::exit(1);
        }
    };
    let mut parameters = fattorini::search::default_parameters_for(Some(&network));
    if let Some(delta) = aspiration {
        parameters.aspiration_delta = delta;
    }
    println!("pawn percent {}", network.pawn_percent());
    let values: Vec<String> = fattorini::search::SearchParameters::NAMES
        .iter()
        .zip(parameters.values())
        .map(|(name, value)| format!("{name} {value}"))
        .collect();
    println!("{}", values.join(", "));
    let (nodes, seconds) = fattorini::bench::bench_with(Some(&network), parameters, depth);
    println!("{}", fattorini::bench::bench_line(nodes, seconds));
}

/// `fattorini tm-probe [<depth>]`, with what the build embeds: the time management's factors
/// on the bench positions (`bench::tm_probe`), one tab-separated line each, the medians last.
fn run_tm_probe(depth: u32) {
    let evaluator = fattorini::uci::embedded_evaluator();
    let parameters = fattorini::uci::embedded_parameters()
        .unwrap_or_else(|| fattorini::search::default_parameters_for(evaluator.as_deref()));
    for line in fattorini::bench::tm_probe(evaluator.as_deref(), parameters, depth) {
        println!("{line}");
    }
}

/// Fattorini's own license, with the additional term the README's License section adds under the
/// AGPLv3's section 7(e): printed by every build, since a binary travels without the README.
const OWN_LICENSE: &str = "\
Fattorini, Copyright (C) 2026 Calogero Zarba
Licensed under the GNU Affero General Public License, version 3 only, with no warranty.
As an additional term under its section 7(e), no right is granted to use the name \"Fattorini\"
or its logo for a modified version: a fork takes another name, another UCI id name and another icon.
The source, the full license and its terms: https://github.com/cigizeta/fattorini
";

/// `fattorini licenses`: Fattorini's own license, then the third-party notices, which only a
/// release embeds.
fn licenses() {
    println!("{OWN_LICENSE}");
    let notices = fattorini::uci::embedded_notices();
    if notices.is_empty() {
        println!("Third-party notices: embedded in a release only, not in this development build.");
    } else {
        print!("{notices}");
    }
}

/// `fattorini pawn-scale [<network>]`: the network's pawn against a material pawn (PeSTO's), in
/// percent, as the engine measures it at load to translate its untuned defaults (`nnue::measure_label_percent`):
/// `network` a `.nnue` file or a player's name (`<config>-g<g>-a<n>`, any `@<tuning>` ignored, the
/// pawn being the network's), else the embedded one. A material pawn up reads as that many centipawns.
fn pawn_scale(network: Option<&str>) {
    let path = network.map(|network| {
        if network.ends_with(".nnue") {
            return network.to_string();
        }
        let player = network.split_once('@').map_or(network, |(player, _)| player);
        fattorini::pipeline::players::resolve_network_file(player).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1);
        })
    });
    let (name, percent) = match path.as_deref() {
        Some(path) => match fattorini::nnue::Network::load(path) {
            Ok(network) => (path.to_string(), network.pawn_percent()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        },
        None => match fattorini::uci::embedded_evaluator() {
            Some(network) => ("the embedded network".to_string(), network.pawn_percent()),
            None => {
                println!("no network embedded: the PeSTO evaluation, whose pawn is 100% by definition");
                return;
            }
        },
    };
    println!("{name}: pawn scale {percent}%, a material pawn up reads as about {percent} centipawns");
}

/// `fattorini translate-tune <player>`: one player's old, classical tune files in its network's
/// unit (`players::translate_tune`); `smithy translate-tunes` does every player.
fn translate_tune(player: &str) {
    match fattorini::pipeline::players::translate_tune(player) {
        Ok(report) if report.is_empty() => println!("{player}: no tune files"),
        Ok(report) => report.iter().for_each(|line| println!("{line}")),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
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
