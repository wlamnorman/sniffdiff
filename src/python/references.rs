use std::collections::BTreeMap;
use std::path::Path;

use tree_sitter::Node;

use crate::language::{
    ModuleName, ParsedFile, Reference, ReferenceKind, ReferenceResolution, SymbolName,
};

pub(crate) fn extract_references(parsed: &ParsedFile) -> Vec<Reference> {
    let mut references = Vec::new();
    let mut imports = ImportBindings::default();
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
    imports: &mut ImportBindings,
    references: &mut Vec<Reference>,
) {
    match node.kind() {
        "import_statement" => {
            let import_references = import_references(node, source, file);
            for reference in &import_references {
                let Some(module) = &reference.resolved_module else {
                    continue;
                };
                imports
                    .modules
                    .insert(reference.name.clone(), module.clone());
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
                imports
                    .symbols
                    .insert(reference.name.clone(), (module.clone(), name.clone()));
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

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_references(child, source, file, imports, references);
    }
}

#[derive(Debug, Default)]
struct ImportBindings {
    modules: BTreeMap<SymbolName, ModuleName>,
    symbols: BTreeMap<SymbolName, (ModuleName, SymbolName)>,
}

fn import_references(node: Node, source: &str, file: &Path) -> Vec<Reference> {
    let text = node_text(node, source);
    let Some(imports) = text.strip_prefix("import ") else {
        return Vec::new();
    };

    imports
        .split(',')
        .filter_map(|part| {
            let (imported, alias) = split_alias(part.trim());
            if imported.is_empty() {
                return None;
            }
            let local_name = alias
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| imported.rsplit('.').next().unwrap_or(imported).to_string());

            Some(Reference {
                file: file.to_path_buf(),
                name: SymbolName::new(local_name),
                module: Some(ModuleName::new(imported)),
                resolved_name: None,
                resolved_module: Some(ModuleName::new(imported)),
                resolution: ReferenceResolution::Resolved,
                line: node.start_position().row + 1,
                kind: ReferenceKind::Import,
            })
        })
        .collect()
}

fn from_import_references(node: Node, source: &str, file: &Path) -> Vec<Reference> {
    let text = node_text(node, source);
    let Some(rest) = text.strip_prefix("from ") else {
        return Vec::new();
    };
    let Some((module, names)) = rest.split_once(" import ") else {
        return Vec::new();
    };

    let module = ModuleName::new(resolve_module(file, module.trim()));
    names
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .filter_map(|part| {
            let (imported, alias) = split_alias(part.trim());
            if imported.is_empty() || imported == "*" {
                return None;
            }
            let local_name = alias.unwrap_or(imported);

            Some(Reference {
                file: file.to_path_buf(),
                name: SymbolName::new(local_name),
                module: Some(module.clone()),
                resolved_name: Some(SymbolName::new(imported)),
                resolved_module: Some(module.clone()),
                resolution: ReferenceResolution::Resolved,
                line: node.start_position().row + 1,
                kind: ReferenceKind::FromImport,
            })
        })
        .collect()
}

fn call_reference(
    node: Node,
    source: &str,
    file: &Path,
    imports: &ImportBindings,
) -> Option<Reference> {
    let function = node.child_by_field_name("function")?;
    let line = function.start_position().row + 1;

    match function.kind() {
        "identifier" => {
            let name = SymbolName::new(node_text(function, source));
            let resolved = imports.symbols.get(&name);
            Some(Reference {
                file: file.to_path_buf(),
                name: name.clone(),
                module: None,
                resolved_name: resolved
                    .map(|(_, resolved_name)| resolved_name.clone())
                    .or(Some(name)),
                resolved_module: resolved.map(|(module, _)| module.clone()),
                resolution: resolved
                    .map(|_| ReferenceResolution::Resolved)
                    .unwrap_or(ReferenceResolution::Unresolved),
                line,
                kind: ReferenceKind::Call,
            })
        }
        "attribute" => {
            let text = node_text(function, source);
            let (module, name) = text.rsplit_once('.')?;
            let module_name = SymbolName::new(module);
            let resolved_module = imports
                .modules
                .get(&module_name)
                .cloned()
                .unwrap_or_else(|| ModuleName::new(module));
            let resolution = if imports.modules.contains_key(&module_name) {
                ReferenceResolution::Resolved
            } else {
                ReferenceResolution::Unresolved
            };
            Some(Reference {
                file: file.to_path_buf(),
                name: SymbolName::new(name),
                module: Some(ModuleName::new(module)),
                resolved_name: Some(SymbolName::new(name)),
                resolved_module: Some(resolved_module),
                resolution,
                line,
                kind: ReferenceKind::Attribute,
            })
        }
        _ => None,
    }
}

fn split_alias(text: &str) -> (&str, Option<&str>) {
    text.split_once(" as ")
        .map(|(name, alias)| (name.trim(), Some(alias.trim())))
        .unwrap_or((text.trim(), None))
}

fn resolve_module(file: &Path, module: &str) -> String {
    let leading_dots = module
        .chars()
        .take_while(|character| *character == '.')
        .count();
    if leading_dots == 0 {
        return module.to_string();
    }

    let suffix = module.trim_start_matches('.');
    let mut package = file.parent().map(path_to_module_parts).unwrap_or_default();
    for _ in 1..leading_dots {
        package.pop();
    }
    if !suffix.is_empty() {
        package.extend(suffix.split('.').map(ToOwned::to_owned));
    }

    package.join(".")
}

fn path_to_module_parts(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .filter(|component| !component.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn node_text(node: Node, source: &str) -> String {
    node.utf8_text(source.as_bytes())
        .unwrap_or_default()
        .trim()
        .to_string()
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
}
