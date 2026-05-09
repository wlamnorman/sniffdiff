use std::fs;
use std::path::Path;
use std::process::Command;

use std::collections::BTreeMap;

use serde_json::Value;
use sniffdiff::{
    PublicAnalysisOptions, ReportLimit, analyze_repo_json, analyze_repo_report,
    analyze_repo_report_verbose,
};
use tempfile::TempDir;

#[test]
fn analyzes_symbols_before_and_after_from_real_git_refs() {
    let repo = TestRepo::new();
    repo.write(
        "src/features.py",
        r#"
def build_features(rows):
    return [_normalize_row(row) for row in rows]

def _normalize_row(row):
    return row

def removed():
    return 1

class Formatter:
    kind = "name"

    def format_name(self, name):
        return name.strip()
"#,
    );
    repo.write(
        "src/legacy.py",
        r#"
def stable_helper():
    return 1
"#,
    );
    repo.write(
        "src/train.py",
        r#"
from src.features import build_features

def train(rows):
    return build_features(rows)
"#,
    );
    repo.write(
        "src/consumer.py",
        r#"
from src.features import build_features

def count(rows):
    return len(build_features(rows))
"#,
    );
    repo.write(
        "tests/test_features.py",
        r#"
from src.features import build_features

def test_build_features():
    assert build_features([]) == []
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.git(&["mv", "src/legacy.py", "src/new_path.py"]);

    repo.write(
        "src/features.py",
        r#"
def build_features(rows, *, strict=False):
    values = []
    for row in rows:
        values.append(_normalize_row(row, strict=strict))
    return values

def _normalize_row(row, *, strict=False):
    if strict and row is None:
        raise ValueError("missing row")
    return row

def added():
    return 2

class Formatter:
    kind = "name"

    def format_name(self, name, *, uppercase=False):
        value = name.strip()
        if uppercase:
            return value.upper()
        return value
"#,
    );
    repo.write(
        "src/train.py",
        r#"
from src.features import build_features

def train(rows):
    values = build_features(rows)
    return len(values)
"#,
    );
    repo.write(
        "src/broken.py",
        r#"
def valid_symbol():
    return 1

def broken(:
"#,
    );
    repo.write(
        "tests/test_features.py",
        r#"
from src.features import build_features

def test_build_features():
    assert build_features([]) == []

def test_build_features_strict():
    assert build_features([], strict=True) == []
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let error =
        analyze_repo_json(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap_err();
    assert!(error.to_string().contains("Python parse errors found"));
    assert!(error.to_string().contains("src/broken.py"));
    assert!(error.to_string().contains("--allow-parse-errors"));

    let partial_report = analyze_repo_report(
        repo.path(),
        &base,
        &head,
        PublicAnalysisOptions {
            allow_parse_errors: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(partial_report.contains("parse_errors: 1 files"));

    let analysis: Value = serde_json::from_str(
        &analyze_repo_json(
            repo.path(),
            &base,
            &head,
            PublicAnalysisOptions {
                allow_parse_errors: true,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(analysis["before"]["files_parsed"], 4);
    assert_eq!(analysis["after"]["files_parsed"], 5);
    assert_eq!(analysis["before"]["files_skipped"], 1);
    assert_eq!(analysis["after"]["files_skipped"], 1);
    assert_eq!(analysis["before"]["files_with_parse_errors"], 0);
    assert_eq!(analysis["after"]["files_with_parse_errors"], 1);

    let changes = changes_by_name(&analysis);

    assert_eq!(analysis["schema_version"], 1);
    assert_top_level_json_shape(&analysis);
    assert_eq!(analysis["before"]["symbol_count"], 8);
    assert_eq!(analysis["after"]["symbol_count"], 10);
    assert_kinds(changes["added"], &["added"]);
    assert_kinds(changes["removed"], &["deleted"]);
    assert_kinds(
        changes["build_features"],
        &["body_changed", "signature_changed"],
    );
    assert_kinds(
        changes["Formatter.format_name"],
        &["body_changed", "signature_changed"],
    );
    assert_kinds(changes["stable_helper"], &["path_changed"]);
    assert_kinds(changes["valid_symbol"], &["added"]);

    assert!(!changes.contains_key("Formatter"));
    assert!(!changes.contains_key("test_build_features_strict"));

    let build_features = changes["build_features"];
    assert_symbol_change_json_shape(build_features);
    assert_eq!(build_features["kind"], "function");
    assert_eq!(build_features["visibility"], "publicish");
    assert_eq!(
        string_array(&build_features["signature_change"]["parameters_added"]),
        vec!["strict"]
    );
    assert_eq!(
        build_features["signature_change"]["parameters_reordered"],
        false
    );
    assert_eq!(build_features["complexity_delta"]["loop_count_delta"], 1);
    assert_eq!(build_features["references_after"]["count"], 4);
    assert_eq!(build_features["references_before"]["count"], 4);
    assert_eq!(build_features["reference_delta"]["count_delta"], 0);
    assert_eq!(build_features["references_after"]["from_import_count"], 2);
    assert_eq!(build_features["references_after"]["direct_call_count"], 2);
    assert_eq!(build_features["references_after"]["import_count"], 0);
    assert_eq!(
        build_features["references_after"]["attribute_call_count"],
        0
    );
    assert_eq!(
        string_array(&build_features["references_after"]["unchanged_files"]),
        vec!["src/consumer.py"]
    );
    assert_eq!(
        string_array(&build_features["references_after"]["changed_files"]),
        vec!["src/train.py"]
    );
    assert_eq!(
        string_array(&build_features["reference_delta"]["unchanged_files"]),
        vec!["src/consumer.py", "src/train.py"]
    );
    let matched_references = build_features["references_after"]["matched_references"]
        .as_array()
        .unwrap();
    assert!(matched_references.iter().any(|reference| {
        reference["file"] == "src/consumer.py"
            && reference["line"] == 4
            && reference["kind"] == "call"
            && reference["match_source"] == "resolved_import"
            && reference["changed_file"] == false
    }));
    assert_eq!(build_features["test_references_after"]["count"], 3);
    assert_eq!(
        build_features["test_references_after"]["direct_call_count"],
        2
    );
    assert_eq!(
        string_array(&build_features["test_references_after"]["changed_files"]),
        vec!["tests/test_features.py"]
    );
    assert!(
        build_features["test_references_after"]["matched_references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| {
                reference["file"] == "tests/test_features.py"
                    && reference["caller_symbol"] == "test_build_features_strict"
                    && reference["kind"] == "call"
                    && reference["changed_file"] == true
            })
    );
    assert_eq!(
        string_array(&build_features["review_signals"]),
        vec![
            "public_signature_changed",
            "signature_changed_with_unchanged_callers",
            "complexity_increased",
        ]
    );
    assert_eq!(
        string_array(&analysis["test_facts"]["changed_test_files"]),
        vec!["tests/test_features.py"]
    );
    assert_eq!(
        string_array(&analysis["test_facts"]["production_files_with_nearby_test_changes"]),
        vec!["src/features.py"]
    );

    let private_helper = changes["_normalize_row"];
    assert_eq!(private_helper["visibility"], "private");
}

#[test]
fn analyzes_package_repo_with_aliases_relative_imports_and_test_facts() {
    let repo = TestRepo::new();
    repo.write("pkg/__init__.py", "");
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows):
    return [row for row in rows]


def clean_rows(rows):
    return list(rows)
"#,
    );
    repo.write(
        "pkg/runner.py",
        r#"
from .features import build_features


def run(rows):
    return build_features(rows)
"#,
    );
    repo.write(
        "pkg/sub/consumer.py",
        r#"
from ..features import build_features as make_features


def consume(rows):
    return make_features(rows)
"#,
    );
    repo.write(
        "pkg/attribute_consumer.py",
        r#"
import pkg.features as features


def consume(rows):
    return features.build_features(rows)
"#,
    );
    repo.write(
        "pkg/unresolved_consumer.py",
        r#"
def consume(rows):
    return build_features(rows)
"#,
    );
    repo.write(
        "tests/test_features.py",
        r#"
from pkg.features import build_features


def test_build_features():
    assert build_features([]) == []
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False):
    values = []
    for row in rows:
        if strict and row is None:
            raise ValueError("missing row")
        values.append(row)
    return values


def clean_rows(rows):
    values = []
    for row in rows:
        if row is not None:
            values.append(row)
    return values
"#,
    );
    repo.write(
        "tests/test_features.py",
        r#"
from pkg.features import build_features


def test_build_features():
    assert build_features([]) == []


def test_build_features_strict():
    assert build_features([], strict=True) == []
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let analysis: Value = serde_json::from_str(
        &analyze_repo_json(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap(),
    )
    .unwrap();
    let changes = changes_by_name(&analysis);
    let build_features = changes["build_features"];

    assert_eq!(analysis["schema_version"], 1);
    assert_eq!(analysis["before"]["files_parsed"], 6);
    assert_eq!(analysis["after"]["files_parsed"], 6);
    assert_eq!(analysis["before"]["files_skipped"], 1);
    assert_eq!(analysis["after"]["files_skipped"], 1);
    assert_kinds(build_features, &["body_changed", "signature_changed"]);
    assert_eq!(
        string_array(&build_features["signature_change"]["parameters_added"]),
        vec!["strict"]
    );
    assert_eq!(build_features["references_after"]["count"], 6);
    assert_eq!(build_features["references_before"]["count"], 6);
    assert_eq!(build_features["reference_delta"]["count_delta"], 0);
    assert_eq!(build_features["references_after"]["resolved_count"], 5);
    assert_eq!(build_features["references_after"]["unresolved_count"], 1);
    assert_eq!(build_features["test_references_after"]["count"], 3);
    assert_eq!(
        build_features["test_references_after"]["direct_call_count"],
        2
    );
    assert_eq!(
        string_array(&build_features["test_references_after"]["changed_files"]),
        vec!["tests/test_features.py"]
    );
    assert_eq!(
        string_array(&build_features["references_after"]["unchanged_files"]),
        vec![
            "pkg/attribute_consumer.py",
            "pkg/runner.py",
            "pkg/sub/consumer.py",
            "pkg/unresolved_consumer.py",
        ]
    );
    assert_eq!(
        string_array(&build_features["review_signals"]),
        vec![
            "public_signature_changed",
            "signature_changed_with_unchanged_callers",
            "complexity_increased",
        ]
    );
    assert_eq!(
        string_array(&analysis["test_facts"]["changed_test_files"]),
        vec!["tests/test_features.py"]
    );
    assert_eq!(
        string_array(&analysis["test_facts"]["production_files_with_nearby_test_changes"]),
        vec!["pkg/features.py"]
    );

    let references = analysis["references_after"].as_array().unwrap();
    assert!(references.iter().any(|reference| {
        reference["file"] == "pkg/sub/consumer.py"
            && reference["name"] == "make_features"
            && reference["resolved_module"] == "pkg.features"
            && reference["resolved_name"] == "build_features"
            && reference["resolution"] == "resolved"
    }));
    assert!(
        build_features["references_after"]["matched_references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| {
                reference["file"] == "pkg/sub/consumer.py"
                    && reference["line"] == 5
                    && reference["match_source"] == "resolved_import"
                    && reference["changed_file"] == false
            })
    );
    assert!(references.iter().any(|reference| {
        reference["file"] == "pkg/unresolved_consumer.py"
            && reference["name"] == "build_features"
            && reference["resolution"] == "unresolved"
    }));
}

#[test]
fn renders_default_report_with_impact_and_inspect_items() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows):
    return [row for row in rows]


def clean_rows(rows):
    return list(rows)
"#,
    );
    repo.write(
        "pkg/runner.py",
        r#"
from pkg.features import build_features


def run(rows):
    return build_features(rows)
"#,
    );
    repo.write(
        "pkg/alias_consumer.py",
        r#"
import pkg.features as features


def consume(rows):
    return features.build_features(rows)
"#,
    );
    repo.write(
        "pkg/legacy.py",
        r#"
def old_helper():
    return "old"
"#,
    );
    repo.write(
        "pkg/old_consumer.py",
        r#"
from pkg.legacy import old_helper


def consume():
    return old_helper()
"#,
    );
    repo.write(
        "pkg/reporting.py",
        r#"
def summarize(rows):
    return len(rows)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False):
    values = []
    for row in rows:
        if strict and row is None:
            raise ValueError("missing row")
        values.append(row)
    return values


def clean_rows(rows):
    values = []
    for row in rows:
        if row is not None:
            values.append(row)
    return values
"#,
    );
    repo.git(&["rm", "pkg/old_consumer.py"]);
    repo.write(
        "pkg/export.py",
        r#"
import httpx
from pydantic import BaseModel
from pkg.reporting import summarize


class ExportEnvelope(BaseModel):
    body: str


def export_summary(rows):
    response = httpx.Response(200, text=str(summarize(rows)))
    return ExportEnvelope(body=response.text)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();

    assert!(report.starts_with("Sniffed the diff..."));
    assert!(report.contains("scope: 3 changed files"));
    assert!(report.contains("0 changed test files"));
    assert!(!report.contains("opinion:"));
    assert!(!report.contains("range:"));
    assert!(!report.contains("Imports:"));
    assert!(!report.contains("new external:"));
    assert!(!report.contains("new internal:"));
    assert!(!report.contains("removed imports:"));
    assert!(!report.contains("inspect:"));
    assert!(report.contains("- pkg/features.py::build_features"));
    assert!(report.contains("change: public signature changed; logic changed"));
    assert!(
        report.contains("signature: build_features(rows) -> build_features(rows, *, strict=False)")
    );
    assert!(report.contains(
        "complexity: increased; branches 0 -> 1; loops 0 -> 1; bool_ops 0 -> 1; nesting 0 -> 2"
    ));
    assert!(report.contains("tests: no nearby test movement"));
    assert!(
        report.contains("unchanged_callers: pkg/alias_consumer.py::consume (1 callsite), pkg/runner.py::run (1 callsite)")
    );
    assert!(!report.contains("fetch:"));
    assert!(!report.contains("coverage_risk:"));
    assert!(report.contains("path-only or low-signal symbol changes"));
    assert!(report.contains("use --json for exhaustive facts"));

    let limited = analyze_repo_report(
        repo.path(),
        &base,
        &head,
        PublicAnalysisOptions {
            report_limit: ReportLimit::Limited(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(limited.matches("\n- ").count(), 1);
    assert!(limited.contains("use --limit"));
    assert!(limited.contains("more items"));

    let verbose =
        analyze_repo_report_verbose(repo.path(), &base, &head, PublicAnalysisOptions::default())
            .unwrap();
    assert!(!verbose.contains("inspect:"));
    assert!(verbose.contains("pkg/features.py::build_features"));
    assert!(verbose.contains("change_kinds: body changed, signature changed"));
    assert!(verbose.contains("references: before"));
}

#[test]
fn report_limit_controls_report_item_count() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def first(rows):
    return list(rows)


def second(rows):
    return list(rows)


def third(rows):
    return list(rows)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/features.py",
        r#"
def first(rows):
    result = []
    for row in rows:
        result.append(row)
    return result


def second(rows):
    result = []
    for row in rows:
        if row is not None:
            result.append(row)
    return result


def third(rows):
    if not rows:
        return []
    return list(rows)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let limited = analyze_repo_report(
        repo.path(),
        &base,
        &head,
        PublicAnalysisOptions {
            report_limit: ReportLimit::Limited(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report_item_count(&limited), 2);
    assert!(limited.contains("use --limit 3 for more items"));

    let all = analyze_repo_report(
        repo.path(),
        &base,
        &head,
        PublicAnalysisOptions {
            report_limit: ReportLimit::All,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report_item_count(&all), 3);
    assert!(!all.contains("use --limit"));

    let verbose_limited = analyze_repo_report_verbose(
        repo.path(),
        &base,
        &head,
        PublicAnalysisOptions {
            report_limit: ReportLimit::Limited(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report_item_count(&verbose_limited), 2);
    assert!(verbose_limited.contains("change_kinds: body changed"));
}

#[test]
fn caller_preview_limit_controls_nested_reference_count() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows):
    return list(rows)
"#,
    );
    for name in ["api", "batch", "pipeline"] {
        let path = format!("pkg/{name}.py");
        repo.write(
            &path,
            r#"
from pkg.features import build_features

def run(rows):
    return build_features(rows)
"#,
        );
    }
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False):
    if strict and not rows:
        return []
    return list(rows)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report = analyze_repo_report(
        repo.path(),
        &base,
        &head,
        PublicAnalysisOptions {
            caller_preview_limit: 2,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(report.contains(
        "unchanged_callers: pkg/api.py::run (1 callsite), pkg/batch.py::run (1 callsite), +1 more"
    ));
    assert!(!report.contains("pkg/pipeline.py::run"));
}

#[test]
fn report_handles_no_signal_bearing_items() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/legacy.py",
        r#"
def stable_helper():
    return 1
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.git(&["mv", "pkg/legacy.py", "pkg/compatibility.py"]);
    repo.git(&["commit", "-m", "rename only"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();

    assert!(report.contains("scope: 1 changed file, 1 changed symbol, 0 changed test files"));
    assert!(report.contains("\n- none\n"));
    assert!(report.contains(
        "omitted: 1 path-only or low-signal symbol changes; use --json for exhaustive facts"
    ));
}

#[test]
fn reports_unchanged_test_references_for_changed_production_symbols() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows):
    return list(rows)
"#,
    );
    repo.write(
        "tests/test_features.py",
        r#"
from pkg.features import build_features


def test_build_features():
    assert build_features([]) == []
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False):
    values = []
    for row in rows:
        values.append(row)
    return values
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let analysis: Value = serde_json::from_str(
        &analyze_repo_json(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap(),
    )
    .unwrap();
    let changes = changes_by_name(&analysis);
    let build_features = changes["build_features"];

    assert_eq!(build_features["references_after"]["count"], 0);
    assert_eq!(build_features["test_references_after"]["count"], 2);
    assert_eq!(
        string_array(&build_features["test_references_after"]["unchanged_files"]),
        vec!["tests/test_features.py"]
    );

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();
    assert!(
        report
            .contains("unchanged_tests: tests/test_features.py::test_build_features (1 callsite)")
    );
    assert!(report.contains("tests: no nearby test movement"));
}

#[test]
fn reports_parse_errors_from_test_reference_pass() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows):
    return list(rows)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False):
    return list(rows)
"#,
    );
    repo.write(
        "tests/test_features.py",
        r#"
def test_broken(:
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let error =
        analyze_repo_json(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap_err();
    assert!(error.to_string().contains("tests/test_features.py"));

    let report = analyze_repo_report(
        repo.path(),
        &base,
        &head,
        PublicAnalysisOptions {
            allow_parse_errors: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(report.contains("parse_errors: 1 files"));

    let analysis: Value = serde_json::from_str(
        &analyze_repo_json(
            repo.path(),
            &base,
            &head,
            PublicAnalysisOptions {
                allow_parse_errors: true,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(analysis["test_facts"]["test_files_with_parse_errors"], 1);
    assert_eq!(
        string_array(&analysis["test_facts"]["test_parse_error_files"]),
        vec!["tests/test_features.py"]
    );
}

#[test]
fn renders_async_signature_change_in_report() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/tasks.py",
        r#"
def load():
    return "ok"
"#,
    );
    repo.write(
        "pkg/consumer.py",
        r#"
from pkg.tasks import load


def run():
    return load()
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/tasks.py",
        r#"
async def load():
    return "ok"
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();

    assert!(report.contains("- pkg/tasks.py::load"));
    assert!(report.contains("signature: async changed; load() -> async load()"));
    assert!(report.contains("unchanged_callers: pkg/consumer.py::run (1 callsite)"));
}

#[test]
fn does_not_match_resolved_references_to_same_named_symbols_in_other_modules() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows):
    return rows
"#,
    );
    repo.write(
        "other/features.py",
        r#"
def build_features(rows):
    return rows
"#,
    );
    repo.write(
        "pkg/consumer.py",
        r#"
from other.features import build_features


def consume(rows):
    return build_features(rows)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False):
    return rows
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let analysis: Value = serde_json::from_str(
        &analyze_repo_json(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap(),
    )
    .unwrap();
    let changes = changes_by_file_and_name(&analysis);
    let build_features = changes[&("pkg/features.py", "build_features")];

    assert_eq!(build_features["references_after"]["count"], 0);
    assert_eq!(build_features["references_before"]["count"], 0);
    assert_eq!(build_features["reference_delta"]["count_delta"], 0);
    assert_eq!(
        string_array(&build_features["references_after"]["unchanged_files"]),
        Vec::<&str>::new()
    );
    assert_eq!(
        string_array(&build_features["review_signals"]),
        vec![
            "public_signature_changed",
            "logic_changed_without_test_movement"
        ]
    );
}

#[test]
fn reports_reference_file_deltas_when_callers_are_added_and_removed() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows):
    return rows
"#,
    );
    repo.write(
        "pkg/old_consumer.py",
        r#"
from pkg.features import build_features


def consume(rows):
    return build_features(rows)
"#,
    );
    repo.write(
        "pkg/stable_consumer.py",
        r#"
from pkg.features import build_features


def consume(rows):
    return build_features(rows)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False):
    return rows
"#,
    );
    repo.git(&["rm", "pkg/old_consumer.py"]);
    repo.write(
        "pkg/new_consumer.py",
        r#"
from pkg.features import build_features


def consume(rows):
    return build_features(rows)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let analysis: Value = serde_json::from_str(
        &analyze_repo_json(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap(),
    )
    .unwrap();
    let changes = changes_by_file_and_name(&analysis);
    let build_features = changes[&("pkg/features.py", "build_features")];

    assert_eq!(build_features["references_before"]["count"], 4);
    assert_eq!(build_features["references_after"]["count"], 4);
    assert_eq!(
        string_array(&build_features["reference_delta"]["added_files"]),
        vec!["pkg/new_consumer.py"]
    );
    assert_eq!(
        string_array(&build_features["reference_delta"]["removed_files"]),
        vec!["pkg/old_consumer.py"]
    );
    assert_eq!(
        string_array(&build_features["reference_delta"]["unchanged_files"]),
        vec!["pkg/stable_consumer.py"]
    );
}

fn assert_kinds(change: &Value, expected: &[&str]) {
    assert_eq!(string_array(&change["change_kinds"]), expected);
}

fn changes_by_name(analysis: &Value) -> BTreeMap<&str, &Value> {
    analysis["symbol_changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| (change["qualified_name"].as_str().unwrap(), change))
        .collect::<BTreeMap<_, _>>()
}

fn changes_by_file_and_name(analysis: &Value) -> BTreeMap<(&str, &str), &Value> {
    analysis["symbol_changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| {
            (
                (
                    change["file"].as_str().unwrap(),
                    change["qualified_name"].as_str().unwrap(),
                ),
                change,
            )
        })
        .collect::<BTreeMap<_, _>>()
}

fn string_array(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect()
}

fn object_keys(value: &Value) -> Vec<&str> {
    let mut keys = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys
}

fn assert_top_level_json_shape(analysis: &Value) {
    assert_eq!(
        object_keys(analysis),
        vec![
            "after",
            "base",
            "before",
            "changed_files",
            "head",
            "language",
            "references_after",
            "schema_version",
            "symbol_changes",
            "test_facts",
        ]
    );
}

fn assert_symbol_change_json_shape(change: &Value) {
    assert_eq!(
        object_keys(change),
        vec![
            "after",
            "before",
            "change_kinds",
            "complexity_delta",
            "file",
            "kind",
            "qualified_name",
            "reference_delta",
            "references_after",
            "references_before",
            "review_signals",
            "signature_change",
            "test_references_after",
            "visibility",
        ]
    );
    assert_eq!(
        object_keys(&change["references_after"]),
        vec![
            "attribute_call_count",
            "changed_files",
            "count",
            "direct_call_count",
            "files",
            "from_import_count",
            "import_count",
            "matched_references",
            "resolved_count",
            "unchanged_files",
            "unresolved_count",
        ]
    );
    assert_eq!(
        object_keys(&change["reference_delta"]),
        vec![
            "added_files",
            "count_delta",
            "removed_files",
            "unchanged_files"
        ]
    );
}

fn report_item_count(report: &str) -> usize {
    report.lines().filter(|line| line.starts_with("- ")).count()
}

struct TestRepo {
    dir: TempDir,
}

impl TestRepo {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let repo = Self { dir };
        repo.git(&["init"]);
        repo.git(&["config", "user.email", "test@example.com"]);
        repo.git(&["config", "user.name", "Test User"]);
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, path: &str, content: &str) {
        let path = self.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content.trim_start()).unwrap();
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}
