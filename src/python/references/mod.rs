use std::path::Path;

use tree_sitter::Node;

use crate::language::{ModuleName, ParsedFile, Reference};

mod calls;
mod imports;
mod scopes;
mod util;

use calls::call_reference;
use imports::{from_import_references, import_references};
use scopes::{ImportScopes, ScopeKind};

pub(crate) fn extract_references(parsed: &ParsedFile) -> Vec<Reference> {
    let mut references = Vec::new();
    let mut imports = ImportScopes::default();
    walk_references(
        parsed.tree.root_node(),
        &parsed.source,
        &parsed.file,
        &mut imports,
        &mut references,
    );
    references
}

fn walk_references(
    node: Node,
    source: &str,
    file: &Path,
    imports: &mut ImportScopes,
    references: &mut Vec<Reference>,
) {
    if node.kind() == "function_definition" {
        imports.enter_scope(ScopeKind::Function);
        walk_reference_children(node, source, file, imports, references);
        imports.exit_scope();
        return;
    }

    if node.kind() == "class_definition" {
        imports.enter_scope(ScopeKind::Class);
        walk_reference_children(node, source, file, imports, references);
        imports.exit_scope();
        return;
    }

    match node.kind() {
        "import_statement" => {
            let import_references = import_references(node, source, file);
            for reference in &import_references {
                let Some(module) = &reference.resolved_module else {
                    continue;
                };
                let imported_module = ModuleName::new(
                    reference
                        .module
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| module.to_string()),
                );
                imports.insert_module(reference.name.clone(), module.clone(), imported_module);
            }
            references.extend(import_references);
        }
        "import_from_statement" => {
            let from_import_references = from_import_references(node, source, file);
            for reference in &from_import_references {
                let (Some(module), Some(name)) =
                    (&reference.resolved_module, &reference.resolved_name)
                else {
                    continue;
                };
                imports.insert_symbol(reference.name.clone(), module.clone(), name.clone());
            }
            references.extend(from_import_references);
        }
        "call" => {
            if let Some(reference) = call_reference(node, source, file, imports) {
                references.push(reference);
            }
        }
        _ => {}
    }

    walk_reference_children(node, source, file, imports, references);
}

fn walk_reference_children(
    node: Node,
    source: &str,
    file: &Path,
    imports: &mut ImportScopes,
    references: &mut Vec<Reference>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_references(child, source, file, imports, references);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::language::{LanguageAdapter, ReferenceKind, ReferenceResolution};
    use crate::python::PythonAdapter;

    #[test]
    fn extracts_import_and_call_references() {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("src/train.py"),
                r#"
import src.features as features
from src.features import build_features, Formatter as NameFormatter


def train(rows):
    values = build_features(rows)
    return features.build_features(values)
"#
                .trim_start()
                .to_string(),
            )
            .unwrap();

        let references = adapter.extract_references(&parsed).unwrap();
        let facts = references
            .iter()
            .map(|reference| {
                (
                    reference.kind,
                    reference.module.as_ref().map(ToString::to_string),
                    reference.name.as_str(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            facts,
            vec![
                (
                    ReferenceKind::Import,
                    Some("src.features".to_string()),
                    "features"
                ),
                (
                    ReferenceKind::FromImport,
                    Some("src.features".to_string()),
                    "build_features"
                ),
                (
                    ReferenceKind::FromImport,
                    Some("src.features".to_string()),
                    "NameFormatter"
                ),
                (ReferenceKind::Call, None, "build_features"),
                (
                    ReferenceKind::Attribute,
                    Some("features".to_string()),
                    "build_features"
                ),
            ]
        );
    }

    #[test]
    fn resolves_import_aliases_for_calls() {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("src/train.py"),
                r#"
import src.features as features
from src.features import build_features as make_features


def train(rows):
    values = make_features(rows)
    return features.build_features(values)
"#
                .trim_start()
                .to_string(),
            )
            .unwrap();

        let calls = adapter
            .extract_references(&parsed)
            .unwrap()
            .into_iter()
            .filter(|reference| {
                matches!(
                    reference.kind,
                    ReferenceKind::Call | ReferenceKind::Attribute
                )
            })
            .map(|reference| {
                (
                    reference.name.to_string(),
                    reference.resolved_name.map(|name| name.to_string()),
                    reference.resolved_module.map(|module| module.to_string()),
                    reference.resolution,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            calls,
            vec![
                (
                    "make_features".to_string(),
                    Some("build_features".to_string()),
                    Some("src.features".to_string()),
                    ReferenceResolution::Resolved,
                ),
                (
                    "build_features".to_string(),
                    Some("build_features".to_string()),
                    Some("src.features".to_string()),
                    ReferenceResolution::Resolved,
                ),
            ]
        );
    }

    #[test]
    fn resolves_unaliased_dotted_imports_for_attribute_calls() {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("pkg/consumer.py"),
                r#"
import pkg.features


def run(rows):
    return pkg.features.build_features(rows)
"#
                .trim_start()
                .to_string(),
            )
            .unwrap();

        let references = adapter.extract_references(&parsed).unwrap();
        let calls = references
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::Attribute)
            .map(|reference| {
                (
                    reference.name.to_string(),
                    reference.module.as_ref().map(ToString::to_string),
                    reference.resolved_module.as_ref().map(ToString::to_string),
                    reference.resolution,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            calls,
            vec![(
                "build_features".to_string(),
                Some("pkg.features".to_string()),
                Some("pkg.features".to_string()),
                ReferenceResolution::Resolved,
            )]
        );
    }

    #[test]
    fn resolves_from_imported_modules_for_attribute_calls() {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("pkg/consumer.py"),
                r#"
from pkg import features


def run(rows):
    return features.build_features(rows)
"#
                .trim_start()
                .to_string(),
            )
            .unwrap();

        let references = adapter.extract_references(&parsed).unwrap();
        let calls = references
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::Attribute)
            .map(|reference| {
                (
                    reference.name.to_string(),
                    reference.module.as_ref().map(ToString::to_string),
                    reference.resolved_module.as_ref().map(ToString::to_string),
                    reference.resolution,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            calls,
            vec![(
                "build_features".to_string(),
                Some("features".to_string()),
                Some("pkg.features".to_string()),
                ReferenceResolution::Resolved,
            )]
        );
    }

    #[test]
    fn keeps_function_local_imports_from_leaking_to_later_functions() {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("pkg/consumer.py"),
                r#"
def configure(rows):
    from pkg import features as local_features
    return local_features.build_features(rows)


def run(rows):
    return local_features.build_features(rows)
"#
                .trim_start()
                .to_string(),
            )
            .unwrap();

        let calls = adapter
            .extract_references(&parsed)
            .unwrap()
            .into_iter()
            .filter(|reference| reference.kind == ReferenceKind::Attribute)
            .map(|reference| {
                (
                    reference.line,
                    reference.resolved_module.map(|module| module.to_string()),
                    reference.resolution,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            calls,
            vec![
                (
                    3,
                    Some("pkg.features".to_string()),
                    ReferenceResolution::Resolved,
                ),
                (
                    7,
                    Some("local_features".to_string()),
                    ReferenceResolution::Unresolved,
                ),
            ]
        );
    }

    #[test]
    fn keeps_class_imports_from_leaking_into_methods() {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("pkg/consumer.py"),
                r#"
class Runner:
    import pkg.features as features

    def run(self, rows):
        return features.build_features(rows)
"#
                .trim_start()
                .to_string(),
            )
            .unwrap();

        let calls = adapter
            .extract_references(&parsed)
            .unwrap()
            .into_iter()
            .filter(|reference| reference.kind == ReferenceKind::Attribute)
            .map(|reference| {
                (
                    reference.line,
                    reference.resolved_module.map(|module| module.to_string()),
                    reference.resolution,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            calls,
            vec![(
                5,
                Some("features".to_string()),
                ReferenceResolution::Unresolved
            )]
        );
    }

    #[test]
    fn keeps_class_imports_visible_inside_class_body() {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("pkg/consumer.py"),
                r#"
class Runner:
    import pkg.features as features
    rows = features.build_features([])
"#
                .trim_start()
                .to_string(),
            )
            .unwrap();

        let calls = adapter
            .extract_references(&parsed)
            .unwrap()
            .into_iter()
            .filter(|reference| reference.kind == ReferenceKind::Attribute)
            .map(|reference| {
                (
                    reference.line,
                    reference.resolved_module.map(|module| module.to_string()),
                    reference.resolution,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            calls,
            vec![(
                3,
                Some("pkg.features".to_string()),
                ReferenceResolution::Resolved,
            )]
        );
    }

    #[test]
    fn resolves_relative_imports_against_file_package() {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("pkg/sub/consumer.py"),
                r#"
from ..features import build_features as make_features


def run(rows):
    return make_features(rows)
"#
                .trim_start()
                .to_string(),
            )
            .unwrap();

        let references = adapter.extract_references(&parsed).unwrap();

        assert_eq!(
            references
                .iter()
                .map(|reference| (
                    reference.kind,
                    reference.name.to_string(),
                    reference.resolved_module.as_ref().map(ToString::to_string),
                    reference.resolved_name.as_ref().map(ToString::to_string),
                    reference.resolution,
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    ReferenceKind::FromImport,
                    "make_features".to_string(),
                    Some("pkg.features".to_string()),
                    Some("build_features".to_string()),
                    ReferenceResolution::Resolved,
                ),
                (
                    ReferenceKind::Call,
                    "make_features".to_string(),
                    Some("pkg.features".to_string()),
                    Some("build_features".to_string()),
                    ReferenceResolution::Resolved,
                ),
            ]
        );
    }

    #[test]
    fn resolves_relative_from_imported_modules_for_attribute_calls() {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("pkg/sub/consumer.py"),
                r#"
from .. import features
from . import local_features as local


def run(rows):
    first = features.build_features(rows)
    return local.build_features(first)
"#
                .trim_start()
                .to_string(),
            )
            .unwrap();

        let calls = adapter
            .extract_references(&parsed)
            .unwrap()
            .into_iter()
            .filter(|reference| reference.kind == ReferenceKind::Attribute)
            .map(|reference| {
                (
                    reference.name.to_string(),
                    reference.resolved_module.map(|module| module.to_string()),
                    reference.resolution,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            calls,
            vec![
                (
                    "build_features".to_string(),
                    Some("pkg.features".to_string()),
                    ReferenceResolution::Resolved,
                ),
                (
                    "build_features".to_string(),
                    Some("pkg.sub.local_features".to_string()),
                    ReferenceResolution::Resolved,
                ),
            ]
        );
    }
}
