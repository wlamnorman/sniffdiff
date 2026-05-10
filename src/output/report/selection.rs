use crate::ReportLimit;
use crate::analysis::Analysis;
use crate::analysis::facts::{ReviewSignal, SymbolChange};

use super::{Omitted, ReportVerbosity};

#[derive(Debug)]
pub(super) struct InspectSelection<'a> {
    pub(super) changes: Vec<&'a SymbolChange>,
    pub(super) total_signal_bearing: usize,
}

pub(super) fn inspect_selection(
    analysis: &Analysis,
    verbosity: ReportVerbosity,
    limit: ReportLimit,
) -> InspectSelection<'_> {
    let signal_bearing = analysis
        .symbol_changes
        .iter()
        .filter(|change| has_inspect_signal(change))
        .collect::<Vec<_>>();

    let changes = match verbosity {
        ReportVerbosity::Full => analysis.symbol_changes.iter().collect(),
        ReportVerbosity::Normal | ReportVerbosity::Verbose => match limit {
            ReportLimit::Limited(limit) => signal_bearing.iter().take(limit).copied().collect(),
            ReportLimit::All => signal_bearing.clone(),
        },
    };

    InspectSelection {
        changes,
        total_signal_bearing: signal_bearing.len(),
    }
}

pub(super) fn omitted(
    total_changes: usize,
    total_signal_bearing: usize,
    displayed_changes: usize,
    displayed_signal_bearing: usize,
) -> Option<Omitted> {
    if displayed_changes >= total_changes {
        return None;
    }

    let hidden_signal = total_signal_bearing.saturating_sub(displayed_signal_bearing);
    let total_low_signal = total_changes.saturating_sub(total_signal_bearing);
    let displayed_low_signal = displayed_changes.saturating_sub(displayed_signal_bearing);
    let low_signal = total_low_signal.saturating_sub(displayed_low_signal);
    let omitted = hidden_signal + low_signal;

    if omitted == 0 {
        return None;
    }

    let hint = match (hidden_signal, low_signal) {
        (0, _) => "use --format json --verbosity full for full details".to_string(),
        (_, 0) => format!(
            "use --limit {} to show all high-signal items",
            total_signal_bearing
        ),
        (_, _) => format!(
            "use --limit {} to show all high-signal items, --verbosity full for {low_signal} low-signal facts",
            total_signal_bearing
        ),
    };

    Some(Omitted {
        symbol_changes: omitted,
        high_signal: hidden_signal,
        low_signal,
        hint,
    })
}

pub(super) fn has_inspect_signal(change: &SymbolChange) -> bool {
    change
        .review_signals
        .iter()
        .any(|signal| *signal != ReviewSignal::PathChangedOnly)
}
