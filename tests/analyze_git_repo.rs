use std::fs;

use std::collections::BTreeMap;

mod common;

use common::TestRepo;
use serde_json::Value;
use sniffdiff::{
    INDEX_REF, PublicAnalysisOptions, ReportLimit, ReportVerbosity, WORKTREE_REF,
    analyze_repo_json, analyze_repo_raw_json as analyze_repo_json_facts, analyze_repo_report,
    analyze_repo_report_verbose, merge_base,
};

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
        analyze_repo_json_facts(repo.path(), &base, &head, PublicAnalysisOptions::default())
            .unwrap_err();
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
    assert!(partial_report.contains("parse_errors:\n  files: 1"));

    let analysis: Value = serde_json::from_str(
        &analyze_repo_json_facts(
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
    assert_eq!(build_features["visibility"], "public");
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
        "pkg/unaliased_module_consumer.py",
        r#"
import pkg.features


def consume(rows):
    return pkg.features.build_features(rows)
"#,
    );
    repo.write(
        "pkg/from_module_consumer.py",
        r#"
from pkg import features


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
        &analyze_repo_json_facts(repo.path(), &base, &head, PublicAnalysisOptions::default())
            .unwrap(),
    )
    .unwrap();
    let changes = changes_by_name(&analysis);
    let build_features = changes["build_features"];

    assert_eq!(analysis["schema_version"], 1);
    assert_eq!(analysis["before"]["files_parsed"], 8);
    assert_eq!(analysis["after"]["files_parsed"], 8);
    assert_eq!(analysis["before"]["files_skipped"], 1);
    assert_eq!(analysis["after"]["files_skipped"], 1);
    assert_kinds(build_features, &["body_changed", "signature_changed"]);
    assert_eq!(
        string_array(&build_features["signature_change"]["parameters_added"]),
        vec!["strict"]
    );
    assert_eq!(build_features["references_after"]["count"], 8);
    assert_eq!(build_features["references_before"]["count"], 8);
    assert_eq!(build_features["reference_delta"]["count_delta"], 0);
    assert_eq!(build_features["references_after"]["resolved_count"], 7);
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
            "pkg/from_module_consumer.py",
            "pkg/runner.py",
            "pkg/sub/consumer.py",
            "pkg/unaliased_module_consumer.py",
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
    assert!(references.iter().any(|reference| {
        reference["file"] == "pkg/unaliased_module_consumer.py"
            && reference["name"] == "build_features"
            && reference["module"] == "pkg.features"
            && reference["resolved_module"] == "pkg.features"
            && reference["resolved_name"] == "build_features"
            && reference["resolution"] == "resolved"
    }));
    assert!(references.iter().any(|reference| {
        reference["file"] == "pkg/from_module_consumer.py"
            && reference["name"] == "build_features"
            && reference["module"] == "features"
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

    assert!(report.starts_with("schema_version: 1"));
    assert!(report.contains("verbosity: normal"));
    assert!(report.contains("scope:\n  changed_files: 3"));
    assert!(report.contains("changed_test_files: 0"));
    assert!(!report.contains("opinion:"));
    assert!(!report.contains("range:"));
    assert!(!report.contains("Imports:"));
    assert!(!report.contains("new external:"));
    assert!(!report.contains("new internal:"));
    assert!(!report.contains("removed imports:"));
    assert!(report.contains("inspect:"));
    assert!(report.contains("- symbol: pkg/features.py::build_features"));
    assert!(report.contains("changes:\n  - public signature\n  - implementation"));
    assert!(report.contains("signature:\n    before: build_features(rows)\n    after: build_features(rows, *, strict=False)"));
    assert!(report.contains("complexity:\n    status: increased"));
    assert!(report.contains("name: nesting\n      before: 0\n      after: 2"));
    assert!(report.contains("tests: no direct test references found"));
    assert!(report.contains(
        "unchanged_callers:\n  - pkg/alias_consumer.py::consume\n  - pkg/runner.py::run"
    ));
    assert!(report.contains("changes:\n  - added public function"));
    assert!(!report.contains("fetch:"));
    assert!(!report.contains("coverage_risk:"));
    assert!(!report.contains("omitted:"));

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
    assert!(limited.contains("show all high-signal items"));

    let verbose =
        analyze_repo_report_verbose(repo.path(), &base, &head, PublicAnalysisOptions::default())
            .unwrap();
    assert!(verbose.contains("inspect:"));
    assert!(verbose.contains("pkg/features.py::build_features"));
    assert!(verbose.contains("change_kinds:\n    - body changed\n    - signature changed"));
    assert!(verbose.contains("references:\n      before:"));
}

#[test]
fn groups_added_class_methods_under_class_in_report() {
    let repo = TestRepo::new();
    repo.write("README.md", "base\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/exceptions.py",
        r#"
class NoSuchCommand(Exception):
    def __init__(self, name):
        if not name:
            raise ValueError("missing name")
        self.name = name

    def format_message(self):
        if self.name:
            return f"No such command: {self.name}"
        return "No such command"
"#,
    );
    repo.write(
        "pkg/core.py",
        r#"
from pkg.exceptions import NoSuchCommand


def resolve_command(name):
    raise NoSuchCommand(name)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();
    let yaml: serde_yaml::Value = serde_yaml::from_str(&report).unwrap();
    let inspect = yaml["inspect"].as_sequence().unwrap();

    assert!(
        inspect
            .iter()
            .any(|item| item["symbol"] == "pkg/exceptions.py::NoSuchCommand"
                && item["members"].as_sequence().is_some_and(|members| {
                    members.len() == 2
                        && members[0]["name"] == "__init__"
                        && members[1]["name"] == "format_message"
                        && members[0]["changes"][0] == "added public method"
                        && members[1]["changes"][0] == "added public method"
                }))
    );
    assert!(
        !inspect
            .iter()
            .any(|item| item["symbol"] == "pkg/exceptions.py::NoSuchCommand.__init__")
    );
    assert!(
        !inspect
            .iter()
            .any(|item| item["symbol"] == "pkg/exceptions.py::NoSuchCommand.format_message")
    );
    assert!(yaml.get("omitted").is_none());
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
    assert!(limited.contains("use --limit 3 to show all high-signal items"));

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
    assert!(verbose_limited.contains("change_kinds:\n    - body changed"));
}

#[test]
fn json_output_uses_the_same_report_model_and_verbosity_levels() {
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
    if strict and not rows:
        return []
    return list(rows)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report: Value = serde_json::from_str(
        &analyze_repo_json(
            repo.path(),
            &base,
            &head,
            PublicAnalysisOptions {
                report_verbosity: ReportVerbosity::Verbose,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["verbosity"], "verbose");
    assert_eq!(report["scope"]["changed_symbols"], 1);
    assert_eq!(
        report["inspect"][0]["symbol"],
        "pkg/features.py::build_features"
    );
    assert_eq!(
        string_array(&report["inspect"][0]["facts"]["change_kinds"]),
        vec!["body changed", "signature changed"]
    );
}

#[test]
fn analyzes_ref_against_worktree_like_git_diff_ref() {
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
    if strict and not rows:
        return []
    return list(rows)
"#,
    );
    repo.write(
        "pkg/new_module.py",
        r#"
def added_helper():
    return "new"
"#,
    );
    repo.git(&["add", "pkg/new_module.py"]);
    repo.write(
        "pkg/untracked.py",
        r#"
def ignored_by_git_diff():
    return "untracked"
"#,
    );

    let report = analyze_repo_report(
        repo.path(),
        &base,
        WORKTREE_REF,
        PublicAnalysisOptions::default(),
    )
    .unwrap();

    assert!(report.contains("changed_files: 2"));
    assert!(report.contains("changed_symbols: 2"));
    assert!(report.contains("- symbol: pkg/features.py::build_features"));
    assert!(report.contains("- symbol: pkg/new_module.py::added_helper"));
    assert!(!report.contains("pkg/untracked.py::ignored_by_git_diff"));
    assert!(report.contains("changes:\n  - added public function"));
}

#[test]
fn analyzes_index_against_worktree_like_git_diff_without_args() {
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

    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False):
    return list(rows)
"#,
    );
    repo.git(&["add", "pkg/features.py"]);
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False, source="unknown"):
    return list(rows)
"#,
    );
    repo.write(
        "pkg/untracked.py",
        r#"
def ignored_by_git_diff():
    return "untracked"
"#,
    );

    let report = analyze_repo_report(
        repo.path(),
        INDEX_REF,
        WORKTREE_REF,
        PublicAnalysisOptions::default(),
    )
    .unwrap();

    assert!(report.contains("changed_files: 1"));
    assert!(report.contains("changed_symbols: 1"));
    assert!(report.contains("before: build_features(rows, *, strict=False)"));
    assert!(report.contains("after: build_features(rows, *, strict=False, source=\"unknown\")"));
    assert!(!report.contains("pkg/untracked.py::ignored_by_git_diff"));
}

#[test]
fn analyzes_head_against_index_like_git_diff_staged() {
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

    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False):
    return list(rows)
"#,
    );
    repo.git(&["add", "pkg/features.py"]);
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False, source="unknown"):
    return list(rows)
"#,
    );

    let report = analyze_repo_report(
        repo.path(),
        "HEAD",
        INDEX_REF,
        PublicAnalysisOptions::default(),
    )
    .unwrap();

    assert!(report.contains("changed_files: 1"));
    assert!(report.contains("changed_symbols: 1"));
    assert!(report.contains("before: build_features(rows)"));
    assert!(report.contains("after: build_features(rows, *, strict=False)"));
    assert!(!report.contains("source=\"unknown\""));
}

#[test]
fn reports_deleted_symbols_against_worktree() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def removed_helper():
    return "removed"
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    fs::remove_file(repo.path().join("pkg/features.py")).unwrap();

    let report = analyze_repo_report(
        repo.path(),
        &base,
        WORKTREE_REF,
        PublicAnalysisOptions::default(),
    )
    .unwrap();

    assert!(report.contains("changed_files: 1"));
    assert!(report.contains("changed_symbols: 1"));
    assert!(report.contains("- symbol: pkg/features.py::removed_helper"));
    assert!(report.contains("changes:\n  - deleted public function"));
}

#[test]
fn reports_deleted_symbols_against_index() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def removed_helper():
    return "removed"
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);

    repo.git(&["rm", "pkg/features.py"]);

    let report = analyze_repo_report(
        repo.path(),
        "HEAD",
        INDEX_REF,
        PublicAnalysisOptions::default(),
    )
    .unwrap();

    assert!(report.contains("changed_files: 1"));
    assert!(report.contains("changed_symbols: 1"));
    assert!(report.contains("- symbol: pkg/features.py::removed_helper"));
    assert!(report.contains("changes:\n  - deleted public function"));
}

#[test]
fn resolves_triple_dot_base_like_git_diff() {
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
def build_features(rows):
    if rows is None:
        return []
    return list(rows)
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "main side"]);
    let main_side = repo.git(&["rev-parse", "HEAD"]);

    repo.git(&["checkout", "-b", "feature", &base]);
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
    repo.git(&["commit", "-m", "feature side"]);
    let feature_side = repo.git(&["rev-parse", "HEAD"]);

    let resolved_base = merge_base(repo.path(), &main_side, &feature_side).unwrap();
    assert_eq!(resolved_base, base);

    let report = analyze_repo_report(
        repo.path(),
        &resolved_base,
        &feature_side,
        PublicAnalysisOptions::default(),
    )
    .unwrap();

    assert!(report.contains("changed_files: 1"));
    assert!(report.contains("- symbol: pkg/features.py::build_features"));
    assert!(report.contains("before: build_features(rows)"));
    assert!(report.contains("after: build_features(rows, *, strict=False)"));
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

    assert!(
        report.contains(
            "unchanged_callers:\n  - pkg/api.py::run\n  - pkg/batch.py::run\n  - +1 more"
        )
    );
    assert!(!report.contains("pkg/pipeline.py::run"));
}

#[test]
fn repeated_callers_show_callsite_count() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows):
    return list(rows)
"#,
    );
    repo.write(
        "pkg/api.py",
        r#"
from pkg.features import build_features


def run(rows):
    left = build_features(rows)
    right = build_features(rows)
    return left + right
"#,
    );
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

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();

    assert!(report.contains("unchanged_callers:\n  - pkg/api.py::run (2 callsites)"));
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

    assert!(report.contains("changed_files: 1"));
    assert!(report.contains("changed_symbols: 1"));
    assert!(report.contains("changed_test_files: 0"));
    assert!(!report.contains("inspect:"));
    assert!(report.contains(
        "omitted:\n  symbol_changes: 1\n  low_signal: 1\n  hint: use --format json --verbosity full for full details"
    ));
}

#[test]
fn report_explains_when_range_has_no_changed_python_files() {
    let repo = TestRepo::new();
    repo.write("README.md", "before\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write("README.md", "after\n");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();

    assert!(report.contains("changed_files: 1"));
    assert!(report.contains("changed_symbols: 0"));
    assert!(report.contains("changed_test_files: 0"));
    assert!(!report.contains("inspect:"));
    assert!(
        report.contains("No Python file changes inside sniffdiff's symbol model were detected.")
    );
}

#[test]
fn report_explains_when_python_changes_have_no_symbol_changes() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/settings.py",
        r#"
FEATURE_FLAG = False
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/settings.py",
        r#"
FEATURE_FLAG = True
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();

    assert!(report.contains("changed_files: 1"));
    assert!(report.contains("changed_symbols: 0"));
    assert!(report.contains("changed_test_files: 0"));
    assert!(!report.contains("inspect:"));
    assert!(
        report.contains("No Python file changes inside sniffdiff's symbol model were detected.")
    );
}

#[test]
fn formatting_only_python_changes_do_not_create_symbol_changes() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def build_features(rows, *, strict=False):
    value = rows[0]
    return value
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/features.py",
        r#"
def build_features(
    rows,
    *,
    strict=False,
):
    value=rows[0]

    # formatting/comment-only change
    return value
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();
    let analysis: Value = serde_json::from_str(
        &analyze_repo_json_facts(repo.path(), &base, &head, PublicAnalysisOptions::default())
            .unwrap(),
    )
    .unwrap();

    assert!(report.contains("changed_files: 1"));
    assert!(report.contains("changed_symbols: 0"));
    assert!(report.contains("changed_test_files: 0"));
    assert_eq!(analysis["symbol_changes"].as_array().unwrap().len(), 0);
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
        &analyze_repo_json_facts(repo.path(), &base, &head, PublicAnalysisOptions::default())
            .unwrap(),
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
    assert!(report.contains("unchanged_tests:\n  - tests/test_features.py::test_build_features"));
    assert!(!report.contains("tests: no direct test references found"));
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
        analyze_repo_json_facts(repo.path(), &base, &head, PublicAnalysisOptions::default())
            .unwrap_err();
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
    assert!(report.contains("parse_errors:\n  files: 1"));

    let analysis: Value = serde_json::from_str(
        &analyze_repo_json_facts(
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

    assert!(report.contains("- symbol: pkg/tasks.py::load"));
    assert!(report.contains("signature:\n    before: load()\n    after: async load()"));
    assert!(report.contains("unchanged_callers:\n  - pkg/consumer.py::run"));
}

#[test]
fn renders_annotation_only_signature_changes_as_type_annotations() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/features.py",
        r#"
def normalize(row):
    return row
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/features.py",
        r#"
def normalize(row: dict[str, object]) -> dict[str, object]:
    return row
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();

    assert!(report.contains("- symbol: pkg/features.py::normalize"));
    assert!(report.contains(
        "changes:\n  - 'type annotations (row: unannotated -> dict[str, object]; return: unannotated -> dict[str, object])'"
    ));
    assert!(!report.contains("- public signature"));
    assert!(report.contains("before: normalize(row)"));
    assert!(report.contains("after: 'normalize(row: dict[str, object]) -> dict[str, object]'"));
}

#[test]
fn renders_parameter_type_annotation_delta_in_change_label() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/errors.py",
        r#"
import collections.abc as cabc


class NoSuchOption(Exception):
    def __init__(self, possibilities: cabc.Sequence[str] | None = None) -> None:
        self.possibilities = possibilities
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/errors.py",
        r#"
import collections.abc as cabc


class NoSuchOption(Exception):
    def __init__(self, possibilities: cabc.Iterable[str] | None = None) -> None:
        self.possibilities = possibilities
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();

    assert!(report.contains("- symbol: pkg/errors.py::NoSuchOption.__init__"));
    assert!(report.contains(
        "changes:\n  - 'type annotation (possibilities: cabc.Sequence[str] | None -> cabc.Iterable[str] | None)'"
    ));
}

#[test]
fn renders_multiline_signature_changes_as_before_after_report_block() {
    let repo = TestRepo::new();
    repo.write(
        "pkg/leaderboard.py",
        r#"
def display_player(
    record: str,
    *,
    current_player_id: str,
) -> str:
    return record
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "base"]);
    let base = repo.git(&["rev-parse", "HEAD"]);

    repo.write(
        "pkg/leaderboard.py",
        r#"
def display_player(record: str) -> str:
    if not record:
        return ""
    return record
"#,
    );
    repo.git(&["add", "."]);
    repo.git(&["commit", "-m", "head"]);
    let head = repo.git(&["rev-parse", "HEAD"]);

    let report =
        analyze_repo_report(repo.path(), &base, &head, PublicAnalysisOptions::default()).unwrap();

    assert!(report.contains("- symbol: pkg/leaderboard.py::display_player"));
    assert!(
        report.contains("before: 'display_player(record: str, *, current_player_id: str) -> str'")
    );
    assert!(report.contains("after: 'display_player(record: str) -> str'"));
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
        &analyze_repo_json_facts(repo.path(), &base, &head, PublicAnalysisOptions::default())
            .unwrap(),
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
        vec!["public_signature_changed"]
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
        &analyze_repo_json_facts(repo.path(), &base, &head, PublicAnalysisOptions::default())
            .unwrap(),
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
    report
        .lines()
        .filter(|line| line.starts_with("- symbol: "))
        .count()
}
