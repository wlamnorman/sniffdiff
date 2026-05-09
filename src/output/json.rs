use std::path::PathBuf;

use serde::Serialize;

use crate::analysis::facts::{
    Analysis, ChangeKind, ComplexityDelta, ReviewSignal, SignatureChangeFacts, SymbolReferenceFacts,
};
use crate::git::{ChangedFile, FileStatus};
use crate::language::{
    ComplexityMetrics, FunctionSignatureFacts, LineRange, ReferenceKind, ReferenceResolution,
    SymbolKind,
};

#[derive(Debug, Serialize)]
pub(crate) struct JsonAnalysis {
    schema_version: u8,
    base: String,
    head: String,
    language: String,
    changed_files: Vec<JsonChangedFile>,
    before: JsonSnapshotSummary,
    after: JsonSnapshotSummary,
    symbol_changes: Vec<JsonSymbolChange>,
    references_after: Vec<JsonReference>,
    test_facts: JsonTestFacts,
}

#[derive(Debug, Serialize)]
struct JsonChangedFile {
    path: PathBuf,
    status: JsonFileStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum JsonFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed { old_path: PathBuf },
}

#[derive(Debug, Serialize)]
struct JsonSnapshotSummary {
    git_ref: String,
    files_considered: usize,
    files_skipped: usize,
    files_parsed: usize,
    files_with_parse_errors: usize,
    parse_error_files: Vec<PathBuf>,
    symbol_count: usize,
}

#[derive(Debug, Serialize)]
struct JsonSymbolChange {
    file: PathBuf,
    qualified_name: String,
    kind: SymbolKind,
    change_kinds: Vec<ChangeKind>,
    visibility: crate::analysis::facts::SymbolVisibility,
    before: Option<JsonSymbolSnapshot>,
    after: Option<JsonSymbolSnapshot>,
    signature_change: Option<SignatureChangeFacts>,
    complexity_delta: Option<ComplexityDelta>,
    references_before: SymbolReferenceFacts,
    references_after: SymbolReferenceFacts,
    test_references_after: SymbolReferenceFacts,
    reference_delta: crate::analysis::facts::SymbolReferenceDelta,
    review_signals: Vec<ReviewSignal>,
}

#[derive(Debug, Serialize)]
struct JsonSymbolSnapshot {
    file: PathBuf,
    qualified_name: String,
    kind: SymbolKind,
    signature: String,
    signature_facts: Option<FunctionSignatureFacts>,
    range: LineRange,
    complexity: ComplexityMetrics,
}

#[derive(Debug, Serialize)]
struct JsonReference {
    file: PathBuf,
    name: String,
    module: Option<String>,
    resolved_name: Option<String>,
    resolved_module: Option<String>,
    resolution: ReferenceResolution,
    line: usize,
    kind: ReferenceKind,
}

#[derive(Debug, Serialize)]
struct JsonTestFacts {
    changed_test_files: Vec<PathBuf>,
    changed_test_file_count: usize,
    test_files_parsed: usize,
    test_files_with_parse_errors: usize,
    test_parse_error_files: Vec<PathBuf>,
    production_files_with_nearby_test_changes: Vec<PathBuf>,
    production_files_without_nearby_test_changes: Vec<PathBuf>,
}

impl From<&Analysis> for JsonAnalysis {
    fn from(analysis: &Analysis) -> Self {
        Self {
            schema_version: 1,
            base: analysis.base.clone(),
            head: analysis.head.clone(),
            language: analysis.language.clone(),
            changed_files: analysis
                .changed_files
                .iter()
                .map(JsonChangedFile::from)
                .collect(),
            before: JsonSnapshotSummary {
                git_ref: analysis.before.git_ref.clone(),
                files_considered: analysis.before.files_considered,
                files_skipped: analysis.before.files_skipped,
                files_parsed: analysis.before.files_parsed,
                files_with_parse_errors: analysis.before.files_with_parse_errors,
                parse_error_files: analysis.before.parse_error_files.clone(),
                symbol_count: analysis.before.symbols.len(),
            },
            after: JsonSnapshotSummary {
                git_ref: analysis.after.git_ref.clone(),
                files_considered: analysis.after.files_considered,
                files_skipped: analysis.after.files_skipped,
                files_parsed: analysis.after.files_parsed,
                files_with_parse_errors: analysis.after.files_with_parse_errors,
                parse_error_files: analysis.after.parse_error_files.clone(),
                symbol_count: analysis.after.symbols.len(),
            },
            symbol_changes: analysis
                .symbol_changes
                .iter()
                .map(JsonSymbolChange::from)
                .collect(),
            references_after: analysis
                .references_after
                .iter()
                .map(JsonReference::from)
                .collect(),
            test_facts: JsonTestFacts {
                changed_test_file_count: analysis.test_facts.changed_test_files.len(),
                changed_test_files: analysis.test_facts.changed_test_files.clone(),
                test_files_parsed: analysis.test_facts.test_files_parsed,
                test_files_with_parse_errors: analysis.test_facts.test_files_with_parse_errors,
                test_parse_error_files: analysis.test_facts.test_parse_error_files.clone(),
                production_files_with_nearby_test_changes: analysis
                    .test_facts
                    .production_files_with_nearby_test_changes
                    .clone(),
                production_files_without_nearby_test_changes: analysis
                    .test_facts
                    .production_files_without_nearby_test_changes
                    .clone(),
            },
        }
    }
}

impl From<&ChangedFile> for JsonChangedFile {
    fn from(file: &ChangedFile) -> Self {
        Self {
            path: file.path.clone(),
            status: match &file.status {
                FileStatus::Added => JsonFileStatus::Added,
                FileStatus::Modified => JsonFileStatus::Modified,
                FileStatus::Deleted => JsonFileStatus::Deleted,
                FileStatus::Renamed { old_path } => JsonFileStatus::Renamed {
                    old_path: old_path.clone(),
                },
            },
        }
    }
}

impl From<&crate::analysis::facts::SymbolChange> for JsonSymbolChange {
    fn from(change: &crate::analysis::facts::SymbolChange) -> Self {
        Self {
            file: change.id.file.clone(),
            qualified_name: change.id.qualified_name.to_string(),
            kind: change.symbol_facts.kind,
            change_kinds: change.kinds.clone(),
            visibility: change.symbol_facts.visibility,
            before: change.before.as_ref().map(JsonSymbolSnapshot::from),
            after: change.after.as_ref().map(JsonSymbolSnapshot::from),
            signature_change: change.signature_change.clone(),
            complexity_delta: change.complexity_delta.clone(),
            references_before: change.references_before.clone(),
            references_after: change.references_after.clone(),
            test_references_after: change.test_references_after.clone(),
            reference_delta: change.reference_delta.clone(),
            review_signals: change.review_signals.clone(),
        }
    }
}

impl From<&crate::language::Symbol> for JsonSymbolSnapshot {
    fn from(symbol: &crate::language::Symbol) -> Self {
        Self {
            file: symbol.file.clone(),
            qualified_name: symbol.qualified_name.to_string(),
            kind: symbol.kind,
            signature: symbol.signature.to_string(),
            signature_facts: symbol.signature_facts.clone(),
            range: symbol.range.clone(),
            complexity: symbol.complexity.clone(),
        }
    }
}

impl From<&crate::language::Reference> for JsonReference {
    fn from(reference: &crate::language::Reference) -> Self {
        Self {
            file: reference.file.clone(),
            name: reference.name.to_string(),
            module: reference.module.as_ref().map(ToString::to_string),
            resolved_name: reference.resolved_name.as_ref().map(ToString::to_string),
            resolved_module: reference.resolved_module.as_ref().map(ToString::to_string),
            resolution: reference.resolution,
            line: reference.line,
            kind: reference.kind,
        }
    }
}
