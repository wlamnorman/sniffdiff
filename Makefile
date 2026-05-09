.PHONY: demo demo-repo

DEMO_REPO := target/sniffdiff-demo-python-repo
DEMO_ENV := target/sniffdiff-demo-python-repo.env

demo-repo:
	@mkdir -p target
	@bash scripts/create-demo-repo.sh "$(DEMO_REPO)" > "$(DEMO_ENV)"
	@cat "$(DEMO_ENV)"

demo: demo-repo
	@base=$$(sed -n 's/^SNIFFDIFF_DEMO_BASE=//p' "$(DEMO_ENV)"); \
	head=$$(sed -n 's/^SNIFFDIFF_DEMO_HEAD=//p' "$(DEMO_ENV)"); \
	repo=$$(sed -n 's/^SNIFFDIFF_DEMO_REPO=//p' "$(DEMO_ENV)"); \
	echo ""; \
	cargo run -- --repo "$$repo" "$$base..$$head"
