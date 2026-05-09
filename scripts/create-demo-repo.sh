#!/usr/bin/env bash
set -euo pipefail

repo_dir="${1:-target/demo-python-repo}"

rm -rf "$repo_dir"
mkdir -p "$repo_dir/src" "$repo_dir/tests"

git -C "$repo_dir" init --quiet
git -C "$repo_dir" config user.email "sniffdiff@example.com"
git -C "$repo_dir" config user.name "sniffdiff demo"

cat > "$repo_dir/src/features.py" <<'PY'
def build_features(rows):
    features = []
    for row in rows:
        features.append(_normalize_row(row))
    return features


def _normalize_row(row):
    return row["name"].strip().lower()


def removed_public_helper():
    return "old"


class Formatter:
    def format_name(self, name):
        formatted = name.strip().title()
        if not formatted:
            return "Unknown"
        return formatted

    def format_many(self, names):
        return [self.format_name(name) for name in names]
PY

cat > "$repo_dir/src/validators.py" <<'PY'
def validate_row(row):
    if "name" not in row:
        return False
    return bool(row["name"].strip())


def is_ready(row):
    return validate_row(row) and bool(row["name"].strip())
PY

cat > "$repo_dir/src/scoring.py" <<'PY'
def score_features(features):
    return len(features)
PY

cat > "$repo_dir/src/train.py" <<'PY'
from src.features import build_features
from src.scoring import score_features


def train(rows):
    features = build_features(rows)
    return score_features(features)
PY

cat > "$repo_dir/src/predict.py" <<'PY'
from src.features import build_features as make_features


def predict(rows):
    return make_features(rows)
PY

cat > "$repo_dir/src/pipeline.py" <<'PY'
import src.features as features


def run_pipeline(rows):
    return features.build_features(rows)
PY

cat > "$repo_dir/src/batch.py" <<'PY'
import src.features as feature_builder


def build_batch(rows):
    return feature_builder.build_features(rows)
PY

cat > "$repo_dir/src/api.py" <<'PY'
from src.features import build_features
from src.validators import is_ready


def preview(rows):
    ready_rows = [row for row in rows if is_ready(row)]
    return build_features(ready_rows)
PY

cat > "$repo_dir/src/formatting.py" <<'PY'
def format_label(value):
    return str(value).strip()


def format_status(value):
    return str(value).strip().upper()
PY

cat > "$repo_dir/src/dashboard.py" <<'PY'
from src.formatting import format_status


def render_status(value):
    return format_status(value)
PY

cat > "$repo_dir/src/legacy.py" <<'PY'
def legacy_transform(value):
    return str(value).lower()
PY

cat > "$repo_dir/src/compat_consumer.py" <<'PY'
from src.legacy import legacy_transform


def use_legacy(value):
    return legacy_transform(value)
PY

cat > "$repo_dir/tests/test_features.py" <<'PY'
from src.features import build_features


def test_build_features():
    assert build_features([{"name": " Ada "}]) == ["ada"]
PY

git -C "$repo_dir" add .
git -C "$repo_dir" commit --quiet -m "base"
base="$(git -C "$repo_dir" rev-parse HEAD)"

git -C "$repo_dir" mv src/legacy.py src/compatibility.py

cat > "$repo_dir/src/features.py" <<'PY'
def build_features(rows, *, strict=False, source="unknown"):
    features = []
    for row in rows:
        normalized = _normalize_row(row, strict=strict)
        if normalized is None:
            continue
        if source != "unknown":
            normalized = f"{source}:{normalized}"
        features.append(normalized)
    return features


def _normalize_row(row, *, strict=False):
    if "name" not in row:
        if strict:
            raise ValueError("missing name")
        return None
    return row["name"].strip().lower()


def added_helper():
    return "new"


class Formatter:
    def format_name(self, name, *, uppercase=False, fallback="unknown"):
        if name is None:
            return fallback
        formatted = name.strip().title()
        if uppercase:
            return formatted.upper()
        return formatted

    def format_many(self, names, *, uppercase=False):
        return [self.format_name(name, uppercase=uppercase) for name in names]
PY

cat > "$repo_dir/src/validators.py" <<'PY'
def validate_row(row, *, strict=False):
    if "name" not in row:
        if strict:
            raise ValueError("missing name")
        return False
    value = row["name"]
    if value is None:
        if strict:
            raise ValueError("missing name")
        return False
    return bool(str(value).strip())


def is_ready(row):
    return validate_row(row)
PY

cat > "$repo_dir/src/train.py" <<'PY'
from src.features import build_features
from src.scoring import score_features


def train(rows):
    features = build_features(rows, strict=True, source="train")
    return score_features(features)
PY

cat > "$repo_dir/src/scoring.py" <<'PY'
def score_features(features):
    score = 0
    for feature in features:
        if not feature:
            continue
        if ":" in feature:
            score += 2
        else:
            score += 1
    return score
PY

cat > "$repo_dir/src/formatting.py" <<'PY'
def format_label(value):
    value = str(value).strip()
    if not value:
        return "unknown"
    return value


def format_status(value):
    value = str(value).strip().upper()
    if value in {"OK", "READY", "DONE"}:
        return "green"
    if value in {"WARN", "PENDING"}:
        return "yellow"
    if value in {"ERROR", "FAILED"}:
        return "red"
    return "gray"
PY

cat > "$repo_dir/src/compat_consumer.py" <<'PY'
from src.compatibility import legacy_transform


def use_legacy(value):
    return legacy_transform(value)
PY

cat > "$repo_dir/src/reporting.py" <<'PY'
from src.features import build_features
from src.formatting import format_label


def summarize(rows):
    features = build_features(rows, source="report")
    return {"count": len(features), "label": format_label("summary")}
PY

cat > "$repo_dir/src/compatibility.py" <<'PY'
def legacy_transform(value):
    return str(value).lower()
PY

cat > "$repo_dir/src/export.py" <<'PY'
import httpx
from pydantic import BaseModel
from src.reporting import summarize


class ExportEnvelope(BaseModel):
    body: str


def export_summary(rows):
    summary = summarize(rows)
    response = httpx.Response(200, text=f"{summary['label']}={summary['count']}")
    return ExportEnvelope(body=response.text)
PY

cat > "$repo_dir/tests/test_features.py" <<'PY'
from src.features import build_features


def test_build_features():
    assert build_features([{"name": " Ada "}]) == ["ada"]


def test_build_features_skips_missing_names():
    assert build_features([{}]) == []
PY

cat > "$repo_dir/tests/test_formatting.py" <<'PY'
from src.formatting import format_status


def test_format_status():
    assert format_status("ok") == "green"
PY

git -C "$repo_dir" add .
git -C "$repo_dir" commit --quiet -m "head"
head="$(git -C "$repo_dir" rev-parse HEAD)"

cat <<EOF
Created demo repo: $repo_dir
Base: $base
Head: $head
SNIFFDIFF_DEMO_REPO=$repo_dir
SNIFFDIFF_DEMO_BASE=$base
SNIFFDIFF_DEMO_HEAD=$head

Run:
  cargo run -- --repo "$repo_dir" "$base..$head"
  cargo run -- --repo "$repo_dir" "$base..$head" --verbose
EOF
