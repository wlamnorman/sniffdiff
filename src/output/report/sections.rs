use std::collections::BTreeSet;

use crate::ReportLimit;
use crate::analysis::Analysis;
use crate::analysis::facts::{ChangeKind, SymbolChange};
use crate::language::{QualifiedName, SymbolKind};

use super::complexity::complexity_report;
use super::labels::{change_labels, signature_report, verbose_facts};
use super::references::{caller_labels, test_references};
use super::selection::{has_inspect_signal, inspect_selection, omitted};
use super::{InspectItem, InspectMember, ParseErrors, Report, ReportScope, ReportVerbosity};

pub(super) fn build_report(
    analysis: &Analysis,
    verbosity: ReportVerbosity,
    limit: ReportLimit,
    caller_preview_limit: usize,
) -> Report {
    let inspect = inspect_selection(analysis, verbosity, limit);
    let parse_error_count = parse_error_count(analysis);
    let grouped_inspect =
        grouped_inspect_items(analysis, &inspect.changes, verbosity, caller_preview_limit);

    Report {
        schema_version: 1,
        verbosity,
        scope: ReportScope {
            changed_files: analysis.changed_files.len(),
            changed_symbols: analysis.symbol_changes.len(),
            changed_test_files: analysis.test_facts.changed_test_files.len(),
        },
        parse_errors: (parse_error_count > 0).then_some(ParseErrors {
            files: parse_error_count,
        }),
        note: analysis
            .symbol_changes
            .is_empty()
            .then_some("No Python file changes inside sniffdiff's symbol model were detected."),
        inspect: grouped_inspect.items,
        omitted: omitted(
            analysis.symbol_changes.len(),
            inspect.total_signal_bearing,
            grouped_inspect.displayed_changes,
            grouped_inspect.displayed_signal_bearing,
        ),
    }
}

struct GroupedInspect {
    items: Vec<InspectItem>,
    displayed_changes: usize,
    displayed_signal_bearing: usize,
}

fn grouped_inspect_items(
    analysis: &Analysis,
    selected_changes: &[&SymbolChange],
    verbosity: ReportVerbosity,
    caller_preview_limit: usize,
) -> GroupedInspect {
    if verbosity == ReportVerbosity::Full {
        return GroupedInspect {
            items: selected_changes
                .iter()
                .map(|change| inspect_item(change, Vec::new(), verbosity, caller_preview_limit))
                .collect(),
            displayed_changes: selected_changes.len(),
            displayed_signal_bearing: selected_changes
                .iter()
                .filter(|change| has_inspect_signal(change))
                .count(),
        };
    }

    let grouped_members = grouped_member_changes(analysis, selected_changes);
    let grouped_member_keys = grouped_members
        .iter()
        .map(|member| change_key(member))
        .collect::<BTreeSet<_>>();

    let mut displayed_keys = BTreeSet::new();
    let mut displayed_signal_bearing = 0;
    let mut items = Vec::new();

    for change in selected_changes {
        if grouped_member_keys.contains(&change_key(change)) {
            continue;
        }

        if displayed_keys.insert(change_key(change)) && has_inspect_signal(change) {
            displayed_signal_bearing += 1;
        }

        let members = grouped_members
            .iter()
            .copied()
            .filter(|member| is_grouped_member(change, member))
            .inspect(|member| {
                if displayed_keys.insert(change_key(member)) && has_inspect_signal(member) {
                    displayed_signal_bearing += 1;
                }
            })
            .map(|member| inspect_member(change, member, verbosity))
            .collect();

        items.push(inspect_item(
            change,
            members,
            verbosity,
            caller_preview_limit,
        ));
    }

    GroupedInspect {
        items,
        displayed_changes: displayed_keys.len(),
        displayed_signal_bearing,
    }
}

fn grouped_member_changes<'a>(
    analysis: &'a Analysis,
    selected_changes: &[&'a SymbolChange],
) -> Vec<&'a SymbolChange> {
    let selected_grouping_classes = selected_changes
        .iter()
        .copied()
        .filter(|change| is_grouping_class(change))
        .collect::<Vec<_>>();

    if selected_grouping_classes.is_empty() {
        return Vec::new();
    }

    analysis
        .symbol_changes
        .iter()
        .filter(|candidate| {
            selected_grouping_classes
                .iter()
                .any(|parent| is_grouped_member(parent, candidate))
        })
        .collect()
}

fn is_grouping_class(change: &SymbolChange) -> bool {
    change.symbol_facts.kind == SymbolKind::Class
        && (change.kinds.contains(&ChangeKind::Added)
            || change.kinds.contains(&ChangeKind::Deleted))
}

fn is_grouped_member(parent: &SymbolChange, candidate: &SymbolChange) -> bool {
    if parent.id.file != candidate.id.file || candidate.symbol_facts.kind != SymbolKind::Method {
        return false;
    }

    let expected_kind = if parent.kinds.contains(&ChangeKind::Added) {
        ChangeKind::Added
    } else if parent.kinds.contains(&ChangeKind::Deleted) {
        ChangeKind::Deleted
    } else {
        return false;
    };

    candidate.kinds.contains(&expected_kind)
        && child_name(&parent.id.qualified_name, &candidate.id.qualified_name).is_some()
}

fn child_name(parent: &QualifiedName, candidate: &QualifiedName) -> Option<String> {
    candidate
        .as_str()
        .strip_prefix(parent.as_str())?
        .strip_prefix('.')
        .map(ToOwned::to_owned)
}

fn change_key(change: &SymbolChange) -> String {
    format!("{}::{}", change.id.file.display(), change.id.qualified_name)
}

fn inspect_item(
    change: &SymbolChange,
    members: Vec<InspectMember>,
    verbosity: ReportVerbosity,
    caller_preview_limit: usize,
) -> InspectItem {
    let (changed_tests, unchanged_tests, tests) =
        test_references(change, verbosity, caller_preview_limit);

    InspectItem {
        symbol: format!("{}::{}", change.id.file.display(), change.id.qualified_name),
        changes: change_labels(change),
        members,
        signature: signature_report(change),
        complexity: complexity_report(change),
        changed_tests,
        unchanged_tests,
        tests,
        unchanged_callers: caller_labels(change, false, verbosity, caller_preview_limit),
        changed_callers: caller_labels(change, true, verbosity, caller_preview_limit),
        facts: (verbosity != ReportVerbosity::Normal).then(|| verbose_facts(change)),
    }
}

fn inspect_member(
    parent: &SymbolChange,
    member: &SymbolChange,
    verbosity: ReportVerbosity,
) -> InspectMember {
    InspectMember {
        name: child_name(&parent.id.qualified_name, &member.id.qualified_name)
            .unwrap_or_else(|| member.id.qualified_name.short_name().to_string()),
        changes: change_labels(member),
        complexity: complexity_report(member),
        facts: (verbosity != ReportVerbosity::Normal).then(|| verbose_facts(member)),
    }
}

fn parse_error_count(analysis: &Analysis) -> usize {
    analysis.before.files_with_parse_errors
        + analysis.after.files_with_parse_errors
        + analysis.test_facts.test_files_with_parse_errors
}
