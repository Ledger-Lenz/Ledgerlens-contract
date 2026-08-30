use anyhow::{bail, Context, Result};
use invocation_fuzzer::{
    load_campaign, load_corpus, replay_campaign, run_fuzz, DEFAULT_CASES, MAX_CASES,
};
use std::env;
use std::path::{Path, PathBuf};

fn usage() -> &'static str {
    "Usage:
  cargo run -p invocation-fuzzer --locked -- smoke [--seed N] [--cases N] [--corpus-dir PATH]
  cargo run -p invocation-fuzzer --locked -- replay PATH

smoke replays every regression fixture before running a bounded deterministic
corpus-guided campaign. replay executes the selected fixture twice and rejects
any observable mismatch."
}

fn parse_number<T>(value: Option<String>, flag: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .with_context(|| format!("missing value for {flag}"))?
        .parse()
        .map_err(|error| anyhow::anyhow!("invalid {flag}: {error}"))
}

fn smoke(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut seed = 0x6390_c0de_u64;
    let mut cases = DEFAULT_CASES;
    let mut corpus_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus");

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--seed" => seed = parse_number(args.next(), "--seed")?,
            "--cases" => cases = parse_number(args.next(), "--cases")?,
            "--corpus-dir" => {
                corpus_dir = PathBuf::from(args.next().context("missing value for --corpus-dir")?)
            }
            _ => bail!("unknown smoke option {flag}\n\n{}", usage()),
        }
    }
    if cases > MAX_CASES {
        bail!("--cases cannot exceed {MAX_CASES}");
    }

    let corpus = load_corpus(&corpus_dir)?;
    let summary = run_fuzz(corpus, seed, cases)?;
    println!(
        "PASS seed={} regression_cases={} generated_cases={} retained_cases={} behavior_signatures={}",
        summary.seed,
        summary.regression_cases,
        summary.generated_cases,
        summary.retained_cases,
        summary.behavior_signatures
    );
    println!(
        "MAX cpu_instructions={} memory_bytes={} encoded_input_bytes={} operations={} raw_arguments={} logical_score_reads={} logical_score_writes={} contract_events={}",
        summary.max_resources.cpu_instructions,
        summary.max_resources.memory_bytes,
        summary.max_resources.encoded_input_bytes,
        summary.max_resources.operations,
        summary.max_resources.raw_arguments,
        summary.max_resources.logical_score_reads,
        summary.max_resources.logical_score_writes,
        summary.max_resources.contract_events,
    );
    Ok(())
}

fn replay(path: &Path) -> Result<()> {
    let campaign = load_campaign(path)?;
    let report = replay_campaign(&campaign)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    println!("PASS deterministic replay: {}", path.display());
    Ok(())
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("smoke") => smoke(args),
        Some("replay") => {
            let path = args.next().context("replay requires a fixture path")?;
            if args.next().is_some() {
                bail!("replay accepts exactly one fixture path");
            }
            replay(Path::new(&path))
        }
        Some("-h" | "--help") => {
            println!("{}", usage());
            Ok(())
        }
        Some(command) => bail!("unknown command {command}\n\n{}", usage()),
        None => bail!("{}", usage()),
    }
}
