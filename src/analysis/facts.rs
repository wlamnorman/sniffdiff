use std::path::PathBuf;

use serde::Serialize;

use crate::git::ChangedFile;
use crate::language::{
    ComplexityMetrics, QualifiedName, Reference, ReferenceKind, ReferenceResolution, Symbol,
    SymbolKind,
};

#[derive(Debug, Serialize)]
pub(crate) struct Analysis {
    pub(crate) base: String,
    pub(crate) head: String,
    pub(crate) language: String,
    pub(crate) changed_files: Vec<ChangedFile>,
    pub(crate) before: Snapshot,
    pub(crate) after: Snapshot,
    pub(crate) symbol_changes: Vec<SymbolChange>,
    pub(crate) references_before: Vec<Reference>,
    pub(crate) references_after: Vec<Reference>,
    pub(crate) test_facts: TestFacts,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AnalysisOptions {
    pub(crate) allow_parse_errors: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct Snapshot {
    pub(crate) git_ref: String,
    pub(crate) files_considered: usize,
    pub(crate) files_skipped: usize,
    pub(crate) files_parsed: usize,
    pub(crate) files_with_parse_errors: usize,
    pub(crate) parse_error_files: Vec<PathBuf>,
    pub(crate) symbols: Vec<Symbol>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct SymbolId {
    pub(crate) file: PathBuf,
    pub(crate) qualified_name: QualifiedName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChangeKind {
    Added,
    Deleted,
    PathChanged,
    BodyChanged,
    SignatureChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SymbolChange {
    pub(crate) id: SymbolId,
    pub(crate) kinds: Vec<ChangeKind>,
    pub(crate) before: Option<Symbol>,
    pub(crate) after: Option<Symbol>,
    pub(crate) symbol_facts: SymbolFacts,
    pub(crate) signature_change: Option<SignatureChangeFacts>,
    pub(crate) complexity_delta: Option<ComplexityDelta>,
    pub(crate) references_before: SymbolReferenceFacts,
    pub(crate) references_after: SymbolReferenceFacts,
    pub(crate) test_references_after: SymbolReferenceFacts,
    pub(crate) reference_delta: SymbolReferenceDelta,
    pub(crate) review_signals: Vec<ReviewSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub(crate) struct SymbolReferenceFacts {
    pub(crate) count: usize,
    pub(crate) resolved_count: usize,
    pub(crate) unresolved_count: usize,
    pub(crate) import_count: usize,
    pub(crate) from_import_count: usize,
    pub(crate) direct_call_count: usize,
    pub(crate) attribute_call_count: usize,
    pub(crate) files: Vec<PathBuf>,
    pub(crate) changed_files: Vec<PathBuf>,
    pub(crate) unchanged_files: Vec<PathBuf>,
    pub(crate) matched_references: Vec<MatchedReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MatchedReference {
    pub(crate) file: PathBuf,
    pub(crate) line: usize,
    pub(crate) caller_symbol: Option<QualifiedName>,
    pub(crate) kind: ReferenceKind,
    pub(crate) resolution: ReferenceResolution,
    pub(crate) match_source: ReferenceMatchSource,
    pub(crate) changed_file: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReferenceMatchSource {
    ResolvedImport,
    ResolvedAttribute,
    UnresolvedShortName,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub(crate) struct SymbolReferenceDelta {
    pub(crate) count_delta: isize,
    pub(crate) added_files: Vec<PathBuf>,
    pub(crate) removed_files: Vec<PathBuf>,
    pub(crate) unchanged_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SymbolFacts {
    pub(crate) kind: SymbolKind,
    pub(crate) visibility: SymbolVisibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SymbolVisibility {
    Private,
    Internal,
    Public,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub(crate) struct SignatureChangeFacts {
    pub(crate) parameters_added: Vec<String>,
    pub(crate) parameters_removed: Vec<String>,
    pub(crate) parameters_reordered: bool,
    pub(crate) parameter_kind_changed: Vec<String>,
    pub(crate) parameter_default_changed: Vec<String>,
    pub(crate) parameter_annotation_changed: Vec<String>,
    pub(crate) return_annotation_changed: bool,
    pub(crate) async_changed: bool,
}

impl SignatureChangeFacts {
    pub(crate) fn has_runtime_change(&self) -> bool {
        self.async_changed
            || !self.parameters_added.is_empty()
            || !self.parameters_removed.is_empty()
            || self.parameters_reordered
            || !self.parameter_kind_changed.is_empty()
            || !self.parameter_default_changed.is_empty()
    }

    pub(crate) fn has_type_annotation_change(&self) -> bool {
        !self.parameter_annotation_changed.is_empty() || self.return_annotation_changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComplexityDelta {
    pub(crate) before: ComplexityMetrics,
    pub(crate) after: ComplexityMetrics,
    pub(crate) length_lines_delta: isize,
    pub(crate) branch_count_delta: isize,
    pub(crate) loop_count_delta: isize,
    pub(crate) boolean_operator_count_delta: isize,
    pub(crate) exception_handler_count_delta: isize,
    pub(crate) match_count_delta: isize,
    pub(crate) with_count_delta: isize,
    pub(crate) max_nesting_depth_delta: isize,
}

pub(crate) const REVIEW_COMPLEXITY_DELTA_THRESHOLD: isize = 2;

impl ComplexityDelta {
    pub(crate) fn has_reportable_structural_change(&self) -> bool {
        self.structural_deltas()
            .iter()
            .any(|delta| delta.abs() >= REVIEW_COMPLEXITY_DELTA_THRESHOLD)
    }

    pub(crate) fn has_reportable_structural_increase(&self) -> bool {
        self.structural_deltas()
            .iter()
            .any(|delta| *delta >= REVIEW_COMPLEXITY_DELTA_THRESHOLD)
    }

    fn structural_deltas(&self) -> [isize; 7] {
        [
            self.branch_count_delta,
            self.loop_count_delta,
            self.boolean_operator_count_delta,
            self.exception_handler_count_delta,
            self.match_count_delta,
            self.with_count_delta,
            self.max_nesting_depth_delta,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewSignal {
    PublicSymbolAdded,
    PublicSymbolDeleted,
    PublicSignatureChanged,
    TypeAnnotationsChanged,
    SignatureChangedWithUnchangedCallers,
    ComplexityIncreased,
    ImplementationChangedWithoutTestMovement,
    PathChangedOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub(crate) struct TestFacts {
    pub(crate) changed_test_files: Vec<PathBuf>,
    pub(crate) test_files_parsed: usize,
    pub(crate) test_files_with_parse_errors: usize,
    pub(crate) test_parse_error_files: Vec<PathBuf>,
    pub(crate) production_files_with_nearby_test_changes: Vec<PathBuf>,
    pub(crate) production_files_without_nearby_test_changes: Vec<PathBuf>,
}
