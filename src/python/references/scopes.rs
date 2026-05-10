use std::collections::BTreeMap;

use crate::language::{ModuleName, SymbolName};

#[derive(Debug, Default)]
pub(super) struct ImportScopes {
    scopes: Vec<ImportScope>,
}

impl ImportScopes {
    pub(super) fn enter_scope(&mut self, kind: ScopeKind) {
        self.scopes.push(ImportScope {
            kind,
            bindings: ImportBindings::default(),
        });
    }

    pub(super) fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    pub(super) fn insert_module(
        &mut self,
        local_name: SymbolName,
        bound_module: ModuleName,
        imported_module: ModuleName,
    ) {
        self.current_scope().modules.insert(
            local_name,
            ModuleBinding {
                bound_module,
                imported_module,
            },
        );
    }

    pub(super) fn insert_symbol(
        &mut self,
        local_name: SymbolName,
        module: ModuleName,
        name: SymbolName,
    ) {
        self.current_scope()
            .symbols
            .insert(local_name, (module, name));
    }

    pub(super) fn symbol(&self, name: &SymbolName) -> Option<&(ModuleName, SymbolName)> {
        self.visible_scopes()
            .find_map(|scope| scope.symbols.get(name))
    }

    pub(super) fn resolve_module_object(&self, object: &str) -> Option<ModuleName> {
        let parts = object.split('.').collect::<Vec<_>>();

        for prefix_len in (1..=parts.len()).rev() {
            let local_name = SymbolName::new(parts[..prefix_len].join("."));
            let suffix = &parts[prefix_len..];

            if let Some(binding) = self.module(&local_name) {
                let candidate = join_module(binding.bound_module.as_str(), suffix);
                if suffix.is_empty()
                    || candidate == binding.imported_module.as_str()
                    || candidate.starts_with(&format!("{}.", binding.imported_module.as_str()))
                {
                    return Some(ModuleName::new(candidate));
                }
            }

            if let Some((module, name)) = self.symbol(&local_name) {
                let imported_module = join_module(module.as_str(), &[name.as_str()]);
                let candidate = join_module(&imported_module, suffix);
                return Some(ModuleName::new(candidate));
            }
        }

        None
    }

    fn module(&self, name: &SymbolName) -> Option<&ModuleBinding> {
        self.visible_scopes()
            .find_map(|scope| scope.modules.get(name))
    }

    fn current_scope(&mut self) -> &mut ImportBindings {
        if self.scopes.is_empty() {
            self.enter_scope(ScopeKind::Module);
        }

        &mut self.scopes.last_mut().expect("scope must exist").bindings
    }

    fn visible_scopes(&self) -> impl Iterator<Item = &ImportBindings> {
        let innermost_function = self
            .scopes
            .iter()
            .rposition(|scope| scope.kind == ScopeKind::Function);

        self.scopes
            .iter()
            .enumerate()
            .rev()
            .filter(move |(index, scope)| {
                !matches!(innermost_function, Some(function_index) if scope.kind == ScopeKind::Class && *index < function_index)
            })
            .map(|(_, scope)| &scope.bindings)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScopeKind {
    Module,
    Class,
    Function,
}

#[derive(Debug)]
struct ImportScope {
    kind: ScopeKind,
    bindings: ImportBindings,
}

#[derive(Debug, Default)]
struct ImportBindings {
    modules: BTreeMap<SymbolName, ModuleBinding>,
    symbols: BTreeMap<SymbolName, (ModuleName, SymbolName)>,
}

#[derive(Debug)]
struct ModuleBinding {
    bound_module: ModuleName,
    imported_module: ModuleName,
}

fn join_module(base: &str, suffix: &[&str]) -> String {
    if suffix.is_empty() {
        return base.to_string();
    }

    format!("{base}.{}", suffix.join("."))
}
