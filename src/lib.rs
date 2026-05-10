pub(crate) mod analysis;
pub(crate) mod git;
pub(crate) mod language;
pub(crate) mod output;
pub(crate) mod python;
pub(crate) mod timing;

use std::path::PathBuf;

use anyhow::Result;

use crate::analysis::{Analysis, AnalysisOptions, analyze};
use crate::git::ShellGit;
use crate::output::{JsonAnalysis, render_json, render_yaml, report_model};
use crate::python::PythonAdapter;
use crate::timing::TimingRecorder;

pub use crate::output::report::ReportVerbosity;
pub use crate::timing::TimingReport;

pub const INDEX_REF: &str = crate::git::INDEX_REF;
pub const WORKTREE_REF: &str = crate::git::WORKTREE_REF;
pub const DEFAULT_CALLER_PREVIEW_LIMIT: usize = 4;

#[derive(Debug, Clone, Copy)]
pub struct PublicAnalysisOptions {
    pub allow_parse_errors: bool,
    pub report_limit: ReportLimit,
    pub caller_preview_limit: usize,
    pub report_verbosity: ReportVerbosity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportLimit {
    Limited(usize),
    All,
}

pub struct TimedOutput {
    pub output: String,
    pub timings: TimingReport,
}

impl Default for ReportLimit {
    fn default() -> Self {
        Self::Limited(5)
    }
}

impl Default for PublicAnalysisOptions {
    fn default() -> Self {
        Self {
            allow_parse_errors: false,
            report_limit: ReportLimit::default(),
            caller_preview_limit: DEFAULT_CALLER_PREVIEW_LIMIT,
            report_verbosity: ReportVerbosity::Normal,
        }
    }
}

pub fn analyze_repo_json(
    repo: impl Into<PathBuf>,
    base: &str,
    head: &str,
    options: PublicAnalysisOptions,
) -> Result<String> {
    let analysis = analyze_repo(repo, base, head, options)?;
    let report = report_model(
        &analysis,
        options.report_verbosity,
        options.report_limit,
        options.caller_preview_limit,
    );
    render_json(&report)
}

pub fn analyze_repo_json_with_timing(
    repo: impl Into<PathBuf>,
    base: &str,
    head: &str,
    options: PublicAnalysisOptions,
) -> Result<TimedOutput> {
    let mut timings = TimingRecorder::start();
    let analysis = analyze_repo_recorded(repo, base, head, options, &mut timings)?;
    let report = timings.time_result("report_model", || {
        Ok(report_model(
            &analysis,
            options.report_verbosity,
            options.report_limit,
            options.caller_preview_limit,
        ))
    })?;
    let output = timings.time_result("render", || render_json(&report))?;
    Ok(TimedOutput {
        output,
        timings: timings.finish(),
    })
}

pub fn analyze_repo_raw_json(
    repo: impl Into<PathBuf>,
    base: &str,
    head: &str,
    options: PublicAnalysisOptions,
) -> Result<String> {
    let analysis = analyze_repo(repo, base, head, options)?;
    Ok(serde_json::to_string_pretty(&JsonAnalysis::from(
        &analysis,
    ))?)
}

pub fn analyze_repo_report(
    repo: impl Into<PathBuf>,
    base: &str,
    head: &str,
    options: PublicAnalysisOptions,
) -> Result<String> {
    let analysis = analyze_repo(repo, base, head, options)?;
    let report = report_model(
        &analysis,
        options.report_verbosity,
        options.report_limit,
        options.caller_preview_limit,
    );
    render_yaml(&report)
}

pub fn analyze_repo_report_with_timing(
    repo: impl Into<PathBuf>,
    base: &str,
    head: &str,
    options: PublicAnalysisOptions,
) -> Result<TimedOutput> {
    let mut timings = TimingRecorder::start();
    let analysis = analyze_repo_recorded(repo, base, head, options, &mut timings)?;
    let report = timings.time_result("report_model", || {
        Ok(report_model(
            &analysis,
            options.report_verbosity,
            options.report_limit,
            options.caller_preview_limit,
        ))
    })?;
    let output = timings.time_result("render", || render_yaml(&report))?;
    Ok(TimedOutput {
        output,
        timings: timings.finish(),
    })
}

pub fn analyze_repo_report_verbose(
    repo: impl Into<PathBuf>,
    base: &str,
    head: &str,
    mut options: PublicAnalysisOptions,
) -> Result<String> {
    options.report_verbosity = ReportVerbosity::Verbose;
    analyze_repo_report(repo, base, head, options)
}

pub fn merge_base(repo: impl Into<PathBuf>, left: &str, right: &str) -> Result<String> {
    ShellGit::new(repo).merge_base(left, right)
}

fn analyze_repo(
    repo: impl Into<PathBuf>,
    base: &str,
    head: &str,
    options: PublicAnalysisOptions,
) -> Result<Analysis> {
    let git = ShellGit::new(repo);
    let adapter = PythonAdapter;
    analyze(
        &git,
        &adapter,
        base,
        head,
        AnalysisOptions {
            allow_parse_errors: options.allow_parse_errors,
        },
    )
}

fn analyze_repo_recorded(
    repo: impl Into<PathBuf>,
    base: &str,
    head: &str,
    options: PublicAnalysisOptions,
    timings: &mut TimingRecorder,
) -> Result<Analysis> {
    let git = ShellGit::new(repo);
    let adapter = PythonAdapter;
    crate::analysis::analyze_recorded(
        &git,
        &adapter,
        base,
        head,
        AnalysisOptions {
            allow_parse_errors: options.allow_parse_errors,
        },
        timings,
    )
}
