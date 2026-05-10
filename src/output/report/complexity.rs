use crate::analysis::facts::{
    ChangeKind, ComplexityDelta, REVIEW_COMPLEXITY_DELTA_THRESHOLD, SymbolChange,
};
use crate::language::ComplexityMetrics;

use super::{ComplexityMetricReport, ComplexityReport};

pub(super) fn complexity_report(change: &SymbolChange) -> Option<ComplexityReport> {
    if let Some(delta) = change.complexity_delta.as_ref() {
        let metrics = changed_complexity_metrics(delta);
        if !metrics.is_empty() {
            return Some(ComplexityReport {
                status: complexity_direction(&metrics),
                metrics,
            });
        }
    }

    if change.kinds.contains(&ChangeKind::Added) {
        return change.after.as_ref().map(|symbol| ComplexityReport {
            status: "new",
            metrics: complexity_snapshot(&symbol.complexity),
        });
    }

    if change.kinds.contains(&ChangeKind::Deleted) {
        return change.before.as_ref().map(|symbol| ComplexityReport {
            status: "deleted",
            metrics: complexity_snapshot(&symbol.complexity),
        });
    }

    None
}

fn changed_complexity_metrics(delta: &ComplexityDelta) -> Vec<ComplexityMetricReport> {
    complexity_metric_values(&delta.before, &delta.after)
        .into_iter()
        .filter_map(|(name, before, after)| {
            ((after as isize - before as isize).abs() >= REVIEW_COMPLEXITY_DELTA_THRESHOLD)
                .then_some(ComplexityMetricReport {
                    name,
                    before: Some(before),
                    after: Some(after),
                })
        })
        .collect()
}

fn complexity_direction(changes: &[ComplexityMetricReport]) -> &'static str {
    let has_increase = changes
        .iter()
        .any(|change| change.after.unwrap_or(0) > change.before.unwrap_or(0));
    let has_decrease = changes
        .iter()
        .any(|change| change.after.unwrap_or(0) < change.before.unwrap_or(0));

    match (has_increase, has_decrease) {
        (true, false) => "increased",
        (false, true) => "decreased",
        _ => "changed",
    }
}

fn complexity_snapshot(metrics: &ComplexityMetrics) -> Vec<ComplexityMetricReport> {
    complexity_metric_values(metrics, metrics)
        .into_iter()
        .filter_map(|(name, value, _)| {
            (value > 0).then_some(ComplexityMetricReport {
                name,
                before: None,
                after: Some(value),
            })
        })
        .collect()
}

fn complexity_metric_values(
    before: &ComplexityMetrics,
    after: &ComplexityMetrics,
) -> [(&'static str, usize, usize); 7] {
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
}
