use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use tree_sitter::Node;

use crate::language::{
    BodyHash, ComplexityMetrics, FunctionSignatureFacts, LineRange, ParameterFacts, ParameterKind,
    ParsedFile, QualifiedName, Signature, Symbol, SymbolKind,
};

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
    let text = node.utf8_text(source.as_bytes()).unwrap_or_default();
    let signature = signature_text(node, source);
    let hash_text = match kind {
        SymbolKind::Class => class_direct_text(node, source),
        SymbolKind::Function | SymbolKind::Method => text.to_string(),
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

fn stable_hash(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn signature_text(node: Node, source: &str) -> String {
    let start = node.start_byte();
    let signature_end = node
        .child_by_field_name("body")
        .map(|body| body.start_byte())
        .and_then(|body_start| {
            source[start..body_start]
                .rfind(':')
                .map(|offset| start + offset + 1)
        })
        .unwrap_or_else(|| {
            let text = node.utf8_text(source.as_bytes()).unwrap_or_default();
            start + text.lines().next().unwrap_or_default().len()
        });

    source[start..signature_end].trim().to_string()
}

fn signature_facts(node: Node, signature: &str) -> FunctionSignatureFacts {
    let parameters = parameter_text(signature)
        .map(parse_parameters)
        .unwrap_or_default();

    FunctionSignatureFacts {
        is_async: signature.starts_with("async def "),
        parameters,
        return_annotation: return_annotation_text(node, signature),
    }
}

fn parameter_text(signature: &str) -> Option<&str> {
    let start = signature.find('(')? + 1;
    let end = signature.rfind(')')?;
    Some(&signature[start..end])
}

fn parse_parameters(parameters: &str) -> Vec<ParameterFacts> {
    let mut facts = Vec::new();
    let parts = split_top_level(parameters, ',');
    let slash_index = parts.iter().position(|part| part.trim() == "/");
    let mut keyword_only = false;

    for (index, raw) in parts.iter().enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        if raw == "/" {
            continue;
        }
        if raw == "*" {
            keyword_only = true;
            continue;
        }

        let (kind, text) = if let Some(name) = raw.strip_prefix("**") {
            (ParameterKind::KwArgs, name)
        } else if let Some(name) = raw.strip_prefix('*') {
            keyword_only = true;
            (ParameterKind::VarArgs, name)
        } else if slash_index.is_some_and(|slash_index| index < slash_index) {
            (ParameterKind::PositionalOnly, raw)
        } else if keyword_only {
            (ParameterKind::KeywordOnly, raw)
        } else {
            (ParameterKind::PositionalOrKeyword, raw)
        };

        let (without_default, has_default) = split_once_top_level(text, '=')
            .map(|(left, _)| (left.trim(), true))
            .unwrap_or((text.trim(), false));
        let (name, annotation) = split_once_top_level(without_default, ':')
            .map(|(left, right)| (left.trim(), Some(right.trim().to_string())))
            .unwrap_or((without_default.trim(), None));

        if name.is_empty() {
            continue;
        }

        facts.push(ParameterFacts {
            name: name.to_string(),
            kind,
            has_default,
            annotation,
        });
    }

    facts
}

fn return_annotation_text(_node: Node, signature: &str) -> Option<String> {
    let after_parameters = &signature[signature.rfind(')')? + 1..];
    let annotation = after_parameters
        .trim()
        .strip_prefix("->")?
        .trim()
        .trim_end_matches(':')
        .trim();

    if annotation.is_empty() {
        None
    } else {
        Some(annotation.to_string())
    }
}

fn split_once_top_level(text: &str, delimiter: char) -> Option<(&str, &str)> {
    let index = top_level_char_indices(text)
        .find(|(_, character)| *character == delimiter)
        .map(|(index, _)| index)?;
    Some((&text[..index], &text[index + delimiter.len_utf8()..]))
}

fn split_top_level(text: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;

    for (index, character) in top_level_char_indices(text) {
        if character == delimiter {
            parts.push(&text[start..index]);
            start = index + character.len_utf8();
        }
    }

    parts.push(&text[start..]);
    parts
}

fn top_level_char_indices(text: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    text.char_indices().filter_map(move |(index, character)| {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
                return None;
            }
            if character == '\\' {
                escaped = true;
                return None;
            }
            if character == active_quote {
                quote = None;
            }
            return None;
        }

        match character {
            '\'' | '"' => {
                quote = Some(character);
                None
            }
            '(' | '[' | '{' => {
                depth += 1;
                None
            }
            ')' | ']' | '}' => {
                depth = depth.saturating_sub(1);
                None
            }
            _ if depth == 0 => Some((index, character)),
            _ => None,
        }
    })
}

fn complexity_metrics(node: Node) -> ComplexityMetrics {
    let mut metrics = ComplexityMetrics {
        length_lines: node.end_position().row + 1 - node.start_position().row,
        ..ComplexityMetrics::default()
    };
    walk_complexity(node, 0, &mut metrics);
    metrics
}

fn walk_complexity(node: Node, nesting_depth: usize, metrics: &mut ComplexityMetrics) {
    let increments_depth = is_nesting_node(node);
    let current_depth = if increments_depth {
        nesting_depth + 1
    } else {
        nesting_depth
    };

    if increments_depth {
        metrics.max_nesting_depth = metrics.max_nesting_depth.max(current_depth);
    }

    match node.kind() {
        "if_statement" | "conditional_expression" => metrics.branch_count += 1,
        "for_statement" | "while_statement" => metrics.loop_count += 1,
        "boolean_operator" => metrics.boolean_operator_count += 1,
        "except_clause" | "except_group_clause" => metrics.exception_handler_count += 1,
        "match_statement" => metrics.match_count += 1,
        "with_statement" => metrics.with_count += 1,
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() {
            walk_complexity(child, current_depth, metrics);
        }
    }
}

fn is_nesting_node(node: Node) -> bool {
    matches!(
        node.kind(),
        "if_statement"
            | "for_statement"
            | "while_statement"
            | "try_statement"
            | "except_clause"
            | "except_group_clause"
            | "with_statement"
            | "match_statement"
            | "case_clause"
    )
}

fn class_direct_text(node: Node, source: &str) -> String {
    let mut excluded_ranges = Vec::new();
    collect_nested_symbol_definition_ranges(node, &mut excluded_ranges, true);
    excluded_ranges.sort_by_key(|(start, _)| *start);

    let mut direct = String::new();
    let mut cursor = node.start_byte();

    for (start, end) in excluded_ranges {
        if start > cursor {
            direct.push_str(&source[cursor..start]);
        }
        cursor = cursor.max(end);
    }

    if cursor < node.end_byte() {
        direct.push_str(&source[cursor..node.end_byte()]);
    }

    direct
}

fn collect_nested_symbol_definition_ranges(
    node: Node,
    ranges: &mut Vec<(usize, usize)>,
    skip_current: bool,
) {
    if !skip_current && is_symbol_definition_wrapper(node) {
        ranges.push((node.start_byte(), node.end_byte()));
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_nested_symbol_definition_ranges(child, ranges, false);
    }
}

fn is_symbol_definition_wrapper(node: Node) -> bool {
    matches!(node.kind(), "function_definition" | "class_definition")
        || (node.kind() == "decorated_definition" && contains_symbol_definition(node))
}

fn contains_symbol_definition(node: Node) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).any(|child| {
        matches!(child.kind(), "function_definition" | "class_definition")
            || contains_symbol_definition(child)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::language::LanguageAdapter;
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
                    parameter.annotation.as_deref()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("source", ParameterKind::PositionalOnly, false, None),
                (
                    "rows",
                    ParameterKind::PositionalOrKeyword,
                    false,
                    Some("list[dict[str, object]]")
                ),
                ("args", ParameterKind::VarArgs, false, None),
                ("strict", ParameterKind::KeywordOnly, true, Some("bool")),
                ("kwargs", ParameterKind::KwArgs, false, None),
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
                    parameter.annotation.as_deref()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("labels", true, None),
                ("mapping", true, Some("dict[str, tuple[int, int]]")),
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
