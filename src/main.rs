use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use sniffdiff::{
    DEFAULT_CALLER_PREVIEW_LIMIT, INDEX_REF, PublicAnalysisOptions, ReportLimit, ReportVerbosity,
    WORKTREE_REF, analyze_repo_json, analyze_repo_json_with_timing, analyze_repo_report,
    analyze_repo_report_with_timing, merge_base,
};

#[derive(Debug, Parser)]
#[command(name = "sniffdiff")]
#[command(about = "Review Python diffs by changed symbols, callers, tests, and complexity.")]
struct Cli {
    /// Git range/ref, following git diff forms: ref, ref1..ref2, or ref1...ref2.
    range: Option<String>,

    /// Base ref for explicit --base/--head comparisons.
    #[arg(long, requires = "head")]
    base: Option<String>,

    /// Head ref for explicit --base/--head comparisons.
    #[arg(long, requires = "base")]
    head: Option<String>,

    /// Compare HEAD, or REF when provided, to the Git index.
    #[arg(long, visible_alias = "cached")]
    staged: bool,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Yaml)]
    format: OutputFormat,

    /// Report verbosity level.
    #[arg(long, visible_alias = "detail", value_enum, default_value_t = CliReportVerbosity::Normal)]
    verbosity: CliReportVerbosity,

    /// Emit the same report model as JSON. Shorthand for --format json.
    #[arg(long, conflicts_with = "format")]
    json: bool,

    /// Include per-item facts. Shorthand for --verbosity verbose.
    #[arg(long)]
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

    /// Print internal timing diagnostics to stderr.
    #[arg(long, hide = true)]
    timing: bool,

    /// Repository root.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (base, head) = resolve_refs(&cli)?;
    let verbosity = if cli.verbose {
        ReportVerbosity::Verbose
    } else {
        cli.verbosity.into()
    };
    let options = PublicAnalysisOptions {
        allow_parse_errors: cli.allow_parse_errors,
        report_limit: cli.limit,
        caller_preview_limit: cli.caller_preview_limit,
        report_verbosity: verbosity,
    };

    let format = if cli.json {
        OutputFormat::Json
    } else {
        cli.format
    };

    if cli.timing {
        let timed = if format == OutputFormat::Json {
            analyze_repo_json_with_timing(cli.repo, &base, &head, options)?
        } else {
            analyze_repo_report_with_timing(cli.repo, &base, &head, options)?
        };
        eprint!("{}", timed.timings.render_text());
        println!("{}", timed.output);
    } else if format == OutputFormat::Json {
        println!("{}", analyze_repo_json(cli.repo, &base, &head, options)?);
    } else {
        println!("{}", analyze_repo_report(cli.repo, &base, &head, options)?);
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RefSelection {
    Direct { base: String, head: String },
    MergeBase { left: String, right: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Yaml,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliReportVerbosity {
    Normal,
    Verbose,
    Full,
}

impl From<CliReportVerbosity> for ReportVerbosity {
    fn from(verbosity: CliReportVerbosity) -> Self {
        match verbosity {
            CliReportVerbosity::Normal => Self::Normal,
            CliReportVerbosity::Verbose => Self::Verbose,
            CliReportVerbosity::Full => Self::Full,
        }
    }
}

fn resolve_refs(cli: &Cli) -> Result<(String, String)> {
    match parse_ref_selection(cli)? {
        RefSelection::Direct { base, head } => Ok((base, head)),
        RefSelection::MergeBase { left, right } => {
            Ok((merge_base(&cli.repo, &left, &right)?, right))
        }
    }
}

fn parse_ref_selection(cli: &Cli) -> Result<RefSelection> {
    if cli.staged && (cli.base.is_some() || cli.head.is_some()) {
        anyhow::bail!("--staged cannot be combined with --base/--head");
    }

    if let Some((left, right)) = cli
        .range
        .as_deref()
        .and_then(|range| range.split_once("..."))
    {
        if cli.staged {
            anyhow::bail!("--staged cannot be combined with an explicit ref range");
        }
        return Ok(RefSelection::MergeBase {
            left: default_empty_ref(left),
            right: default_empty_ref(right),
        });
    }

    if let Some((base, head)) = cli
        .range
        .as_deref()
        .and_then(|range| range.split_once(".."))
    {
        if cli.staged {
            anyhow::bail!("--staged cannot be combined with an explicit ref range");
        }

        return Ok(RefSelection::Direct {
            base: default_empty_ref(base),
            head: default_empty_ref(head),
        });
    }

    if let Some(base) = cli.range.as_deref() {
        if cli.staged {
            return Ok(RefSelection::Direct {
                base: base.to_string(),
                head: INDEX_REF.to_string(),
            });
        }

        return Ok(RefSelection::Direct {
            base: base.to_string(),
            head: WORKTREE_REF.to_string(),
        });
    }

    if let (Some(base), Some(head)) = (&cli.base, &cli.head) {
        return Ok(RefSelection::Direct {
            base: base.clone(),
            head: head.clone(),
        });
    }

    if cli.staged {
        return Ok(RefSelection::Direct {
            base: "HEAD".to_string(),
            head: INDEX_REF.to_string(),
        });
    }

    Ok(RefSelection::Direct {
        base: INDEX_REF.to_string(),
        head: WORKTREE_REF.to_string(),
    })
}

fn default_empty_ref(git_ref: &str) -> String {
    if git_ref.is_empty() {
        "HEAD".to_string()
    } else {
        git_ref.to_string()
    }
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

    fn test_cli(range: Option<&str>, staged: bool) -> Cli {
        Cli {
            range: range.map(str::to_string),
            base: None,
            head: None,
            staged,
            format: OutputFormat::Yaml,
            verbosity: CliReportVerbosity::Normal,
            json: false,
            verbose: false,
            limit: ReportLimit::default(),
            caller_preview_limit: DEFAULT_CALLER_PREVIEW_LIMIT,
            allow_parse_errors: false,
            timing: false,
            repo: PathBuf::from("."),
        }
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

    #[test]
    fn parses_single_ref_as_worktree_comparison() {
        let cli = test_cli(Some("main"), false);

        assert_eq!(
            parse_ref_selection(&cli).unwrap(),
            RefSelection::Direct {
                base: "main".to_string(),
                head: WORKTREE_REF.to_string()
            }
        );
    }

    #[test]
    fn parses_explicit_range_as_ref_comparison() {
        let cli = test_cli(Some("main..HEAD"), false);

        assert_eq!(
            parse_ref_selection(&cli).unwrap(),
            RefSelection::Direct {
                base: "main".to_string(),
                head: "HEAD".to_string()
            }
        );
    }

    #[test]
    fn parses_triple_dot_as_merge_base_comparison() {
        let cli = test_cli(Some("main...HEAD"), false);

        assert_eq!(
            parse_ref_selection(&cli).unwrap(),
            RefSelection::MergeBase {
                left: "main".to_string(),
                right: "HEAD".to_string()
            }
        );
    }

    #[test]
    fn defaults_empty_two_dot_range_sides_to_head() {
        assert_eq!(
            parse_ref_selection(&test_cli(Some("main.."), false)).unwrap(),
            RefSelection::Direct {
                base: "main".to_string(),
                head: "HEAD".to_string()
            }
        );
        assert_eq!(
            parse_ref_selection(&test_cli(Some("..feature"), false)).unwrap(),
            RefSelection::Direct {
                base: "HEAD".to_string(),
                head: "feature".to_string()
            }
        );
    }

    #[test]
    fn defaults_empty_triple_dot_range_sides_to_head() {
        assert_eq!(
            parse_ref_selection(&test_cli(Some("main..."), false)).unwrap(),
            RefSelection::MergeBase {
                left: "main".to_string(),
                right: "HEAD".to_string()
            }
        );
        assert_eq!(
            parse_ref_selection(&test_cli(Some("...feature"), false)).unwrap(),
            RefSelection::MergeBase {
                left: "HEAD".to_string(),
                right: "feature".to_string()
            }
        );
    }

    #[test]
    fn parses_no_ref_as_index_to_worktree_comparison() {
        let cli = test_cli(None, false);

        assert_eq!(
            parse_ref_selection(&cli).unwrap(),
            RefSelection::Direct {
                base: INDEX_REF.to_string(),
                head: WORKTREE_REF.to_string()
            }
        );
    }

    #[test]
    fn parses_staged_without_ref_as_head_to_index_comparison() {
        let cli = test_cli(None, true);

        assert_eq!(
            parse_ref_selection(&cli).unwrap(),
            RefSelection::Direct {
                base: "HEAD".to_string(),
                head: INDEX_REF.to_string()
            }
        );
    }

    #[test]
    fn parses_staged_with_ref_as_ref_to_index_comparison() {
        let cli = test_cli(Some("main"), true);

        assert_eq!(
            parse_ref_selection(&cli).unwrap(),
            RefSelection::Direct {
                base: "main".to_string(),
                head: INDEX_REF.to_string()
            }
        );
    }

    #[test]
    fn parses_cached_as_staged_alias() {
        let cli = Cli::try_parse_from(["sniffdiff", "--cached"]).unwrap();

        assert!(cli.staged);
        assert_eq!(
            parse_ref_selection(&cli).unwrap(),
            RefSelection::Direct {
                base: "HEAD".to_string(),
                head: INDEX_REF.to_string()
            }
        );
    }

    #[test]
    fn parses_hidden_timing_flag() {
        let cli = Cli::try_parse_from(["sniffdiff", "--timing"]).unwrap();

        assert!(cli.timing);
    }

    #[test]
    fn rejects_staged_with_triple_dot_range() {
        let cli = test_cli(Some("main...HEAD"), true);

        assert!(parse_ref_selection(&cli).is_err());
    }
}
