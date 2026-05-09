use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::language::{ModuleName, QualifiedName, Symbol, SymbolName};

#[derive(Debug, Clone, Default)]
pub(crate) struct ModuleIndex {
    symbols_by_module: BTreeMap<ModuleName, BTreeSet<SymbolName>>,
}

impl ModuleIndex {
    pub(crate) fn from_symbols(symbols: &[Symbol]) -> Self {
        let mut index = Self::default();

        for symbol in symbols {
            index
                .symbols_by_module
                .entry(module_name(&symbol.file))
                .or_default()
                .insert(top_level_name(&symbol.qualified_name));
        }

        index
    }

    pub(crate) fn contains_symbol(&self, module: &ModuleName, name: &SymbolName) -> bool {
        self.symbols_by_module
            .get(module)
            .is_some_and(|symbols| symbols.contains(name))
    }

    pub(crate) fn symbol_module(symbol: &Symbol) -> ModuleName {
        module_name(&symbol.file)
    }

    pub(crate) fn symbol_top_level_name(symbol: &Symbol) -> SymbolName {
        top_level_name(&symbol.qualified_name)
    }
}

fn module_name(path: &Path) -> ModuleName {
    let without_extension = path.with_extension("");
    let mut components = without_extension
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();

    if components.last() == Some(&"__init__") {
        components.pop();
    }

    ModuleName::new(components.join("."))
}

fn top_level_name(qualified_name: &QualifiedName) -> SymbolName {
    SymbolName::new(
        qualified_name
            .to_string()
            .split_once('.')
            .map(|(name, _)| name.to_string())
            .unwrap_or_else(|| qualified_name.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::language::{BodyHash, ComplexityMetrics, LineRange, Signature, SymbolKind};

    use super::*;

    #[test]
    fn indexes_top_level_symbols_by_module() {
        let index = ModuleIndex::from_symbols(&[
            symbol("pkg/features.py", "build_features"),
            symbol("pkg/features.py", "Formatter.format_name"),
            symbol("pkg/__init__.py", "exported"),
        ]);

        assert!(index.contains_symbol(
            &ModuleName::new("pkg.features"),
            &SymbolName::new("build_features")
        ));
        assert!(index.contains_symbol(
            &ModuleName::new("pkg.features"),
            &SymbolName::new("Formatter")
        ));
        assert!(index.contains_symbol(&ModuleName::new("pkg"), &SymbolName::new("exported")));
        assert!(!index.contains_symbol(
            &ModuleName::new("other.features"),
            &SymbolName::new("build_features")
        ));
    }

    fn symbol(file: &str, name: &str) -> Symbol {
        Symbol {
            file: PathBuf::from(file),
            qualified_name: QualifiedName::new(name),
            kind: SymbolKind::Function,
            signature: Signature::new(format!("def {name}():")),
            signature_facts: None,
            range: LineRange { start: 1, end: 2 },
            body_hash: BodyHash::new("hash"),
            complexity: ComplexityMetrics::default(),
        }
    }
}
