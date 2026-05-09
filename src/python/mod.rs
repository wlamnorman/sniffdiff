mod references;
mod symbols;

use std::path::PathBuf;

use anyhow::{Context, Result};
use tree_sitter::{Parser, Tree};

use crate::language::{LanguageAdapter, ParsedFile, Reference, Symbol};

#[derive(Debug, Default)]
pub(crate) struct PythonAdapter;

impl LanguageAdapter for PythonAdapter {
    fn language_name(&self) -> &'static str {
        "python"
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn parse_file(&self, file: PathBuf, source: String) -> Result<ParsedFile> {
        let tree =
            parse_tree(&source).with_context(|| format!("failed to parse {}", file.display()))?;

        Ok(ParsedFile {
            file,
            source,
            has_parse_errors: tree.root_node().has_error(),
            tree,
        })
    }

    fn extract_symbols(&self, parsed: &ParsedFile) -> Result<Vec<Symbol>> {
        Ok(symbols::extract_symbols(parsed))
    }

    fn extract_references(&self, parsed: &ParsedFile) -> Result<Vec<Reference>> {
        Ok(references::extract_references(parsed))
    }
}

fn parse_tree(source: &str) -> Result<Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .context("failed to initialize tree-sitter-python")?;

    parser
        .parse(source, None)
        .context("tree-sitter returned no tree")
}
