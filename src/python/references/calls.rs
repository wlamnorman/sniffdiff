use std::path::Path;

use tree_sitter::Node;

use crate::language::{ModuleName, Reference, ReferenceKind, ReferenceResolution, SymbolName};
use crate::python::references::scopes::ImportScopes;
use crate::python::references::util::node_text;

pub(super) fn call_reference(
    node: Node,
    source: &str,
    file: &Path,
    imports: &ImportScopes,
) -> Option<Reference> {
    let function = node.child_by_field_name("function")?;
    let line = function.start_position().row + 1;

    match function.kind() {
        "identifier" => {
            let name = SymbolName::new(node_text(function, source));
            let resolved = imports.symbol(&name);
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
            let resolved = imports.resolve_module_object(module);
            let resolution = if resolved.is_some() {
                ReferenceResolution::Resolved
            } else {
                ReferenceResolution::Unresolved
            };
            Some(Reference {
                file: file.to_path_buf(),
                name: SymbolName::new(name),
                module: Some(ModuleName::new(module)),
                resolved_name: Some(SymbolName::new(name)),
                resolved_module: resolved.or_else(|| Some(ModuleName::new(module))),
                resolution,
                line,
                kind: ReferenceKind::Attribute,
            })
        }
        _ => None,
    }
}
