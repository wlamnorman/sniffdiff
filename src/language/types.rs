use std::path::PathBuf;

use serde::Serialize;
use tree_sitter::Tree;

macro_rules! impl_display {
    ($type:ty) => {
        impl std::fmt::Display for $type {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct QualifiedName(String);

impl QualifiedName {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn short_name(&self) -> SymbolName {
        self.0
            .rsplit_once('.')
            .map(|(_, name)| SymbolName::new(name))
            .unwrap_or_else(|| SymbolName::new(self.0.clone()))
    }
}

impl_display!(QualifiedName);

impl PartialEq<&str> for QualifiedName {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct SymbolName(String);

impl SymbolName {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl_display!(SymbolName);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct ModuleName(String);

impl ModuleName {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl_display!(ModuleName);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct Signature(String);

impl Signature {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl_display!(Signature);

impl PartialEq<&str> for Signature {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct BodyHash(String);

impl BodyHash {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl_display!(BodyHash);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SymbolKind {
    Function,
    Method,
    Class,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolKind::Function => formatter.write_str("function"),
            SymbolKind::Method => formatter.write_str("method"),
            SymbolKind::Class => formatter.write_str("class"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct LineRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Symbol {
    pub(crate) file: PathBuf,
    pub(crate) qualified_name: QualifiedName,
    pub(crate) kind: SymbolKind,
    pub(crate) signature: Signature,
    pub(crate) signature_facts: Option<FunctionSignatureFacts>,
    pub(crate) range: LineRange,
    pub(crate) body_hash: BodyHash,
    pub(crate) complexity: ComplexityMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FunctionSignatureFacts {
    pub(crate) is_async: bool,
    pub(crate) parameters: Vec<ParameterFacts>,
    pub(crate) return_annotation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ParameterFacts {
    pub(crate) name: String,
    pub(crate) kind: ParameterKind,
    pub(crate) has_default: bool,
    pub(crate) annotation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ParameterKind {
    PositionalOnly,
    PositionalOrKeyword,
    KeywordOnly,
    VarArgs,
    KwArgs,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub(crate) struct ComplexityMetrics {
    pub(crate) length_lines: usize,
    pub(crate) branch_count: usize,
    pub(crate) loop_count: usize,
    pub(crate) boolean_operator_count: usize,
    pub(crate) exception_handler_count: usize,
    pub(crate) match_count: usize,
    pub(crate) with_count: usize,
    pub(crate) max_nesting_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(dead_code)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReferenceKind {
    Import,
    FromImport,
    Call,
    Attribute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReferenceResolution {
    Resolved,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Reference {
    pub(crate) file: PathBuf,
    pub(crate) name: SymbolName,
    pub(crate) module: Option<ModuleName>,
    pub(crate) resolved_name: Option<SymbolName>,
    pub(crate) resolved_module: Option<ModuleName>,
    pub(crate) resolution: ReferenceResolution,
    pub(crate) line: usize,
    pub(crate) kind: ReferenceKind,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedFile {
    pub(crate) file: PathBuf,
    pub(crate) source: String,
    pub(crate) tree: Tree,
    pub(crate) has_parse_errors: bool,
}
