use std::path::Path;

use tree_sitter::Node;

use crate::language::{
    BodyHash, LineRange, ParsedFile, QualifiedName, Signature, Symbol, SymbolKind,
};

mod complexity;
mod fingerprint;
mod signature;

use complexity::complexity_metrics;
use fingerprint::{class_direct_text, function_body_fingerprint, stable_hash};
use signature::{signature_facts, signature_text};

pub(crate) fn extract_symbols(parsed: &ParsedFile) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut class_stack = Vec::new();
    walk_symbols(
        parsed.tree.root_node(),
        &parsed.source,
        &parsed.file,
        &mut class_stack,
        0,
        &mut symbols,
    );
    symbols
}

fn walk_symbols(
    node: Node,
    source: &str,
    file: &Path,
    class_stack: &mut Vec<String>,
    function_depth: usize,
    symbols: &mut Vec<Symbol>,
) {
    match node.kind() {
        "class_definition" => {
            if function_depth > 0 {
                return;
            }

            if let Some(name) = child_text(node, "identifier", source) {
                symbols.push(symbol_from_node(
                    node,
                    source,
                    file,
                    SymbolKind::Class,
                    qualified_name(class_stack, &name),
                ));

                class_stack.push(name);
                walk_named_children(node, source, file, class_stack, function_depth, symbols);
                class_stack.pop();
                return;
            }
        }
        "function_definition" => {
            if function_depth == 0
                && let Some(name) = child_text(node, "identifier", source)
            {
                let kind = if class_stack.is_empty() {
                    SymbolKind::Function
                } else {
                    SymbolKind::Method
                };
                symbols.push(symbol_from_node(
                    node,
                    source,
                    file,
                    kind,
                    qualified_name(class_stack, &name),
                ));
            }

            walk_named_children(node, source, file, class_stack, function_depth + 1, symbols);
            return;
        }
        _ => {}
    }

    walk_named_children(node, source, file, class_stack, function_depth, symbols);
}

fn walk_named_children(
    node: Node,
    source: &str,
    file: &Path,
    class_stack: &mut Vec<String>,
    function_depth: usize,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_symbols(child, source, file, class_stack, function_depth, symbols);
    }
}

fn symbol_from_node(
    node: Node,
    source: &str,
    file: &Path,
    kind: SymbolKind,
    qualified_name: QualifiedName,
) -> Symbol {
    let signature = signature_text(node, source);
    let hash_text = match kind {
        SymbolKind::Class => class_direct_text(node, source),
        SymbolKind::Function | SymbolKind::Method => function_body_fingerprint(node, source),
    };
    let signature_facts = match kind {
        SymbolKind::Function | SymbolKind::Method => Some(signature_facts(node, &signature)),
        SymbolKind::Class => None,
    };

    Symbol {
        file: file.to_path_buf(),
        qualified_name,
        kind,
        signature: Signature::new(signature),
        signature_facts,
        range: LineRange {
            start: node.start_position().row + 1,
            end: node.end_position().row + 1,
        },
        body_hash: BodyHash::new(stable_hash(&hash_text)),
        complexity: complexity_metrics(node),
    }
}

fn child_text(node: Node, kind: &str, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
        .and_then(|child| child.utf8_text(source.as_bytes()).ok())
        .map(ToOwned::to_owned)
}

fn qualified_name(class_stack: &[String], name: &str) -> QualifiedName {
    if class_stack.is_empty() {
        return QualifiedName::new(name);
    }

    QualifiedName::new(format!("{}.{}", class_stack.join("."), name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::language::{LanguageAdapter, ParameterKind};
    use crate::python::PythonAdapter;

    #[test]
    fn extracts_top_level_functions_classes_and_methods() {
        let source = r#"
def build_features(rows, *, strict=False):
    if strict:
        return rows
    return []

class User:
    def normalize_email(self):
        return self.email.lower()
"#;

        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(PathBuf::from("src/features.py"), source.to_string())
            .unwrap();
        let symbols = adapter.extract_symbols(&parsed).unwrap();

        let names = symbols
            .iter()
            .map(|symbol| (symbol.qualified_name.as_str(), symbol.kind))
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                ("build_features", SymbolKind::Function),
                ("User", SymbolKind::Class),
                ("User.normalize_email", SymbolKind::Method),
            ]
        );
    }

    #[test]
    fn captures_signature_line_range_and_body_hash() {
        let source = "def f(x):\n    return x + 1\n";
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(PathBuf::from("src/simple.py"), source.to_string())
            .unwrap();
        let symbols = adapter.extract_symbols(&parsed).unwrap();

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].signature, "def f(x):");
        assert_eq!(symbols[0].range, LineRange { start: 1, end: 2 });
        assert!(!symbols[0].body_hash.is_empty());
    }

    #[test]
    fn extracts_async_functions_as_functions() {
        let symbols = extract(
            r#"
async def fetch_user(user_id):
    return user_id
"#,
        );

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].qualified_name, "fetch_user");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
        assert_eq!(symbols[0].signature, "async def fetch_user(user_id):");
    }

    #[test]
    fn decorated_function_uses_underlying_function_name() {
        let symbols = extract(
            r#"
@cache
@trace("feature")
def build_features(rows):
    return rows
"#,
        );

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].qualified_name, "build_features");
        assert_eq!(symbols[0].signature, "def build_features(rows):");
    }

    #[test]
    fn extracts_static_class_and_property_methods_as_methods() {
        let symbols = extract(
            r#"
class User:
    @staticmethod
    def from_row(row):
        return User()

    @classmethod
    def anonymous(cls):
        return cls()

    @property
    def email(self):
        return "a@example.com"
"#,
        );

        let names = symbols
            .iter()
            .map(|symbol| (symbol.qualified_name.as_str(), symbol.kind))
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                ("User", SymbolKind::Class),
                ("User.from_row", SymbolKind::Method),
                ("User.anonymous", SymbolKind::Method),
                ("User.email", SymbolKind::Method),
            ]
        );
    }

    #[test]
    fn extracts_nested_classes_and_methods_with_qualified_names() {
        let symbols = extract(
            r#"
class Pipeline:
    class Step:
        def run(self):
            return "ok"

    def build(self):
        return self.Step()
"#,
        );

        let names = symbols
            .iter()
            .map(|symbol| (symbol.qualified_name.as_str(), symbol.kind))
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                ("Pipeline", SymbolKind::Class),
                ("Pipeline.Step", SymbolKind::Class),
                ("Pipeline.Step.run", SymbolKind::Method),
                ("Pipeline.build", SymbolKind::Method),
            ]
        );
    }

    #[test]
    fn captures_multiline_signature() {
        let symbols = extract(
            r#"
def build_features(
    rows,
    *,
    strict=False,
):
    return rows
"#,
        );

        assert_eq!(
            symbols[0].signature,
            "def build_features(\n    rows,\n    *,\n    strict=False,\n):"
        );
    }

    #[test]
    fn function_body_hash_ignores_signature_and_body_formatting() {
        let before = extract(
            r#"
def build(rows, *, strict=False):
    value = rows[0]
    return value
"#,
        );
        let after = extract(
            r#"
def build(
    rows,
    *,
    strict=False,
):
    value=rows[0]

    # Comment-only changes should not affect the syntax fingerprint.
    return value
"#,
        );

        assert_eq!(before[0].body_hash, after[0].body_hash);
    }

    #[test]
    fn extracts_structured_signature_facts() {
        let symbols = extract(
            r#"
async def build_features(
    source,
    /,
    rows: list[dict[str, object]],
    *args,
    strict: bool = False,
    **kwargs,
) -> list[str]:
    return []
"#,
        );

        let signature = symbols[0].signature_facts.as_ref().unwrap();

        assert!(signature.is_async);
        assert_eq!(signature.return_annotation, Some("list[str]".to_string()));
        assert_eq!(
            signature
                .parameters
                .iter()
                .map(|parameter| (
                    parameter.name.as_str(),
                    parameter.kind,
                    parameter.has_default,
                    parameter.default_value.as_deref(),
                    parameter.annotation.as_deref()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("source", ParameterKind::PositionalOnly, false, None, None),
                (
                    "rows",
                    ParameterKind::PositionalOrKeyword,
                    false,
                    None,
                    Some("list[dict[str, object]]")
                ),
                ("args", ParameterKind::VarArgs, false, None, None),
                (
                    "strict",
                    ParameterKind::KeywordOnly,
                    true,
                    Some("False"),
                    Some("bool")
                ),
                ("kwargs", ParameterKind::KwArgs, false, None, None),
            ]
        );
    }

    #[test]
    fn parses_defaults_with_commas_without_splitting_parameters() {
        let symbols = extract(
            r#"
def configure(
    labels=("a", "b"),
    mapping: dict[str, tuple[int, int]] = {"x": (1, 2)},
):
    return labels, mapping
"#,
        );

        let signature = symbols[0].signature_facts.as_ref().unwrap();

        assert_eq!(
            signature
                .parameters
                .iter()
                .map(|parameter| (
                    parameter.name.as_str(),
                    parameter.has_default,
                    parameter.default_value.as_deref(),
                    parameter.annotation.as_deref()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("labels", true, Some("(\"a\", \"b\")"), None),
                (
                    "mapping",
                    true,
                    Some("{\"x\": (1, 2)}"),
                    Some("dict[str, tuple[int, int]]")
                ),
            ]
        );
    }

    #[test]
    fn extracts_complexity_metrics_without_scoring_them() {
        let symbols = extract(
            r#"
def classify(rows, strict=False):
    with open("x") as handle:
        for row in rows:
            if row and (strict or row.ready):
                return handle.read()
    try:
        return None
    except ValueError:
        return "bad"
"#,
        );

        let complexity = &symbols[0].complexity;

        assert_eq!(complexity.length_lines, 9);
        assert_eq!(complexity.branch_count, 1);
        assert_eq!(complexity.loop_count, 1);
        assert_eq!(complexity.with_count, 1);
        assert_eq!(complexity.exception_handler_count, 1);
        assert_eq!(complexity.boolean_operator_count, 2);
        assert_eq!(complexity.max_nesting_depth, 3);
    }

    #[test]
    fn counts_match_and_conditional_expression_complexity() {
        let symbols = extract(
            r#"
def describe(value):
    match value:
        case 1:
            return "one"
        case _:
            return "many" if value else "none"
"#,
        );

        let complexity = &symbols[0].complexity;

        assert_eq!(complexity.match_count, 1);
        assert_eq!(complexity.branch_count, 1);
        assert_eq!(complexity.max_nesting_depth, 2);
    }

    #[test]
    fn ignores_nested_functions_by_contract() {
        let symbols = extract(
            r#"
def outer(value):
    def inner():
        return value
    return inner()
"#,
        );

        let names = symbols
            .iter()
            .map(|symbol| symbol.qualified_name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["outer"]);
    }

    #[test]
    fn class_hash_does_not_change_when_only_method_body_changes() {
        let before = extract(
            r#"
class Formatter:
    kind = "name"

    def format_name(self, name):
        return name.strip()
"#,
        );
        let after = extract(
            r#"
class Formatter:
    kind = "name"

    def format_name(self, name):
        return name.strip().title()
"#,
        );

        let before_class = before
            .iter()
            .find(|symbol| symbol.qualified_name == "Formatter")
            .unwrap();
        let after_class = after
            .iter()
            .find(|symbol| symbol.qualified_name == "Formatter")
            .unwrap();
        let before_method = before
            .iter()
            .find(|symbol| symbol.qualified_name == "Formatter.format_name")
            .unwrap();
        let after_method = after
            .iter()
            .find(|symbol| symbol.qualified_name == "Formatter.format_name")
            .unwrap();

        assert_eq!(before_class.body_hash, after_class.body_hash);
        assert_ne!(before_method.body_hash, after_method.body_hash);
    }

    #[test]
    fn class_hash_does_not_change_when_methods_are_reordered() {
        let before = extract(
            r#"
class Formatter:
    kind = "name"

    def first(self):
        return "first"

    def second(self):
        return "second"
"#,
        );
        let after = extract(
            r#"
class Formatter:
    kind = "name"

    def second(self):
        return "second"

    def first(self):
        return "first"
"#,
        );

        let before_class = before
            .iter()
            .find(|symbol| symbol.qualified_name == "Formatter")
            .unwrap();
        let after_class = after
            .iter()
            .find(|symbol| symbol.qualified_name == "Formatter")
            .unwrap();
        let before_methods = before
            .iter()
            .filter(|symbol| matches!(symbol.kind, SymbolKind::Method))
            .map(|symbol| symbol.qualified_name.as_str())
            .collect::<Vec<_>>();
        let after_methods = after
            .iter()
            .filter(|symbol| matches!(symbol.kind, SymbolKind::Method))
            .map(|symbol| symbol.qualified_name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(before_class.body_hash, after_class.body_hash);
        assert_eq!(before_methods, vec!["Formatter.first", "Formatter.second"]);
        assert_eq!(after_methods, vec!["Formatter.second", "Formatter.first"]);
    }

    #[test]
    fn class_hash_changes_when_direct_class_body_changes() {
        let before = extract(
            r#"
class Formatter:
    kind = "name"

    def format_name(self, name):
        return name.strip()
"#,
        );
        let after = extract(
            r#"
class Formatter:
    kind = "display_name"

    def format_name(self, name):
        return name.strip()
"#,
        );

        let before_class = before
            .iter()
            .find(|symbol| symbol.qualified_name == "Formatter")
            .unwrap();
        let after_class = after
            .iter()
            .find(|symbol| symbol.qualified_name == "Formatter")
            .unwrap();

        assert_ne!(before_class.body_hash, after_class.body_hash);
    }

    #[test]
    fn reports_parse_errors_without_blocking_symbol_extraction() {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("src/broken.py"),
                "def valid():\n    return 1\n\ndef broken(:\n".to_string(),
            )
            .unwrap();

        let symbols = adapter.extract_symbols(&parsed).unwrap();

        assert!(parsed.has_parse_errors);
        assert_eq!(symbols[0].qualified_name, "valid");
    }

    fn extract(source: &str) -> Vec<Symbol> {
        let adapter = PythonAdapter;
        let parsed = adapter
            .parse_file(
                PathBuf::from("src/example.py"),
                source.trim_start().to_string(),
            )
            .unwrap();
        adapter.extract_symbols(&parsed).unwrap()
    }
}
