use tree_sitter::Node;

use crate::language::{FunctionSignatureFacts, ParameterFacts, ParameterKind};

pub(super) fn signature_text(node: Node, source: &str) -> String {
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

pub(super) fn signature_facts(_node: Node, signature: &str) -> FunctionSignatureFacts {
    let parameters = parameter_text(signature)
        .map(parse_parameters)
        .unwrap_or_default();

    FunctionSignatureFacts {
        is_async: signature.starts_with("async def "),
        parameters,
        return_annotation: return_annotation_text(signature),
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

        let (without_default, default_value) = split_once_top_level(text, '=')
            .map(|(left, right)| (left.trim(), Some(normalize_signature_fragment(right))))
            .unwrap_or((text.trim(), None));
        let (name, annotation) = split_once_top_level(without_default, ':')
            .map(|(left, right)| (left.trim(), Some(normalize_signature_fragment(right))))
            .unwrap_or((without_default.trim(), None));

        if name.is_empty() {
            continue;
        }

        facts.push(ParameterFacts {
            name: name.to_string(),
            kind,
            has_default: default_value.is_some(),
            default_value,
            annotation,
        });
    }

    facts
}

fn return_annotation_text(signature: &str) -> Option<String> {
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
        Some(normalize_signature_fragment(annotation))
    }
}

fn normalize_signature_fragment(fragment: &str) -> String {
    fragment.split_whitespace().collect::<Vec<_>>().join(" ")
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
