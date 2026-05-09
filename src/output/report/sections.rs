use std::path::Path;

use crate::ReportLimit;
use crate::analysis::Analysis;
use crate::analysis::facts::{
    ChangeKind, ComplexityDelta, ReviewSignal, SymbolChange, SymbolReferenceFacts,
};
use crate::language::{ComplexityMetrics, QualifiedName, ReferenceKind};

use super::{ReportVerbosity, count_label, preview_strings};

pub(super) fn render(
    analysis: &Analysis,
    verbosity: ReportVerbosity,
    limit: ReportLimit,
    caller_preview_limit: usize,
    output: &mut Vec<String>,
) {
    let inspect = inspect_selection(analysis, limit);
    let parse_error_count = parse_error_count(analysis);

    output.push(format!(
        "scope: {}, {}, {}",
        count_label(
            analysis.changed_files.len(),
            "changed file",
            "changed files"
        ),
        count_label(
            analysis.symbol_changes.len(),
            "changed symbol",
            "changed symbols"
        ),
        count_label(
            analysis.test_facts.changed_test_files.len(),
            "changed test file",
            "changed test files"
        )
    ));
    if parse_error_count > 0 {
        output.push(format!("parse_errors: {parse_error_count} files"));
    }

    output.push(String::new());
    render_inspect(&inspect.changes, verbosity, caller_preview_limit, output);

    if let Some(line) = omitted_line(analysis.symbol_changes.len(), &inspect, limit) {
        output.push(String::new());
        output.push(line);
    }
}

fn render_inspect(
    inspect: &[&SymbolChange],
    verbosity: ReportVerbosity,
    caller_preview_limit: usize,
    output: &mut Vec<String>,
) {
    if inspect.is_empty() {
        output.push("- none".to_string());
    }
    for change in inspect {
        render_review_target(change, verbosity, caller_preview_limit, output);
    }
}

fn render_review_target(
    change: &SymbolChange,
    verbosity: ReportVerbosity,
    caller_preview_limit: usize,
    output: &mut Vec<String>,
) {
    output.push(format!(
        "- {}::{}",
        change.id.file.display(),
        change.id.qualified_name
    ));
    for reason in review_reason_lines(change) {
        output.push(format!("  {reason}"));
    }
    if let Some(signature) = signature_line(change) {
        output.push(format!("  signature: {signature}"));
    }
    if let Some(complexity) = complexity_line(change) {
        output.push(format!("  complexity: {complexity}"));
    }
    for test_line in test_reference_lines(change, caller_preview_limit) {
        output.push(format!("  {test_line}"));
    }
    render_callers(
        "unchanged_callers",
        &caller_labels(change, false),
        caller_preview_limit,
        output,
    );
    render_callers(
        "changed_callers",
        &caller_labels(change, true),
        caller_preview_limit,
        output,
    );
    if verbosity == ReportVerbosity::Verbose {
        render_verbose_symbol_details(change, output);
    }
}

fn render_callers(
    label: &str,
    callers: &[String],
    caller_preview_limit: usize,
    output: &mut Vec<String>,
) {
    if !callers.is_empty() {
        output.push(format!(
            "  {label}: {}",
            preview_strings(callers, caller_preview_limit)
        ));
    }
}

fn render_verbose_symbol_details(change: &SymbolChange, output: &mut Vec<String>) {
    output.push(format!("  change_kinds: {}", change_kinds_text(change)));
    output.push(format!("  signals: {}", review_signals_text(change)));
    output.push(format!(
        "  references: before {}, after {}, delta {}",
        change.references_before.count,
        change.references_after.count,
        signed_delta(change.reference_delta.count_delta)
    ));
    output.push(format!(
        "  test_references: after {}",
        change.test_references_after.count
    ));
}

fn has_inspect_signal(change: &SymbolChange) -> bool {
    change
        .review_signals
        .iter()
        .any(|signal| *signal != ReviewSignal::PathChangedOnly)
}

#[derive(Debug)]
struct InspectSelection<'a> {
    changes: Vec<&'a SymbolChange>,
    total_signal_bearing: usize,
}

fn inspect_selection(analysis: &Analysis, limit: ReportLimit) -> InspectSelection<'_> {
    let signal_bearing = analysis
        .symbol_changes
        .iter()
        .filter(|change| has_inspect_signal(change))
        .collect::<Vec<_>>();

    let changes = match limit {
        ReportLimit::Limited(limit) => signal_bearing.iter().take(limit).copied().collect(),
        ReportLimit::All => signal_bearing.clone(),
    };

    InspectSelection {
        changes,
        total_signal_bearing: signal_bearing.len(),
    }
}

fn omitted_line(
    total_changes: usize,
    inspect: &InspectSelection<'_>,
    limit: ReportLimit,
) -> Option<String> {
    let shown = inspect.changes.len();
    let hidden_signal = inspect.total_signal_bearing.saturating_sub(shown);
    let low_signal = total_changes.saturating_sub(inspect.total_signal_bearing);
    let omitted = hidden_signal + low_signal;

    if omitted == 0 {
        return None;
    }

    match (hidden_signal, low_signal, limit) {
        (0, _, _) => Some(format!(
            "omitted: {omitted} path-only or low-signal symbol changes; use --json for exhaustive facts"
        )),
        (_, 0, _) => Some(format!(
            "omitted: {omitted} symbol changes; use --limit {} for more items",
            inspect.total_signal_bearing
        )),
        (_, _, _) => Some(format!(
            "omitted: {omitted} symbol changes; use --limit {} for {hidden_signal} more items, --json for {low_signal} low-signal facts",
            inspect.total_signal_bearing
        )),
    }
}

fn parse_error_count(analysis: &Analysis) -> usize {
    analysis.before.files_with_parse_errors
        + analysis.after.files_with_parse_errors
        + analysis.test_facts.test_files_with_parse_errors
}

fn review_reason_lines(change: &SymbolChange) -> Vec<String> {
    let mut changes = Vec::new();

    if change
        .review_signals
        .contains(&ReviewSignal::PublicSignatureChanged)
    {
        changes.push("public signature changed");
    }

    if change.kinds.contains(&ChangeKind::BodyChanged) {
        changes.push("logic changed");
    }

    if change
        .review_signals
        .contains(&ReviewSignal::PathChangedOnly)
    {
        changes.push("path changed only");
    }

    if changes.is_empty() {
        vec!["change: no review signals".to_string()]
    } else {
        vec![format!("change: {}", changes.join("; "))]
    }
}

fn signature_line(change: &SymbolChange) -> Option<String> {
    if !change.kinds.contains(&ChangeKind::SignatureChanged) {
        return None;
    }

    let before = change.before.as_ref()?;
    let after = change.after.as_ref()?;
    let async_change = change.signature_change.as_ref().and_then(async_change_text);

    let signatures = format!(
        "{} -> {}",
        clean_signature(&before.signature.to_string()),
        clean_signature(&after.signature.to_string())
    );
    async_change
        .map(|change| format!("{change}; {signatures}"))
        .or(Some(signatures))
}

fn async_change_text(
    signature_change: &crate::analysis::facts::SignatureChangeFacts,
) -> Option<&'static str> {
    if signature_change.async_changed {
        Some("async changed")
    } else {
        None
    }
}

fn clean_signature(signature: &str) -> String {
    let signature = signature.trim_end_matches(':');
    signature
        .strip_prefix("async def ")
        .map(|signature| format!("async {signature}"))
        .or_else(|| signature.strip_prefix("def ").map(ToOwned::to_owned))
        .unwrap_or_else(|| signature.to_string())
}

fn has_no_test_movement(change: &SymbolChange) -> bool {
    change
        .review_signals
        .contains(&ReviewSignal::LogicChangedWithoutTestMovement)
}

fn complexity_line(change: &SymbolChange) -> Option<String> {
    if let Some(delta) = change.complexity_delta.as_ref() {
        let changed_metrics = changed_complexity_metrics(delta);
        if !changed_metrics.is_empty() {
            return Some(format!(
                "{}; {}",
                complexity_direction(&changed_metrics),
                changed_metrics
                    .iter()
                    .map(|metric| format!("{} {} -> {}", metric.name, metric.before, metric.after))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }

    if change.kinds.contains(&ChangeKind::Added) {
        return change
            .after
            .as_ref()
            .map(|symbol| format!("new; {}", complexity_snapshot(&symbol.complexity)));
    }

    if change.kinds.contains(&ChangeKind::Deleted) {
        return change
            .before
            .as_ref()
            .map(|symbol| format!("deleted; {}", complexity_snapshot(&symbol.complexity)));
    }

    None
}

#[derive(Debug, Clone, Copy)]
struct ComplexityMetricChange {
    name: &'static str,
    before: usize,
    after: usize,
}

fn changed_complexity_metrics(delta: &ComplexityDelta) -> Vec<ComplexityMetricChange> {
    let before = &delta.before;
    let after = &delta.after;
    [
        ("branches", before.branch_count, after.branch_count),
        ("loops", before.loop_count, after.loop_count),
        (
            "bool_ops",
            before.boolean_operator_count,
            after.boolean_operator_count,
        ),
        (
            "exceptions",
            before.exception_handler_count,
            after.exception_handler_count,
        ),
        ("match", before.match_count, after.match_count),
        ("with", before.with_count, after.with_count),
        ("nesting", before.max_nesting_depth, after.max_nesting_depth),
    ]
    .into_iter()
    .filter_map(|(name, before, after)| {
        (before != after).then_some(ComplexityMetricChange {
            name,
            before,
            after,
        })
    })
    .collect()
}

fn complexity_direction(changes: &[ComplexityMetricChange]) -> &'static str {
    let has_increase = changes.iter().any(|change| change.after > change.before);
    let has_decrease = changes.iter().any(|change| change.after < change.before);

    match (has_increase, has_decrease) {
        (true, false) => "increased",
        (false, true) => "decreased",
        _ => "changed",
    }
}

fn complexity_snapshot(metrics: &ComplexityMetrics) -> String {
    let parts = [
        ("branches", metrics.branch_count),
        ("loops", metrics.loop_count),
        ("bool_ops", metrics.boolean_operator_count),
        ("exceptions", metrics.exception_handler_count),
        ("match", metrics.match_count),
        ("with", metrics.with_count),
        ("nesting", metrics.max_nesting_depth),
    ]
    .into_iter()
    .filter_map(|(name, value)| (value > 0).then_some(format!("{name} {value}")))
    .collect::<Vec<_>>();

    if parts.is_empty() {
        "flat".to_string()
    } else {
        parts.join("; ")
    }
}

fn caller_labels(change: &SymbolChange, changed_file: bool) -> Vec<String> {
    reference_labels(&change.references_after, changed_file)
}

fn test_reference_lines(change: &SymbolChange, caller_preview_limit: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let changed_tests = reference_labels(&change.test_references_after, true);
    let unchanged_tests = reference_labels(&change.test_references_after, false);

    if !changed_tests.is_empty() {
        lines.push(format!(
            "changed_tests: {}",
            preview_strings(&changed_tests, caller_preview_limit)
        ));
    }

    if !unchanged_tests.is_empty() {
        lines.push(format!(
            "unchanged_tests: {}",
            preview_strings(&unchanged_tests, caller_preview_limit)
        ));
    }

    if changed_tests.is_empty() && has_no_test_movement(change) {
        lines.push("tests: no nearby test movement".to_string());
    }

    lines
}

fn reference_labels(references: &SymbolReferenceFacts, changed_file: bool) -> Vec<String> {
    let mut callers: Vec<(String, usize)> = Vec::new();

    for reference in &references.matched_references {
        if reference.changed_file != changed_file {
            continue;
        }
        if !matches!(
            reference.kind,
            ReferenceKind::Call | ReferenceKind::Attribute
        ) {
            continue;
        }
        push_or_increment(
            &mut callers,
            caller_label(
                &reference.file,
                reference.caller_symbol.as_ref(),
                reference.line,
            ),
        );
    }

    callers
        .into_iter()
        .map(|(caller, count)| {
            format!("{caller} ({})", count_label(count, "callsite", "callsites"))
        })
        .collect()
}

fn push_or_increment(values: &mut Vec<(String, usize)>, value: String) {
    if let Some((_, count)) = values.iter_mut().find(|(existing, _)| existing == &value) {
        *count += 1;
        return;
    }

    values.push((value, 1));
}

fn caller_label(file: &Path, caller_symbol: Option<&QualifiedName>, line: usize) -> String {
    caller_symbol.map_or_else(
        || format!("{}:{line}", file.display()),
        |symbol| format!("{}::{symbol}", file.display()),
    )
}

fn change_kinds_text(change: &SymbolChange) -> String {
    change
        .kinds
        .iter()
        .map(change_kind_text)
        .collect::<Vec<_>>()
        .join(", ")
}

fn change_kind_text(kind: &ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Deleted => "deleted",
        ChangeKind::PathChanged => "path changed",
        ChangeKind::BodyChanged => "body changed",
        ChangeKind::SignatureChanged => "signature changed",
    }
}

fn review_signals_text(change: &SymbolChange) -> String {
    if change.review_signals.is_empty() {
        return "none".to_string();
    }

    change
        .review_signals
        .iter()
        .map(|signal| match signal {
            ReviewSignal::PublicSignatureChanged => "public signature changed",
            ReviewSignal::SignatureChangedWithUnchangedCallers => {
                "signature changed with unchanged callers"
            }
            ReviewSignal::ComplexityIncreased => "complexity increased",
            ReviewSignal::LogicChangedWithoutTestMovement => "logic changed without test movement",
            ReviewSignal::PathChangedOnly => "path changed only",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn signed_delta(delta: isize) -> String {
    if delta > 0 {
        format!("+{delta}")
    } else {
        delta.to_string()
    }
}
