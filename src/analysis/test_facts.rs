use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::analysis::facts::Snapshot;
use crate::analysis::facts::{SymbolChange, TestFacts};
use crate::analysis::filter::is_test_path;
use crate::git::ChangedFile;

pub(crate) fn derive_test_facts(
    changed_files: &[ChangedFile],
    symbol_changes: &[SymbolChange],
    test_snapshot: &Snapshot,
) -> TestFacts {
    let changed_test_files = changed_files
        .iter()
        .filter(|file| is_test_path(&file.path))
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let changed_test_modules = changed_test_files
        .iter()
        .filter_map(|path| nearby_production_stem(path))
        .collect::<BTreeSet<_>>();
    let changed_production_files = symbol_changes
        .iter()
        .filter_map(changed_symbol_file)
        .filter(|path| !is_test_path(path))
        .collect::<BTreeSet<_>>();

    let (with_nearby, without_nearby): (BTreeSet<_>, BTreeSet<_>) =
        changed_production_files.into_iter().partition(|path| {
            production_stem(path)
                .as_ref()
                .is_some_and(|stem| changed_test_modules.contains(stem))
        });

    TestFacts {
        changed_test_files: changed_test_files.into_iter().collect(),
        test_files_parsed: test_snapshot.files_parsed,
        test_files_with_parse_errors: test_snapshot.files_with_parse_errors,
        test_parse_error_files: test_snapshot.parse_error_files.clone(),
        production_files_with_nearby_test_changes: with_nearby.into_iter().collect(),
        production_files_without_nearby_test_changes: without_nearby.into_iter().collect(),
    }
}

fn changed_symbol_file(change: &SymbolChange) -> Option<PathBuf> {
    change
        .after
        .as_ref()
        .or(change.before.as_ref())
        .map(|symbol| symbol.file.clone())
}

fn nearby_production_stem(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;
    let stem = file_name.strip_suffix(".py")?;
    let stem = stem
        .strip_prefix("test_")
        .or_else(|| stem.strip_suffix("_test"))
        .unwrap_or(stem);

    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

fn production_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use crate::analysis::facts::{
        ChangeKind, SymbolFacts, SymbolId, SymbolReferenceFacts, SymbolVisibility,
    };
    use crate::git::FileStatus;
    use crate::language::{
        BodyHash, ComplexityMetrics, LineRange, QualifiedName, Signature, Symbol, SymbolKind,
    };

    use super::*;

    #[test]
    fn separates_changed_production_files_by_nearby_test_movement() {
        let changed_files = vec![ChangedFile {
            path: PathBuf::from("tests/test_features.py"),
            status: FileStatus::Modified,
        }];
        let symbol_changes = vec![
            change("src/features.py", "build_features"),
            change("src/train.py", "train"),
        ];

        let facts = derive_test_facts(&changed_files, &symbol_changes, &test_snapshot());

        assert_eq!(
            facts.changed_test_files,
            vec![PathBuf::from("tests/test_features.py")]
        );
        assert_eq!(
            facts.production_files_with_nearby_test_changes,
            vec![PathBuf::from("src/features.py")]
        );
        assert_eq!(
            facts.production_files_without_nearby_test_changes,
            vec![PathBuf::from("src/train.py")]
        );
    }

    fn change(file: &str, name: &str) -> SymbolChange {
        let symbol = Symbol {
            file: PathBuf::from(file),
            qualified_name: QualifiedName::new(name),
            kind: SymbolKind::Function,
            signature: Signature::new(format!("def {name}():")),
            signature_facts: None,
            range: LineRange { start: 1, end: 2 },
            body_hash: BodyHash::new("hash"),
            complexity: ComplexityMetrics::default(),
        };

        SymbolChange {
            id: SymbolId {
                file: PathBuf::from(file),
                qualified_name: QualifiedName::new(name),
            },
            kinds: vec![ChangeKind::BodyChanged],
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

    fn test_snapshot() -> Snapshot {
        Snapshot {
            git_ref: "HEAD".to_string(),
            files_considered: 1,
            files_skipped: 0,
            files_parsed: 1,
            files_with_parse_errors: 0,
            parse_error_files: Vec::new(),
            symbols: Vec::new(),
        }
    }
}
