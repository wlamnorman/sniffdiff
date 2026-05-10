use crate::analysis::facts::{
    ChangeKind, ComplexityDelta, ReviewSignal, SignatureChangeFacts, SymbolChange, SymbolVisibility,
};
use crate::language::{FunctionSignatureFacts, ParameterFacts};

use super::{ReferenceCounts, SignatureReport, VerboseFacts};

pub(super) fn change_labels(change: &SymbolChange) -> Vec<String> {
    let mut changes = Vec::new();

    if change.kinds.contains(&ChangeKind::Added) {
        changes.push(format!(
            "added {} {}",
            visibility_label(change.symbol_facts.visibility),
            kind_label(change)
        ));
    }

    if change.kinds.contains(&ChangeKind::Deleted) {
        changes.push(format!(
            "deleted {} {}",
            visibility_label(change.symbol_facts.visibility),
            kind_label(change)
        ));
    }

    if change.kinds.contains(&ChangeKind::SignatureChanged) {
        if let Some(signature_change) = change.signature_change.as_ref() {
            if signature_change.has_runtime_change() {
                changes.push(format!(
                    "{} signature",
                    visibility_label(change.symbol_facts.visibility)
                ));
            }
            if signature_change.has_type_annotation_change() {
                changes.push(type_annotation_label(change, signature_change));
            }
        } else {
            changes.push(format!(
                "{} signature",
                visibility_label(change.symbol_facts.visibility)
            ));
        }
    }

    if should_report_implementation_change(change) {
        changes.push("implementation".to_string());
    }

    if change
        .review_signals
        .contains(&ReviewSignal::PathChangedOnly)
    {
        changes.push("path changed only".to_string());
    }

    if changes.is_empty() {
        vec!["no review signals".to_string()]
    } else {
        changes
    }
}

pub(super) fn signature_report(change: &SymbolChange) -> Option<SignatureReport> {
    if !change.kinds.contains(&ChangeKind::SignatureChanged) {
        return None;
    }

    Some(SignatureReport {
        before: clean_signature(&change.before.as_ref()?.signature.to_string()),
        after: clean_signature(&change.after.as_ref()?.signature.to_string()),
    })
}

pub(super) fn verbose_facts(change: &SymbolChange) -> VerboseFacts {
    VerboseFacts {
        change_kinds: change.kinds.iter().map(change_kind_text).collect(),
        signals: change
            .review_signals
            .iter()
            .map(review_signal_text)
            .collect(),
        references: ReferenceCounts {
            before: change.references_before.count,
            after: change.references_after.count,
            delta: change.reference_delta.count_delta,
        },
        test_references_after: change.test_references_after.count,
    }
}

fn should_report_implementation_change(change: &SymbolChange) -> bool {
    if !change.kinds.contains(&ChangeKind::BodyChanged) {
        return false;
    }

    !change.kinds.contains(&ChangeKind::SignatureChanged)
        || change
            .complexity_delta
            .as_ref()
            .is_some_and(ComplexityDelta::has_reportable_structural_change)
}

fn visibility_label(visibility: SymbolVisibility) -> &'static str {
    match visibility {
        SymbolVisibility::Private => "private",
        SymbolVisibility::Internal => "internal",
        SymbolVisibility::Public => "public",
    }
}

fn kind_label(change: &SymbolChange) -> &'static str {
    match change.symbol_facts.kind {
        crate::language::SymbolKind::Class => "class",
        crate::language::SymbolKind::Function => "function",
        crate::language::SymbolKind::Method => "method",
    }
}

fn type_annotation_label(change: &SymbolChange, signature_change: &SignatureChangeFacts) -> String {
    let deltas = annotation_deltas(change, signature_change);
    if deltas.is_empty() {
        "type annotations".to_string()
    } else if deltas.len() == 1 {
        format!("type annotation ({})", deltas[0])
    } else {
        format!("type annotations ({})", deltas.join("; "))
    }
}

fn annotation_deltas(
    change: &SymbolChange,
    signature_change: &SignatureChangeFacts,
) -> Vec<String> {
    let Some(before) = change
        .before
        .as_ref()
        .and_then(|symbol| symbol.signature_facts.as_ref())
    else {
        return Vec::new();
    };
    let Some(after) = change
        .after
        .as_ref()
        .and_then(|symbol| symbol.signature_facts.as_ref())
    else {
        return Vec::new();
    };

    let mut deltas = signature_change
        .parameter_annotation_changed
        .iter()
        .filter_map(|name| parameter_annotation_delta(before, after, name))
        .collect::<Vec<_>>();

    if signature_change.return_annotation_changed {
        deltas.push(format!(
            "return: {} -> {}",
            annotation_text(before.return_annotation.as_deref()),
            annotation_text(after.return_annotation.as_deref())
        ));
    }

    deltas
}

fn parameter_annotation_delta(
    before: &FunctionSignatureFacts,
    after: &FunctionSignatureFacts,
    name: &str,
) -> Option<String> {
    let before_parameter = parameter_by_name(before, name)?;
    let after_parameter = parameter_by_name(after, name)?;
    Some(format!(
        "{}: {} -> {}",
        name,
        annotation_text(before_parameter.annotation.as_deref()),
        annotation_text(after_parameter.annotation.as_deref())
    ))
}

fn parameter_by_name<'a>(
    signature: &'a FunctionSignatureFacts,
    name: &str,
) -> Option<&'a ParameterFacts> {
    signature
        .parameters
        .iter()
        .find(|parameter| parameter.name == name)
}

fn annotation_text(annotation: Option<&str>) -> &str {
    annotation.unwrap_or("unannotated")
}

fn clean_signature(signature: &str) -> String {
    let signature = signature.trim().trim_end_matches(':').trim();
    let signature = signature
        .strip_prefix("async def ")
        .map(|signature| format!("async {signature}"))
        .or_else(|| signature.strip_prefix("def ").map(ToOwned::to_owned))
        .unwrap_or_else(|| signature.to_string());

    signature
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("( ", "(")
        .replace(", )", ")")
        .replace(",)", ")")
        .replace(" )", ")")
        .replace(" ,", ",")
}

fn change_kind_text(kind: &ChangeKind) -> String {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Deleted => "deleted",
        ChangeKind::PathChanged => "path changed",
        ChangeKind::BodyChanged => "body changed",
        ChangeKind::SignatureChanged => "signature changed",
    }
    .to_string()
}

fn review_signal_text(signal: &ReviewSignal) -> String {
    match signal {
        ReviewSignal::PublicSymbolAdded => "public symbol added",
        ReviewSignal::PublicSymbolDeleted => "public symbol deleted",
        ReviewSignal::PublicSignatureChanged => "public signature changed",
        ReviewSignal::TypeAnnotationsChanged => "type annotations changed",
        ReviewSignal::SignatureChangedWithUnchangedCallers => {
            "signature changed with unchanged callers"
        }
        ReviewSignal::ComplexityIncreased => "complexity increased",
        ReviewSignal::ImplementationChangedWithoutTestMovement => {
            "implementation changed without test movement"
        }
        ReviewSignal::PathChangedOnly => "path changed only",
    }
    .to_string()
}
