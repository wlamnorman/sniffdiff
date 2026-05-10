.PHONY: check preflight pre-release fmt lint test rust-test python-test version-check package-dry-run rust-package-dry-run python-package-dry-run python-dist python-dist-clean python-dist-sdist python-dist-host python-dist-linux-x86_64 python-upload-testpypi python-upload-pypi perf example examples example-requests example-click

DEMO_REPO := target/sniffdiff-demo-python-repo
DEMO_ENV := target/sniffdiff-demo-python-repo.env
REAL_EXAMPLE := scripts/run-real-example.sh
PYTHON ?= python3
PYPI_DIST ?= target/pypi-dist
MATURIN ?= maturin
MATURIN_DOCKER_IMAGE ?= ghcr.io/pyo3/maturin

check: fmt lint test

preflight: check package-dry-run

pre-release: check version-check rust-package-dry-run python-package-dry-run

fmt:
	@cargo fmt

lint:
	@cargo clippy --all-targets --all-features -- -D warnings

test: rust-test python-test

rust-test:
	@cargo test

python-test:
	@PYTHONPATH=python "$(PYTHON)" -m unittest discover -s python/tests

version-check:
	@"$(PYTHON)" scripts/check-versions.py

package-dry-run:
	@cargo publish --dry-run --allow-dirty

rust-package-dry-run:
	@cargo publish --dry-run

python-package-dry-run:
	@maturin build --sdist --out target/wheels

python-dist: python-dist-clean python-dist-sdist python-dist-host python-dist-linux-x86_64
	@ls -1 "$(PYPI_DIST)"

python-dist-clean:
	@rm -rf "$(PYPI_DIST)"
	@mkdir -p "$(PYPI_DIST)"

python-dist-sdist:
	@"$(MATURIN)" sdist --out "$(PYPI_DIST)"

python-dist-host:
	@"$(MATURIN)" build --release --locked --out "$(PYPI_DIST)"

python-dist-linux-x86_64:
	@docker run --rm -v "$$(pwd)":/io "$(MATURIN_DOCKER_IMAGE)" build --release --locked --manylinux 2014 --target x86_64-unknown-linux-gnu --out /io/"$(PYPI_DIST)"

python-upload-testpypi:
	@"$(MATURIN)" upload --repository testpypi "$(PYPI_DIST)"/*

python-upload-pypi:
	@"$(MATURIN)" upload "$(PYPI_DIST)"/*

perf:
	@"$(PYTHON)" scripts/run-perf-smoke.py

example-repo:
	@mkdir -p target
	@bash scripts/create-example-repo.sh "$(DEMO_REPO)" > "$(DEMO_ENV)"
	@cat "$(DEMO_ENV)"

example: example-repo
	@base=$$(sed -n 's/^SNIFFDIFF_DEMO_BASE=//p' "$(DEMO_ENV)"); \
	head=$$(sed -n 's/^SNIFFDIFF_DEMO_HEAD=//p' "$(DEMO_ENV)"); \
	repo=$$(sed -n 's/^SNIFFDIFF_DEMO_REPO=//p' "$(DEMO_ENV)"); \
	echo ""; \
	cargo run -- --repo "$$repo" "$$base..$$head"


examples: example-requests example-click

example-requests:
	@bash "$(REAL_EXAMPLE)" \
		requests-digest-security \
		https://github.com/psf/requests.git \
		a044b020dea43230585126901684a0f30ec635a8 \
		all

example-click:
	@bash "$(REAL_EXAMPLE)" \
		click-no-such-command \
		https://github.com/pallets/click.git \
		831c8f0948af519e45b90801d7430ff25451f972 \
		8
