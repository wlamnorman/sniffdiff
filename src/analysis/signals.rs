use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::analysis::facts::{ChangeKind, ReviewSignal, SymbolChange, SymbolVisibility, TestFacts};

pub(crate) fn attach_review_signals(symbol_changes: &mut [SymbolChange], test_facts: &TestFacts) {
    let production_files_without_test_movement = test_facts
        .production_files_without_nearby_test_changes
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();

    for change in &mut *symbol_changes {
        let mut signals = Vec::new();

        if change.symbol_facts.visibility == SymbolVisibility::Publicish
            && change.kinds.contains(&ChangeKind::SignatureChanged)
        {
            signals.push(ReviewSignal::PublicSignatureChanged);
        }

        if change.kinds.contains(&ChangeKind::SignatureChanged)
            && !change.references_after.unchanged_files.is_empty()
        {
            signals.push(ReviewSignal::SignatureChangedWithUnchangedCallers);
        }

        if change
            .complexity_delta
            .as_ref()
            .is_some_and(complexity_increased)
        {
            signals.push(ReviewSignal::ComplexityIncreased);
        }

        if change.kinds.contains(&ChangeKind::BodyChanged)
            && changed_file(change)
                .as_ref()
                .is_some_and(|file| production_files_without_test_movement.contains(file))
        {
            signals.push(ReviewSignal::LogicChangedWithoutTestMovement);
        }

        if change.kinds == vec![ChangeKind::PathChanged] {
            signals.push(ReviewSignal::PathChangedOnly);
        }

        change.review_signals = signals;
    }

    symbol_changes.sort_by(|left, right| {
        signal_weight(right)
            .cmp(&signal_weight(left))
            .then_with(|| left.id.file.cmp(&right.id.file))
            .then_with(|| left.id.qualified_name.cmp(&right.id.qualified_name))
    });
}

fn signal_weight(change: &SymbolChange) -> usize {
    change
        .review_signals
        .iter()
        .map(|signal| match signal {
            ReviewSignal::SignatureChangedWithUnchangedCallers => 50,
            ReviewSignal::PublicSignatureChanged => 30,
            ReviewSignal::ComplexityIncreased => 20,
            ReviewSignal::LogicChangedWithoutTestMovement => 15,
            ReviewSignal::PathChangedOnly => 1,
        })
        .sum()
}

fn complexity_increased(delta: &crate::analysis::facts::ComplexityDelta) -> bool {
    delta.branch_count_delta > 0
        || delta.loop_count_delta > 0
        || delta.boolean_operator_count_delta > 0
        || delta.exception_handler_count_delta > 0
        || delta.match_count_delta > 0
        || delta.with_count_delta > 0
        || delta.max_nesting_depth_delta > 0
}

fn changed_file(change: &SymbolChange) -> Option<PathBuf> {
    change
        .after
        .as_ref()
        .or(change.before.as_ref())
        .map(|symbol| symbol.file.clone())
}

#[cfg(test)]
mod tests {
    use crate::analysis::facts::{
        ComplexityDelta, SignatureChangeFacts, SymbolFacts, SymbolId, SymbolReferenceFacts,
        SymbolVisibility,
    };
    use crate::language::{
        BodyHash, ComplexityMetrics, LineRange, QualifiedName, Signature, Symbol, SymbolKind,
    };

    use super::*;

    #[test]
    fn derives_review_signals_from_raw_facts() {
        let mut changes = vec![change()];
        changes[0].kinds = vec![ChangeKind::BodyChanged, ChangeKind::SignatureChanged];
        changes[0].signature_change = Some(SignatureChangeFacts::default());
        changes[0].complexity_delta = Some(ComplexityDelta {
            before: ComplexityMetrics::default(),
            after: ComplexityMetrics {
                branch_count: 1,
                ..ComplexityMetrics::default()
            },
            length_lines_delta: 0,
            branch_count_delta: 1,
            loop_count_delta: 0,
            boolean_operator_count_delta: 0,
            exception_handler_count_delta: 0,
            match_count_delta: 0,
            with_count_delta: 0,
            max_nesting_depth_delta: 0,
        });
        changes[0].references_after.unchanged_files = vec![PathBuf::from("src/consumer.py")];

        let test_facts = TestFacts {
            production_files_without_nearby_test_changes: vec![PathBuf::from("src/features.py")],
            ..TestFacts::default()
        };

        attach_review_signals(&mut changes, &test_facts);

        assert_eq!(
            changes[0].review_signals,
            vec![
                ReviewSignal::PublicSignatureChanged,
                ReviewSignal::SignatureChangedWithUnchangedCallers,
                ReviewSignal::ComplexityIncreased,
                ReviewSignal::LogicChangedWithoutTestMovement,
            ]
        );
    }

    fn change() -> SymbolChange {
        let symbol = Symbol {
            file: PathBuf::from("src/features.py"),
            qualified_name: QualifiedName::new("build_features"),
            kind: SymbolKind::Function,
            signature: Signature::new("def build_features(rows):"),
            signature_facts: None,
            range: LineRange { start: 1, end: 2 },
            body_hash: BodyHash::new("hash"),
            complexity: ComplexityMetrics::default(),
        };

        SymbolChange {
            id: SymbolId {
                file: PathBuf::from("src/features.py"),
                qualified_name: QualifiedName::new("build_features"),
            },
            kinds: Vec::new(),
            before: Some(symbol.clone()),
            after: Some(symbol),
            symbol_facts: SymbolFacts {
                kind: SymbolKind::Function,
                visibility: SymbolVisibility::Publicish,
            },
            signature_change: None,
            complexity_delta: None,
            references_before: SymbolReferenceFacts::default(),
            references_after: SymbolReferenceFacts::default(),
            test_references_after: SymbolReferenceFacts::default(),
            reference_delta: Default::default(),
            review_signals: Vec::new(),
        }
    }
}
