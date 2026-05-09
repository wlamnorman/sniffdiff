use crate::analysis::facts::{
    MatchedReference, ReferenceMatchSource, SymbolChange, SymbolReferenceDelta,
    SymbolReferenceFacts,
};
use crate::analysis::modules::ModuleIndex;
use crate::git::{ChangedFile, FileStatus};
use crate::language::{QualifiedName, Reference, ReferenceKind, ReferenceResolution, Symbol};
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
            reference_match_source(reference, change, module_index).map(|match_source| {
                MatchedReference {
                    file: reference.file.clone(),
                    line: reference.line,
                    caller_symbol: enclosing_symbol(reference, symbols),
                    kind: reference.kind,
                    resolution: reference.resolution,
                    match_source,
                    changed_file: changed_paths.contains(&reference.file),
                }
            })
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

    (reference.name == short_name).then_some(ReferenceMatchSource::UnresolvedShortName)
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
                visibility: SymbolVisibility::Publicish,
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
}
