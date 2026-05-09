use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use clap::Parser;
use sniffdiff::{
    DEFAULT_CALLER_PREVIEW_LIMIT, PublicAnalysisOptions, ReportLimit, analyze_repo_json,
    analyze_repo_report, analyze_repo_report_verbose,
};

#[derive(Debug, Parser)]
#[command(name = "sniffdiff")]
#[command(
    about = "Semantic commit analysis for Python: blast radius, symbol diffs and complexity changes."
)]
struct Cli {
    /// Git range in base..head form. Uses --base/--head when omitted.
    range: Option<String>,

    /// Base ref used when RANGE is omitted.
    #[arg(long, default_value = "main")]
    base: String,

    /// Head ref used when RANGE is omitted.
    #[arg(long, default_value = "HEAD")]
    head: String,

    /// Emit exhaustive JSON facts for tooling.
    #[arg(long)]
    json: bool,

    /// Include more items and per-item facts in the same report shape.
    #[arg(long, conflicts_with = "json")]
    verbose: bool,

    /// Maximum number of report items to show, or "all".
    #[arg(long, default_value = "5", value_parser = parse_report_limit)]
    limit: ReportLimit,

    /// Maximum caller/test references to preview per report item.
    #[arg(long, default_value_t = DEFAULT_CALLER_PREVIEW_LIMIT, value_parser = parse_positive_usize)]
    caller_preview_limit: usize,

    /// Return partial facts when Python parse errors are present.
    #[arg(long)]
    allow_parse_errors: bool,

    /// Repository root.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (base, head) = parse_refs(&cli);
    let options = PublicAnalysisOptions {
        allow_parse_errors: cli.allow_parse_errors,
        report_limit: cli.limit,
        caller_preview_limit: cli.caller_preview_limit,
    };

    if cli.json {
        println!("{}", analyze_repo_json(cli.repo, &base, &head, options)?);
    } else {
        let sniff = SniffLine::choose();
        let show_progress = io::stderr().is_terminal();
        if show_progress {
            eprint!("{}", sniff.running());
            let _ = io::stderr().flush();
        }
        let output = if cli.verbose {
            analyze_repo_report_verbose(cli.repo, &base, &head, options)?
        } else {
            analyze_repo_report(cli.repo, &base, &head, options)?
        };
        if show_progress {
            eprint!("\r\x1b[K");
        }
        println!("{output}");
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct SniffLine {
    text: SniffText,
    nose: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct SniffText {
    running: &'static str,
}

impl SniffLine {
    fn choose() -> Self {
        const TEXTS: &[SniffText] = &[
            SniffText {
                running: "Sniffing the diff...",
            },
            SniffText {
                running: "Taking a whiff of the diff...",
            },
            SniffText {
                running: "Nose down in the diff...",
            },
            SniffText {
                running: "Following the diff scent...",
            },
            SniffText {
                running: "Consulting the nose...",
            },
        ];
        const NOSES: &[&str] = &["(°🐽°)", "🐽", "👃", "(°👃°)"];

        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos() as usize)
            .unwrap_or(0);
        Self {
            text: TEXTS[seed % TEXTS.len()],
            nose: NOSES[(seed / TEXTS.len()) % NOSES.len()],
        }
    }

    fn running(&self) -> String {
        format!("{} {}", self.text.running, self.nose)
    }
}

fn parse_refs(cli: &Cli) -> (String, String) {
    if let Some((base, head)) = cli
        .range
        .as_deref()
        .and_then(|range| range.split_once(".."))
    {
        return (base.to_string(), head.to_string());
    }

    (cli.base.clone(), cli.head.clone())
}

fn parse_report_limit(value: &str) -> Result<ReportLimit, String> {
    if value.eq_ignore_ascii_case("all") {
        return Ok(ReportLimit::All);
    }

    let limit = value
        .parse::<usize>()
        .map_err(|_| "expected a positive integer or \"all\"".to_string())?;
    if limit == 0 {
        return Err("expected a positive integer or \"all\"".to_string());
    }

    Ok(ReportLimit::Limited(limit))
}

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let limit = value
        .parse::<usize>()
        .map_err(|_| "expected a positive integer".to_string())?;
    if limit == 0 {
        return Err("expected a positive integer".to_string());
    }

    Ok(limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_lines_format_running_text_with_nose() {
        let line = SniffLine {
            text: SniffText {
                running: "Following the diff scent...",
            },
            nose: "👃",
        };

        assert_eq!(line.running(), "Following the diff scent... 👃");
    }

    #[test]
    fn parses_report_limit() {
        assert_eq!(parse_report_limit("all"), Ok(ReportLimit::All));
        assert_eq!(parse_report_limit("7"), Ok(ReportLimit::Limited(7)));
        assert!(parse_report_limit("0").is_err());
        assert!(parse_report_limit("many").is_err());
    }

    #[test]
    fn parses_positive_usize() {
        assert_eq!(parse_positive_usize("4"), Ok(4));
        assert!(parse_positive_usize("0").is_err());
        assert!(parse_positive_usize("many").is_err());
    }
}
