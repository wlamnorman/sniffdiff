use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use tree_sitter::Node;

pub(super) fn stable_hash(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(super) fn function_body_fingerprint(node: Node, source: &str) -> String {
    node.child_by_field_name("body")
        .map(|body| syntax_fingerprint(body, source))
        .unwrap_or_else(|| {
            node.utf8_text(source.as_bytes())
                .unwrap_or_default()
                .to_string()
        })
}

pub(super) fn class_direct_text(node: Node, source: &str) -> String {
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

fn syntax_fingerprint(node: Node, source: &str) -> String {
    if node.kind() == "comment" {
        return String::new();
    }

    let mut cursor = node.walk();
    let children = node
        .children(&mut cursor)
        .map(|child| syntax_fingerprint(child, source))
        .filter(|fingerprint| !fingerprint.is_empty())
        .collect::<Vec<_>>();

    if children.is_empty() {
        let text = node.utf8_text(source.as_bytes()).unwrap_or_default().trim();
        if text.is_empty() {
            node.kind().to_string()
        } else {
            format!("{}:{text}", node.kind())
        }
    } else {
        format!("{}({})", node.kind(), children.join(","))
    }
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
