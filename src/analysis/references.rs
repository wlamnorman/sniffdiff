use crate::analysis::facts::{
    MatchedReference, ReferenceMatchSource, SymbolChange, SymbolReferenceDelta,
    SymbolReferenceFacts,
};
use crate::analysis::modules::ModuleIndex;
use crate::git::{ChangedFile, FileStatus};
use crate::language::{
    QualifiedName, Reference, ReferenceKind, ReferenceResolution, Symbol, SymbolKind, SymbolName,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

pub(crate) fn attach_reference_facts(
    symbol_changes: &mut [SymbolChange],
    before: ReferenceSnapshot<'_>,
    after: ReferenceSnapshot<'_>,
    changed_files: &[ChangedFile],
) {
    let changed_paths = changed_paths(changed_files);

    for change in symbol_changes {
        change.references_before = reference_facts(
            change,
            before.references,
            &changed_paths,
            before.module_index,
            before.symbols,
        );
        change.references_after = reference_facts(
            change,
            after.references,
            &changed_paths,
            after.module_index,
            after.symbols,
        );
        change.reference_delta =
            reference_delta(&change.references_before, &change.references_after);
    }
}

pub(crate) fn attach_test_reference_facts(
    symbol_changes: &mut [SymbolChange],
    tests_after: ReferenceSnapshot<'_>,
    changed_files: &[ChangedFile],
) {
    let changed_paths = changed_paths(changed_files);

    for change in symbol_changes {
        change.test_references_after = reference_facts(
            change,
            tests_after.references,
            &changed_paths,
            tests_after.module_index,
            tests_after.symbols,
        );
    }
}

pub(crate) struct ReferenceSnapshot<'a> {
    pub(crate) references: &'a [Reference],
    pub(crate) module_index: &'a ModuleIndex,
    pub(crate) symbols: &'a [Symbol],
}

fn reference_facts(
    change: &SymbolChange,
    references: &[Reference],
    changed_paths: &BTreeSet<PathBuf>,
    module_index: &ModuleIndex,
    symbols: &[Symbol],
) -> SymbolReferenceFacts {
    let matching_references = references
        .iter()
        .filter_map(|reference| {
            let caller_symbol = enclosing_symbol(reference, symbols);
            reference_match_source(reference, change, module_index, caller_symbol.as_ref()).map(
                |match_source| MatchedReference {
                    file: reference.file.clone(),
                    line: reference.line,
                    caller_symbol,
                    kind: reference.kind,
                    resolution: reference.resolution,
                    match_source,
                    changed_file: changed_paths.contains(&reference.file),
                },
            )
        })
        .collect::<Vec<_>>();
    let files = matching_references
        .iter()
        .map(|reference| reference.file.clone())
        .collect::<BTreeSet<_>>();
    let (changed, unchanged): (BTreeSet<_>, BTreeSet<_>) = files
        .iter()
        .cloned()
        .partition(|file| changed_paths.contains(file));

    SymbolReferenceFacts {
        count: matching_references.len(),
        resolved_count: matching_references
            .iter()
            .filter(|reference| reference.resolution == ReferenceResolution::Resolved)
            .count(),
        unresolved_count: matching_references
            .iter()
            .filter(|reference| reference.resolution == ReferenceResolution::Unresolved)
            .count(),
        import_count: count_reference_kind(&matching_references, ReferenceKind::Import),
        from_import_count: count_reference_kind(&matching_references, ReferenceKind::FromImport),
        direct_call_count: count_reference_kind(&matching_references, ReferenceKind::Call),
        attribute_call_count: count_reference_kind(&matching_references, ReferenceKind::Attribute),
        files: files.into_iter().collect(),
        changed_files: changed.into_iter().collect(),
        unchanged_files: unchanged.into_iter().collect(),
        matched_references: matching_references,
    }
}

fn enclosing_symbol(reference: &Reference, symbols: &[Symbol]) -> Option<QualifiedName> {
    symbols
        .iter()
        .filter(|symbol| symbol.file == reference.file)
        .filter(|symbol| {
            symbol.range.start <= reference.line
                && reference.line <= symbol.range.end
                && symbol.qualified_name.short_name() != reference.name
        })
        .min_by_key(|symbol| symbol.range.end - symbol.range.start)
        .map(|symbol| symbol.qualified_name.clone())
}

fn reference_match_source(
    reference: &Reference,
    change: &SymbolChange,
    module_index: &ModuleIndex,
    caller_symbol: Option<&QualifiedName>,
) -> Option<ReferenceMatchSource> {
    let symbol = change.after.as_ref().or(change.before.as_ref());
    let Some(symbol) = symbol else {
        return (reference.name == change.id.qualified_name.short_name())
            .then_some(ReferenceMatchSource::UnresolvedShortName);
    };

    let module = ModuleIndex::symbol_module(symbol);
    let top_level_name = ModuleIndex::symbol_top_level_name(symbol);
    let short_name = symbol.qualified_name.short_name();

    if reference.resolution == ReferenceResolution::Resolved {
        let Some(resolved_module) = &reference.resolved_module else {
            return None;
        };
        let Some(resolved_name) = &reference.resolved_name else {
            return None;
        };

        let matches = resolved_module == &module
            && module_index.contains_symbol(resolved_module, resolved_name)
            && (resolved_name == &top_level_name || resolved_name == &short_name);

        if matches {
            return Some(match reference.kind {
                ReferenceKind::Attribute => ReferenceMatchSource::ResolvedAttribute,
                ReferenceKind::Import | ReferenceKind::FromImport | ReferenceKind::Call => {
                    ReferenceMatchSource::ResolvedImport
                }
            });
        }

        return None;
    }

    unresolved_short_name_match(reference, symbol, &short_name, caller_symbol)
}

fn unresolved_short_name_match(
    reference: &Reference,
    symbol: &Symbol,
    short_name: &SymbolName,
    caller_symbol: Option<&QualifiedName>,
) -> Option<ReferenceMatchSource> {
    if reference.name != *short_name {
        return None;
    }

    if symbol.kind == SymbolKind::Method {
        return unresolved_same_class_method_match(reference, symbol, caller_symbol);
    }

    if is_noisy_unresolved_name(short_name.as_str()) {
        return None;
    }

    Some(ReferenceMatchSource::UnresolvedShortName)
}

fn unresolved_same_class_method_match(
    reference: &Reference,
    symbol: &Symbol,
    caller_symbol: Option<&QualifiedName>,
) -> Option<ReferenceMatchSource> {
    if reference.kind != ReferenceKind::Attribute || reference.file != symbol.file {
        return None;
    }

    let receiver = reference.module.as_ref()?.to_string();
    if receiver != "self" && receiver != "cls" {
        return None;
    }

    let target_owner = symbol.qualified_name.as_str().rsplit_once('.')?.0;
    let caller_owner = caller_symbol?.as_str().rsplit_once('.')?.0;

    (target_owner == caller_owner).then_some(ReferenceMatchSource::UnresolvedShortName)
}

fn is_noisy_unresolved_name(name: &str) -> bool {
    matches!(
        name,
        "__call__"
            | "__enter__"
            | "__exit__"
            | "__init__"
            | "__iter__"
            | "__next__"
            | "close"
            | "get"
            | "main"
            | "open"
            | "read"
            | "run"
            | "set"
            | "write"
    )
}

fn count_reference_kind(references: &[MatchedReference], kind: ReferenceKind) -> usize {
    references
        .iter()
        .filter(|reference| reference.kind == kind)
        .count()
}

fn reference_delta(
    before: &SymbolReferenceFacts,
    after: &SymbolReferenceFacts,
) -> SymbolReferenceDelta {
    let before_files = before.files.iter().cloned().collect::<BTreeSet<_>>();
    let after_files = after.files.iter().cloned().collect::<BTreeSet<_>>();

    SymbolReferenceDelta {
        count_delta: after.count as isize - before.count as isize,
        added_files: after_files.difference(&before_files).cloned().collect(),
        removed_files: before_files.difference(&after_files).cloned().collect(),
        unchanged_files: before_files.intersection(&after_files).cloned().collect(),
    }
}

fn changed_paths(changed_files: &[ChangedFile]) -> BTreeSet<PathBuf> {
    changed_files
        .iter()
        .flat_map(|file| match &file.status {
            FileStatus::Renamed { old_path } => vec![file.path.clone(), old_path.clone()],
            FileStatus::Added | FileStatus::Modified | FileStatus::Deleted => {
                vec![file.path.clone()]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::analysis::facts::{ChangeKind, SymbolFacts, SymbolId, SymbolVisibility};
    use crate::git::FileStatus;
    use crate::language::{
        BodyHash, LineRange, QualifiedName, Signature, Symbol, SymbolKind, SymbolName,
    };

    #[test]
    fn attaches_reference_facts_to_changed_symbols() {
        let mut changes = vec![SymbolChange {
            id: SymbolId {
                file: PathBuf::from("src/features.py"),
                qualified_name: QualifiedName::new("build_features"),
            },
            kinds: vec![ChangeKind::BodyChanged],
            symbol_facts: SymbolFacts {
                kind: SymbolKind::Function,
                visibility: SymbolVisibility::Public,
            },
            signature_change: None,
            complexity_delta: None,
            before: Some(symbol(
                "src/features.py",
                "build_features",
                "def build_features(rows):",
                "aaa",
            )),
            after: Some(symbol(
                "src/features.py",
                "build_features",
                "def build_features(rows):",
                "bbb",
            )),
            references_before: SymbolReferenceFacts::default(),
            references_after: SymbolReferenceFacts::default(),
            test_references_after: SymbolReferenceFacts::default(),
            reference_delta: Default::default(),
            review_signals: Vec::new(),
        }];
        let references = vec![
            reference("src/train.py", "build_features"),
            reference("tests/test_features.py", "build_features"),
        ];
        let changed_files = vec![ChangedFile {
            path: PathBuf::from("tests/test_features.py"),
            status: FileStatus::Modified,
        }];

        let module_index = ModuleIndex::from_symbols(
            &changes
                .iter()
                .filter_map(|change| change.after.clone())
                .collect::<Vec<_>>(),
        );
        let symbols = vec![
            symbol("src/train.py", "train", "def train(rows):", "train"),
            symbol(
                "tests/test_features.py",
                "test_build_features",
                "def test_build_features():",
                "test",
            ),
        ];
        attach_reference_facts(
            &mut changes,
            ReferenceSnapshot {
                references: &[],
                module_index: &module_index,
                symbols: &[],
            },
            ReferenceSnapshot {
                references: &references,
                module_index: &module_index,
                symbols: &symbols,
            },
            &changed_files,
        );

        assert_eq!(changes[0].references_after.count, 2);
        assert_eq!(
            changes[0].references_after.files,
            vec![
                PathBuf::from("src/train.py"),
                PathBuf::from("tests/test_features.py")
            ]
        );
        assert_eq!(
            changes[0].references_after.changed_files,
            vec![PathBuf::from("tests/test_features.py")]
        );
        assert_eq!(
            changes[0].references_after.unchanged_files,
            vec![PathBuf::from("src/train.py")]
        );
        assert_eq!(changes[0].references_after.matched_references.len(), 2);
        assert_eq!(
            changes[0].references_after.matched_references[0].caller_symbol,
            Some(QualifiedName::new("train"))
        );
        assert_eq!(changes[0].reference_delta.count_delta, 2);
        assert_eq!(
            changes[0].reference_delta.added_files,
            vec![
                PathBuf::from("src/train.py"),
                PathBuf::from("tests/test_features.py")
            ]
        );
    }

    #[test]
    fn does_not_match_unresolved_method_calls_by_short_name() {
        let mut method = symbol(
            "src/features.py",
            "Formatter.format",
            "def format(self, value):",
            "aaa",
        );
        method.kind = SymbolKind::Method;
        let mut changes = vec![SymbolChange {
            id: SymbolId {
                file: PathBuf::from("src/features.py"),
                qualified_name: QualifiedName::new("Formatter.format"),
            },
            kinds: vec![ChangeKind::BodyChanged],
            symbol_facts: SymbolFacts {
                kind: SymbolKind::Method,
                visibility: SymbolVisibility::Public,
            },
            signature_change: None,
            complexity_delta: None,
            before: Some(method.clone()),
            after: Some(method.clone()),
            references_before: SymbolReferenceFacts::default(),
            references_after: SymbolReferenceFacts::default(),
            test_references_after: SymbolReferenceFacts::default(),
            reference_delta: Default::default(),
            review_signals: Vec::new(),
        }];
        let references = vec![reference("src/other.py", "format")];
        let module_index = ModuleIndex::from_symbols(&[method]);

        attach_reference_facts(
            &mut changes,
            ReferenceSnapshot {
                references: &[],
                module_index: &module_index,
                symbols: &[],
            },
            ReferenceSnapshot {
                references: &references,
                module_index: &module_index,
                symbols: &[],
            },
            &[],
        );

        assert_eq!(changes[0].references_after.count, 0);
    }

    #[test]
    fn matches_unresolved_same_class_self_method_calls() {
        let mut method = symbol(
            "src/features.py",
            "Formatter.format",
            "def format(self, value):",
            "aaa",
        );
        method.kind = SymbolKind::Method;
        let mut caller = symbol(
            "src/features.py",
            "Formatter.format_many",
            "def format_many(self, values):",
            "bbb",
        );
        caller.kind = SymbolKind::Method;
        caller.range = LineRange { start: 10, end: 12 };
        let mut changes = vec![SymbolChange {
            id: SymbolId {
                file: PathBuf::from("src/features.py"),
                qualified_name: QualifiedName::new("Formatter.format"),
            },
            kinds: vec![ChangeKind::BodyChanged],
            symbol_facts: SymbolFacts {
                kind: SymbolKind::Method,
                visibility: SymbolVisibility::Public,
            },
            signature_change: None,
            complexity_delta: None,
            before: Some(method.clone()),
            after: Some(method.clone()),
            references_before: SymbolReferenceFacts::default(),
            references_after: SymbolReferenceFacts::default(),
            test_references_after: SymbolReferenceFacts::default(),
            reference_delta: Default::default(),
            review_signals: Vec::new(),
        }];
        let references = vec![attribute_reference("src/features.py", "self", "format", 11)];
        let module_index = ModuleIndex::from_symbols(&[method.clone(), caller.clone()]);

        attach_reference_facts(
            &mut changes,
            ReferenceSnapshot {
                references: &[],
                module_index: &module_index,
                symbols: &[],
            },
            ReferenceSnapshot {
                references: &references,
                module_index: &module_index,
                symbols: &[method, caller],
            },
            &[],
        );

        assert_eq!(changes[0].references_after.count, 1);
        assert_eq!(
            changes[0].references_after.matched_references[0].caller_symbol,
            Some(QualifiedName::new("Formatter.format_many"))
        );
    }

    fn symbol(file: &str, name: &str, signature: &str, body_hash: &str) -> Symbol {
        Symbol {
            file: PathBuf::from(file),
            qualified_name: QualifiedName::new(name),
            kind: SymbolKind::Function,
            signature: Signature::new(signature),
            signature_facts: None,
            range: LineRange { start: 1, end: 2 },
            body_hash: BodyHash::new(body_hash),
            complexity: Default::default(),
        }
    }

    fn reference(file: &str, name: &str) -> Reference {
        Reference {
            file: PathBuf::from(file),
            name: SymbolName::new(name),
            module: None,
            resolved_name: Some(SymbolName::new(name)),
            resolved_module: None,
            resolution: ReferenceResolution::Unresolved,
            line: 1,
            kind: ReferenceKind::Call,
        }
    }

    fn attribute_reference(file: &str, module: &str, name: &str, line: usize) -> Reference {
        Reference {
            file: PathBuf::from(file),
            name: SymbolName::new(name),
            module: Some(crate::language::ModuleName::new(module)),
            resolved_name: Some(SymbolName::new(name)),
            resolved_module: Some(crate::language::ModuleName::new(module)),
            resolution: ReferenceResolution::Unresolved,
            line,
            kind: ReferenceKind::Attribute,
        }
    }
}
