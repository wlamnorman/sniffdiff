use std::collections::BTreeMap;
use std::path::Path;

use crate::analysis::facts::{ReviewSignal, SymbolChange, SymbolReferenceFacts};
use crate::language::{QualifiedName, ReferenceKind};

use super::ReportVerbosity;

pub(super) fn test_references(
    change: &SymbolChange,
    verbosity: ReportVerbosity,
    caller_preview_limit: usize,
) -> (Vec<String>, Vec<String>, Option<&'static str>) {
    let changed_tests = reference_labels(
        &change.test_references_after,
        true,
        verbosity,
        caller_preview_limit,
    );
    let unchanged_tests = reference_labels(
        &change.test_references_after,
        false,
        verbosity,
        caller_preview_limit,
    );
    let tests = (changed_tests.is_empty()
        && unchanged_tests.is_empty()
        && change
            .review_signals
            .contains(&ReviewSignal::ImplementationChangedWithoutTestMovement))
    .then_some("no direct test references found");

    (changed_tests, unchanged_tests, tests)
}

pub(super) fn caller_labels(
    change: &SymbolChange,
    changed_file: bool,
    verbosity: ReportVerbosity,
    caller_preview_limit: usize,
) -> Vec<String> {
    reference_labels(
        &change.references_after,
        changed_file,
        verbosity,
        caller_preview_limit,
    )
}

fn reference_labels(
    references: &SymbolReferenceFacts,
    changed_file: bool,
    verbosity: ReportVerbosity,
    caller_preview_limit: usize,
) -> Vec<String> {
    let mut callers: BTreeMap<String, usize> = BTreeMap::new();

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
        let caller = caller_label(
            &reference.file,
            reference.caller_symbol.as_ref(),
            reference.line,
        );
        *callers.entry(caller).or_default() += 1;
    }

    let limit = match verbosity {
        ReportVerbosity::Full => usize::MAX,
        ReportVerbosity::Normal | ReportVerbosity::Verbose => caller_preview_limit,
    };
    let mut labels = callers
        .into_iter()
        .map(|(caller, count)| caller_count_label(caller, count))
        .collect::<Vec<_>>();

    let total_labels = labels.len();
    if total_labels > limit {
        labels.truncate(limit);
        labels.push(format!("+{} more", total_labels - limit));
    }

    labels
}

fn caller_count_label(caller: String, count: usize) -> String {
    if count == 1 {
        caller
    } else {
        format!("{caller} ({count} callsites)")
    }
}

fn caller_label(file: &Path, caller_symbol: Option<&QualifiedName>, line: usize) -> String {
    caller_symbol.map_or_else(
        || format!("{}:{line}", file.display()),
        |symbol| format!("{}::{symbol}", file.display()),
    )
}
