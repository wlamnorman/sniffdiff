use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::analysis::facts::{
    ChangeKind, ComplexityDelta, SignatureChangeFacts, SymbolChange, SymbolFacts, SymbolId,
    SymbolReferenceFacts, SymbolVisibility,
};
use crate::git::{ChangedFile, FileStatus};
use crate::language::{FunctionSignatureFacts, Symbol};

pub(crate) fn diff_symbols(
    before: &[Symbol],
    after: &[Symbol],
    changed_files: &[ChangedFile],
) -> Vec<SymbolChange> {
    let renamed_paths = renamed_paths(changed_files);
    let before_index = symbol_index(before, &renamed_paths);
    let after_index = symbol_index(after, &BTreeMap::new());
    let ids = before_index
        .keys()
        .chain(after_index.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    ids.into_iter()
        .filter_map(|id| {
            let before = before_index.get(&id).copied();
            let after = after_index.get(&id).copied();
            let kinds = change_kinds(&id, before, after);

            if kinds.is_empty() {
                return None;
            }

            Some(SymbolChange {
                symbol_facts: symbol_facts(before, after),
                signature_change: signature_change_facts(before, after),
                complexity_delta: complexity_delta(before, after),
                id,
                kinds,
                before: before.cloned(),
                after: after.cloned(),
                references_before: SymbolReferenceFacts::default(),
                references_after: SymbolReferenceFacts::default(),
                test_references_after: SymbolReferenceFacts::default(),
                reference_delta: Default::default(),
                review_signals: Vec::new(),
            })
        })
        .collect()
}

fn symbol_facts(before: Option<&Symbol>, after: Option<&Symbol>) -> SymbolFacts {
    let symbol = after.or(before).expect("symbol facts require one side");
    let short_name = symbol.qualified_name.short_name();
    let is_private = short_name.as_str().starts_with('_');

    SymbolFacts {
        kind: symbol.kind,
        visibility: if is_private {
            SymbolVisibility::Private
        } else {
            SymbolVisibility::Publicish
        },
    }
}

fn symbol_index<'a>(
    symbols: &'a [Symbol],
    renamed_paths: &BTreeMap<PathBuf, PathBuf>,
) -> BTreeMap<SymbolId, &'a Symbol> {
    symbols
        .iter()
        .map(|symbol| {
            (
                SymbolId {
                    file: renamed_paths
                        .get(&symbol.file)
                        .cloned()
                        .unwrap_or_else(|| symbol.file.clone()),
                    qualified_name: symbol.qualified_name.clone(),
                },
                symbol,
            )
        })
        .collect()
}

fn renamed_paths(changed_files: &[ChangedFile]) -> BTreeMap<PathBuf, PathBuf> {
    changed_files
        .iter()
        .filter_map(|file| match &file.status {
            FileStatus::Renamed { old_path } => Some((old_path.clone(), file.path.clone())),
            FileStatus::Added | FileStatus::Modified | FileStatus::Deleted => None,
        })
        .collect()
}

fn change_kinds(id: &SymbolId, before: Option<&Symbol>, after: Option<&Symbol>) -> Vec<ChangeKind> {
    match (before, after) {
        (None, Some(_)) => vec![ChangeKind::Added],
        (Some(_), None) => vec![ChangeKind::Deleted],
        (Some(before), Some(after)) => {
            let mut kinds = Vec::new();
            if before.file != id.file || before.file != after.file {
                kinds.push(ChangeKind::PathChanged);
            }
            if before.body_hash != after.body_hash {
                kinds.push(ChangeKind::BodyChanged);
            }
            if before.signature != after.signature {
                kinds.push(ChangeKind::SignatureChanged);
            }
            kinds
        }
        (None, None) => Vec::new(),
    }
}

fn signature_change_facts(
    before: Option<&Symbol>,
    after: Option<&Symbol>,
) -> Option<SignatureChangeFacts> {
    let before = before?.signature_facts.as_ref()?;
    let after = after?.signature_facts.as_ref()?;
    let facts = compare_signatures(before, after);

    if facts == SignatureChangeFacts::default() {
        None
    } else {
        Some(facts)
    }
}

fn compare_signatures(
    before: &FunctionSignatureFacts,
    after: &FunctionSignatureFacts,
) -> SignatureChangeFacts {
    let before_by_name = before
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter))
        .collect::<BTreeMap<_, _>>();
    let after_by_name = after
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter))
        .collect::<BTreeMap<_, _>>();

    let parameters_added = after_by_name
        .keys()
        .filter(|name| !before_by_name.contains_key(**name))
        .map(|name| (*name).to_string())
        .collect();
    let parameters_removed = before_by_name
        .keys()
        .filter(|name| !after_by_name.contains_key(**name))
        .map(|name| (*name).to_string())
        .collect();

    let shared_names = before_by_name
        .keys()
        .filter(|name| after_by_name.contains_key(**name))
        .copied()
        .collect::<BTreeSet<_>>();

    let mut parameter_kind_changed = Vec::new();
    let mut parameter_default_changed = Vec::new();
    let mut parameter_annotation_changed = Vec::new();

    for name in &shared_names {
        let before = before_by_name[name];
        let after = after_by_name[name];
        if before.kind != after.kind {
            parameter_kind_changed.push((*name).to_string());
        }
        if before.has_default != after.has_default {
            parameter_default_changed.push((*name).to_string());
        }
        if before.annotation != after.annotation {
            parameter_annotation_changed.push((*name).to_string());
        }
    }

    let before_order = before
        .parameters
        .iter()
        .filter(|parameter| shared_names.contains(parameter.name.as_str()))
        .map(|parameter| parameter.name.as_str())
        .collect::<Vec<_>>();
    let after_order = after
        .parameters
        .iter()
        .filter(|parameter| shared_names.contains(parameter.name.as_str()))
        .map(|parameter| parameter.name.as_str())
        .collect::<Vec<_>>();

    SignatureChangeFacts {
        parameters_added,
        parameters_removed,
        parameters_reordered: before_order != after_order,
        parameter_kind_changed,
        parameter_default_changed,
        parameter_annotation_changed,
        return_annotation_changed: before.return_annotation != after.return_annotation,
        async_changed: before.is_async != after.is_async,
    }
}

fn complexity_delta(before: Option<&Symbol>, after: Option<&Symbol>) -> Option<ComplexityDelta> {
    let before = before?;
    let after = after?;

    if before.complexity == after.complexity {
        return None;
    }

    Some(ComplexityDelta {
        before: before.complexity.clone(),
        after: after.complexity.clone(),
        length_lines_delta: delta(
            before.complexity.length_lines,
            after.complexity.length_lines,
        ),
        branch_count_delta: delta(
            before.complexity.branch_count,
            after.complexity.branch_count,
        ),
        loop_count_delta: delta(before.complexity.loop_count, after.complexity.loop_count),
        boolean_operator_count_delta: delta(
            before.complexity.boolean_operator_count,
            after.complexity.boolean_operator_count,
        ),
        exception_handler_count_delta: delta(
            before.complexity.exception_handler_count,
            after.complexity.exception_handler_count,
        ),
        match_count_delta: delta(before.complexity.match_count, after.complexity.match_count),
        with_count_delta: delta(before.complexity.with_count, after.complexity.with_count),
        max_nesting_depth_delta: delta(
            before.complexity.max_nesting_depth,
            after.complexity.max_nesting_depth,
        ),
    })
}

fn delta(before: usize, after: usize) -> isize {
    after as isize - before as isize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::{
        BodyHash, ComplexityMetrics, FunctionSignatureFacts, LineRange, ParameterFacts,
        ParameterKind, QualifiedName, Signature, SymbolKind,
    };

    #[test]
    fn detects_added_deleted_body_and_signature_changes() {
        let before = vec![
            symbol("src/app.py", "removed", "def removed():", "aaa"),
            symbol("src/app.py", "changed", "def changed(x):", "bbb"),
            symbol("src/app.py", "resigned", "def resigned(x):", "ccc"),
            symbol("src/app.py", "same", "def same():", "ddd"),
        ];
        let after = vec![
            symbol("src/app.py", "added", "def added():", "eee"),
            symbol("src/app.py", "changed", "def changed(x):", "zzz"),
            symbol("src/app.py", "resigned", "def resigned(x, y):", "yyy"),
            symbol("src/app.py", "same", "def same():", "ddd"),
        ];

        let changes = diff_symbols(&before, &after, &[]);
        let by_name = changes
            .iter()
            .map(|change| (change.id.qualified_name.as_str(), change.kinds.clone()))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(by_name["added"], vec![ChangeKind::Added]);
        assert_eq!(by_name["removed"], vec![ChangeKind::Deleted]);
        assert_eq!(by_name["changed"], vec![ChangeKind::BodyChanged]);
        assert_eq!(
            by_name["resigned"],
            vec![ChangeKind::BodyChanged, ChangeKind::SignatureChanged]
        );
        assert!(!by_name.contains_key("same"));
    }

    #[test]
    fn treats_symbols_in_renamed_files_as_path_changed_not_deleted_and_added() {
        let before = vec![symbol("src/old.py", "same", "def same():", "aaa")];
        let after = vec![symbol("src/new.py", "same", "def same():", "aaa")];
        let changed_files = vec![ChangedFile {
            path: PathBuf::from("src/new.py"),
            status: FileStatus::Renamed {
                old_path: PathBuf::from("src/old.py"),
            },
        }];

        let changes = diff_symbols(&before, &after, &changed_files);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].id.file, PathBuf::from("src/new.py"));
        assert_eq!(changes[0].kinds, vec![ChangeKind::PathChanged]);
    }

    #[test]
    fn reports_structured_signature_change_facts() {
        let mut before = symbol(
            "src/app.py",
            "build",
            "def build(rows, strict=False):",
            "aaa",
        );
        before.signature_facts = Some(signature_facts(vec![
            parameter("rows", ParameterKind::PositionalOrKeyword, false, None),
            parameter("strict", ParameterKind::PositionalOrKeyword, true, None),
        ]));
        let mut after = symbol(
            "src/app.py",
            "build",
            "def build(rows: list, *, limit=10) -> list:",
            "bbb",
        );
        after.signature_facts = Some(FunctionSignatureFacts {
            is_async: false,
            parameters: vec![
                parameter(
                    "rows",
                    ParameterKind::PositionalOrKeyword,
                    false,
                    Some("list"),
                ),
                parameter("limit", ParameterKind::KeywordOnly, true, None),
            ],
            return_annotation: Some("list".to_string()),
        });

        let changes = diff_symbols(&[before], &[after], &[]);
        let signature_change = changes[0].signature_change.as_ref().unwrap();

        assert_eq!(signature_change.parameters_added, vec!["limit"]);
        assert_eq!(signature_change.parameters_removed, vec!["strict"]);
        assert_eq!(signature_change.parameter_annotation_changed, vec!["rows"]);
        assert!(signature_change.return_annotation_changed);
    }

    #[test]
    fn reports_complexity_delta_facts() {
        let mut before = symbol("src/app.py", "build", "def build(rows):", "aaa");
        before.complexity = ComplexityMetrics {
            length_lines: 3,
            branch_count: 1,
            max_nesting_depth: 1,
            ..ComplexityMetrics::default()
        };
        let mut after = symbol("src/app.py", "build", "def build(rows):", "bbb");
        after.complexity = ComplexityMetrics {
            length_lines: 8,
            branch_count: 3,
            loop_count: 1,
            max_nesting_depth: 2,
            ..ComplexityMetrics::default()
        };

        let changes = diff_symbols(&[before], &[after], &[]);
        let complexity_delta = changes[0].complexity_delta.as_ref().unwrap();

        assert_eq!(complexity_delta.length_lines_delta, 5);
        assert_eq!(complexity_delta.branch_count_delta, 2);
        assert_eq!(complexity_delta.loop_count_delta, 1);
        assert_eq!(complexity_delta.max_nesting_depth_delta, 1);
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
            complexity: ComplexityMetrics::default(),
        }
    }

    fn signature_facts(parameters: Vec<ParameterFacts>) -> FunctionSignatureFacts {
        FunctionSignatureFacts {
            is_async: false,
            parameters,
            return_annotation: None,
        }
    }

    fn parameter(
        name: &str,
        kind: ParameterKind,
        has_default: bool,
        annotation: Option<&str>,
    ) -> ParameterFacts {
        ParameterFacts {
            name: name.to_string(),
            kind,
            has_default,
            annotation: annotation.map(ToOwned::to_owned),
        }
    }
}
