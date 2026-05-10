mod complexity;
mod labels;
mod references;
mod sections;
mod selection;

use crate::ReportLimit;
use crate::analysis::Analysis;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportVerbosity {
    Normal,
    Verbose,
    Full,
}

pub(crate) fn report_model(
    analysis: &Analysis,
    verbosity: ReportVerbosity,
    limit: ReportLimit,
    caller_preview_limit: usize,
) -> Report {
    sections::build_report(analysis, verbosity, limit, caller_preview_limit)
}

pub(crate) fn render_yaml(report: &Report) -> anyhow::Result<String> {
    Ok(serde_yaml::to_string(report)?)
}

pub(crate) fn render_json(report: &Report) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(crate) schema_version: u8,
    pub(crate) verbosity: ReportVerbosity,
    pub(crate) scope: ReportScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parse_errors: Option<ParseErrors>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) inspect: Vec<InspectItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) omitted: Option<Omitted>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReportScope {
    pub(crate) changed_files: usize,
    pub(crate) changed_symbols: usize,
    pub(crate) changed_test_files: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ParseErrors {
    pub(crate) files: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct InspectItem {
    pub(crate) symbol: String,
    pub(crate) changes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) members: Vec<InspectMember>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) signature: Option<SignatureReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) complexity: Option<ComplexityReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_tests: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) unchanged_tests: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tests: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) unchanged_callers: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_callers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) facts: Option<VerboseFacts>,
}

#[derive(Debug, Serialize)]
pub(crate) struct InspectMember {
    pub(crate) name: String,
    pub(crate) changes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) complexity: Option<ComplexityReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) facts: Option<VerboseFacts>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SignatureReport {
    pub(crate) before: String,
    pub(crate) after: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ComplexityReport {
    pub(crate) status: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) metrics: Vec<ComplexityMetricReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ComplexityMetricReport {
    pub(crate) name: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) before: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) after: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct VerboseFacts {
    pub(crate) change_kinds: Vec<String>,
    pub(crate) signals: Vec<String>,
    pub(crate) references: ReferenceCounts,
    pub(crate) test_references_after: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReferenceCounts {
    pub(crate) before: usize,
    pub(crate) after: usize,
    pub(crate) delta: isize,
}

#[derive(Debug, Serialize)]
pub(crate) struct Omitted {
    pub(crate) symbol_changes: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub(crate) high_signal: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub(crate) low_signal: usize,
    pub(crate) hint: String,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}
