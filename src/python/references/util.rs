use tree_sitter::Node;

pub(super) fn field_children<'a>(
    node: Node<'a>,
    field_name: &'static str,
) -> impl Iterator<Item = Node<'a>> {
    let mut cursor = node.walk();
    node.children_by_field_name(field_name, &mut cursor)
        .filter(|node| node.is_named())
        .collect::<Vec<_>>()
        .into_iter()
}

pub(super) fn node_text(node: Node, source: &str) -> String {
    node.utf8_text(source.as_bytes())
        .unwrap_or_default()
        .trim()
        .to_string()
}
