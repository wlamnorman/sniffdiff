use std::path::Path;

use tree_sitter::Node;

use crate::language::{ModuleName, Reference, ReferenceKind, ReferenceResolution, SymbolName};
use crate::python::references::util::{field_children, node_text};

pub(super) fn import_references(node: Node, source: &str, file: &Path) -> Vec<Reference> {
    field_children(node, "name")
        .filter_map(|part| {
            let (imported, alias) = import_name_and_alias(part, source)?;
            if imported.is_empty() {
                return None;
            }
            let root_name = imported
                .split('.')
                .next()
                .unwrap_or(imported.as_str())
                .to_string();
            let local_name = alias.clone().unwrap_or_else(|| root_name.clone());
            let bound_module = alias.as_ref().map_or(root_name, |_| imported.clone());

            Some(Reference {
                file: file.to_path_buf(),
                name: SymbolName::new(local_name),
                module: Some(ModuleName::new(imported)),
                resolved_name: None,
                resolved_module: Some(ModuleName::new(bound_module)),
                resolution: ReferenceResolution::Resolved,
                line: part.start_position().row + 1,
                kind: ReferenceKind::Import,
            })
        })
        .collect()
}

pub(super) fn from_import_references(node: Node, source: &str, file: &Path) -> Vec<Reference> {
    let module_node = node.child_by_field_name("module_name");
    let Some(module_node) = module_node else {
        return Vec::new();
    };
    let module = ModuleName::new(resolve_module(file, &node_text(module_node, source)));

    field_children(node, "name")
        .filter_map(|part| {
            let (imported, alias) = import_name_and_alias(part, source)?;
            if imported.is_empty() || imported == "*" {
                return None;
            }
            let local_name = alias.unwrap_or_else(|| imported.clone());

            Some(Reference {
                file: file.to_path_buf(),
                name: SymbolName::new(local_name),
                module: Some(module.clone()),
                resolved_name: Some(SymbolName::new(imported)),
                resolved_module: Some(module.clone()),
                resolution: ReferenceResolution::Resolved,
                line: part.start_position().row + 1,
                kind: ReferenceKind::FromImport,
            })
        })
        .collect()
}

fn import_name_and_alias(node: Node, source: &str) -> Option<(String, Option<String>)> {
    if node.kind() == "aliased_import" {
        let name = node.child_by_field_name("name")?;
        let alias = node.child_by_field_name("alias")?;
        return Some((node_text(name, source), Some(node_text(alias, source))));
    }

    Some((node_text(node, source), None))
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
