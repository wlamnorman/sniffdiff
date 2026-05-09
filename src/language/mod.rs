mod types;

use std::path::PathBuf;

use anyhow::Result;

pub(crate) use types::*;

pub(crate) trait LanguageAdapter {
    fn language_name(&self) -> &'static str;
    fn file_extensions(&self) -> &'static [&'static str];
    fn parse_file(&self, file: PathBuf, source: String) -> Result<ParsedFile>;
    fn extract_symbols(&self, parsed: &ParsedFile) -> Result<Vec<Symbol>>;
    fn extract_references(&self, parsed: &ParsedFile) -> Result<Vec<Reference>>;
}
