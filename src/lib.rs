pub(crate) mod analysis;
pub(crate) mod git;
pub(crate) mod language;
pub(crate) mod output;
pub(crate) mod python;

use std::path::PathBuf;

use anyhow::Result;

use crate::analysis::{Analysis, AnalysisOptions, analyze};
use crate::git::ShellGit;
use crate::output::{JsonAnalysis, ReportVerbosity, render_report};
use crate::python::PythonAdapter;

pub const DEFAULT_CALLER_PREVIEW_LIMIT: usize = 4;

#[derive(Debug, Clone, Copy)]
pub struct PublicAnalysisOptions {
    pub allow_parse_errors: bool,
    pub report_limit: ReportLimit,
    pub caller_preview_limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportLimit {
    Limited(usize),
    All,
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
    Ok(render_report(
        &analyze_repo(repo, base, head, options)?,
        ReportVerbosity::Normal,
        options.report_limit,
        options.caller_preview_limit,
    ))
}

pub fn analyze_repo_report_verbose(
    repo: impl Into<PathBuf>,
    base: &str,
    head: &str,
    options: PublicAnalysisOptions,
) -> Result<String> {
    Ok(render_report(
        &analyze_repo(repo, base, head, options)?,
        ReportVerbosity::Verbose,
        options.report_limit,
        options.caller_preview_limit,
    ))
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
