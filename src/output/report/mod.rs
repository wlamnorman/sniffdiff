mod sections;

use crate::ReportLimit;
use crate::analysis::Analysis;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReportVerbosity {
    Normal,
    Verbose,
}

pub(crate) fn render_report(
    analysis: &Analysis,
    verbosity: ReportVerbosity,
    limit: ReportLimit,
    caller_preview_limit: usize,
) -> String {
    let mut output = Vec::new();
    output.push("Sniffed the diff... 🐽".to_string());

    match verbosity {
        ReportVerbosity::Normal => sections::render(
            analysis,
            verbosity,
            limit,
            caller_preview_limit,
            &mut output,
        ),
        ReportVerbosity::Verbose => sections::render(
            analysis,
            verbosity,
            limit,
            caller_preview_limit,
            &mut output,
        ),
    }

    output.join("\n")
}

pub(super) fn count_label(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

pub(super) fn preview_strings(values: &[String], limit: usize) -> String {
    if values.is_empty() {
        return "none".to_string();
    }

    let mut preview = values.iter().take(limit).cloned().collect::<Vec<_>>();
    if values.len() > limit {
        preview.push(format!("+{} more", values.len() - limit));
    }

    preview.join(", ")
}
