mod diff;
pub(crate) mod facts;
mod filter;
mod modules;
mod references;
mod signals;
mod test_facts;

use std::collections::BTreeSet;

use anyhow::{Result, bail};

pub(crate) use facts::{Analysis, AnalysisOptions};

use crate::analysis::diff::diff_symbols;
use crate::analysis::facts::Snapshot;
use crate::analysis::filter::FileFilter;
use crate::analysis::modules::ModuleIndex;
use crate::analysis::references::{
    ReferenceSnapshot, attach_reference_facts, attach_test_reference_facts,
};
use crate::analysis::signals::attach_review_signals;
use crate::analysis::test_facts::derive_test_facts;
use crate::git::GitBackend;
use crate::language::{LanguageAdapter, Reference, Symbol};
use crate::timing::TimingRecorder;

pub(crate) fn analyze(
    git: &impl GitBackend,
    adapter: &impl LanguageAdapter,
    base: &str,
    head: &str,
    options: AnalysisOptions,
) -> Result<Analysis> {
    analyze_inner(git, adapter, base, head, options, None)
}

pub(crate) fn analyze_recorded(
    git: &impl GitBackend,
    adapter: &impl LanguageAdapter,
    base: &str,
    head: &str,
    options: AnalysisOptions,
    timings: &mut TimingRecorder,
) -> Result<Analysis> {
    analyze_inner(git, adapter, base, head, options, Some(timings))
}

fn analyze_inner(
    git: &impl GitBackend,
    adapter: &impl LanguageAdapter,
    base: &str,
    head: &str,
    options: AnalysisOptions,
    mut timings: Option<&mut TimingRecorder>,
) -> Result<Analysis> {
    let changed_files = time(&mut timings, "git_changed_files", || {
        git.changed_files(base, head)
    })?;
    let (before, references_before) = time(&mut timings, "before_snapshot", || {
        parse_snapshot_with_references(git, adapter, base)
    })?;
    let (after, references_after) = time(&mut timings, "after_snapshot", || {
        parse_snapshot_with_references(git, adapter, head)
    })?;
    let (tests_after, test_references_after) = time(&mut timings, "test_snapshot", || {
        parse_test_snapshot_with_references(git, adapter, head)
    })?;
    time(&mut timings, "parse_error_validation", || {
        validate_parse_errors(&[&before, &after, &tests_after], options)
    })?;
    let before_module_index = ModuleIndex::from_symbols(&before.symbols);
    let after_module_index = ModuleIndex::from_symbols(&after.symbols);
    let mut symbol_changes = time(&mut timings, "symbol_diff", || {
        Ok(diff_symbols(
            &before.symbols,
            &after.symbols,
            &changed_files,
        ))
    })?;
    time(&mut timings, "reference_facts", || {
        attach_reference_facts(
            &mut symbol_changes,
            ReferenceSnapshot {
                references: &references_before,
                module_index: &before_module_index,
                symbols: &before.symbols,
            },
            ReferenceSnapshot {
                references: &references_after,
                module_index: &after_module_index,
                symbols: &after.symbols,
            },
            &changed_files,
        );
        Ok(())
    })?;
    time(&mut timings, "test_reference_facts", || {
        attach_test_reference_facts(
            &mut symbol_changes,
            ReferenceSnapshot {
                references: &test_references_after,
                module_index: &after_module_index,
                symbols: &tests_after.symbols,
            },
            &changed_files,
        );
        Ok(())
    })?;
    let test_facts = time(&mut timings, "test_facts", || {
        Ok(derive_test_facts(
            &changed_files,
            &symbol_changes,
            &tests_after,
        ))
    })?;
    time(&mut timings, "review_signals", || {
        attach_review_signals(&mut symbol_changes, &test_facts);
        Ok(())
    })?;

    Ok(Analysis {
        base: base.to_string(),
        head: head.to_string(),
        language: adapter.language_name().to_string(),
        changed_files,
        before,
        after,
        symbol_changes,
        references_before,
        references_after,
        test_facts,
    })
}

fn time<T>(
    timings: &mut Option<&mut TimingRecorder>,
    name: &'static str,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    if let Some(timings) = timings.as_deref_mut() {
        timings.time_result(name, f)
    } else {
        f()
    }
}

fn validate_parse_errors(snapshots: &[&Snapshot], options: AnalysisOptions) -> Result<()> {
    if options.allow_parse_errors {
        return Ok(());
    }

    let parse_error_files = snapshots
        .iter()
        .flat_map(|snapshot| snapshot.parse_error_files.iter())
        .collect::<BTreeSet<_>>();

    if parse_error_files.is_empty() {
        return Ok(());
    }

    let files = parse_error_files
        .into_iter()
        .map(|path| format!("- {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n");

    bail!(
        "Python parse errors found.\n\n\
sniffdiff requires syntactically valid Python by default because review facts may be incomplete.\n\n\
Files with parse errors:\n{}\n\n\
Re-run with --allow-parse-errors to get partial analysis.",
        files
    )
}

fn parse_snapshot_with_references(
    git: &impl GitBackend,
    adapter: &impl LanguageAdapter,
    git_ref: &str,
) -> Result<(Snapshot, Vec<Reference>)> {
    let mut snapshot = SnapshotBuilder::new(git_ref);
    let mut references = Vec::new();
    let filter = FileFilter::default();

    for path in git.list_files_at_ref(git_ref, adapter.file_extensions())? {
        snapshot.consider_file();
        if filter.should_skip(&path) {
            snapshot.skip_file();
            continue;
        }

        if let Some(source) = git.read_file_at_ref(git_ref, &path)? {
            let parsed = adapter.parse_file(path, source)?;
            let symbols = adapter.extract_symbols(&parsed)?;
            references.extend(adapter.extract_references(&parsed)?);
            snapshot.add_parsed_file(&parsed, symbols);
        }
    }

    Ok((snapshot.finish(), references))
}

fn parse_test_snapshot_with_references(
    git: &impl GitBackend,
    adapter: &impl LanguageAdapter,
    git_ref: &str,
) -> Result<(Snapshot, Vec<Reference>)> {
    let mut snapshot = SnapshotBuilder::new(git_ref);
    let mut references = Vec::new();

    for path in git.list_files_at_ref(git_ref, adapter.file_extensions())? {
        if !filter::is_test_path(&path) {
            continue;
        }

        snapshot.consider_file();
        if let Some(source) = git.read_file_at_ref(git_ref, &path)? {
            let parsed = adapter.parse_file(path, source)?;
            let symbols = adapter.extract_symbols(&parsed)?;
            references.extend(adapter.extract_references(&parsed)?);
            snapshot.add_parsed_file(&parsed, symbols);
        }
    }

    Ok((snapshot.finish(), references))
}

struct SnapshotBuilder {
    git_ref: String,
    files_considered: usize,
    files_skipped: usize,
    files_parsed: usize,
    files_with_parse_errors: usize,
    parse_error_files: Vec<std::path::PathBuf>,
    symbols: Vec<Symbol>,
}

impl SnapshotBuilder {
    fn new(git_ref: &str) -> Self {
        Self {
            git_ref: git_ref.to_string(),
            files_considered: 0,
            files_skipped: 0,
            files_parsed: 0,
            files_with_parse_errors: 0,
            parse_error_files: Vec::new(),
            symbols: Vec::new(),
        }
    }

    fn consider_file(&mut self) {
        self.files_considered += 1;
    }

    fn skip_file(&mut self) {
        self.files_skipped += 1;
    }

    fn add_parsed_file(&mut self, parsed: &crate::language::ParsedFile, symbols: Vec<Symbol>) {
        self.files_parsed += 1;
        if parsed.has_parse_errors {
            self.files_with_parse_errors += 1;
            self.parse_error_files.push(parsed.file.clone());
        }
        self.symbols.extend(symbols);
    }

    fn finish(self) -> Snapshot {
        Snapshot {
            git_ref: self.git_ref,
            files_considered: self.files_considered,
            files_skipped: self.files_skipped,
            files_parsed: self.files_parsed,
            files_with_parse_errors: self.files_with_parse_errors,
            parse_error_files: self.parse_error_files,
            symbols: self.symbols,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::analysis::filter::is_test_path;

    #[test]
    fn identifies_test_paths_for_default_skipping() {
        assert!(is_test_path(Path::new("tests/test_features.py")));
        assert!(is_test_path(Path::new("test/test_features.py")));
        assert!(is_test_path(Path::new("src/test_features.py")));
        assert!(is_test_path(Path::new("src/features_test.py")));
        assert!(!is_test_path(Path::new("src/features.py")));
        assert!(!is_test_path(Path::new("src/contest.py")));
    }
}
