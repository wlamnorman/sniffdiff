use tree_sitter::Node;

use crate::language::ComplexityMetrics;

pub(super) fn complexity_metrics(node: Node) -> ComplexityMetrics {
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
