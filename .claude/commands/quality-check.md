---
description: Run the complete pre-commit quality gate checklist (fmt, clippy, test, build)
allowed-tools: Bash(cargo:*)
---

# Quality Check

Run the complete pre-commit quality gate checklist for Rehoboam.

## Steps

Execute each step in sequence, stopping on first failure:

1. **Format Check** - Verify code formatting
   ```bash
   cargo fmt --all -- --check
   ```

2. **Lint Check** - Run clippy with strict warnings
   ```bash
   cargo clippy --all-targets --all-features -- -D warnings
   ```

3. **Test Suite** - Run all tests
   ```bash
   cargo test --all-features
   ```

4. **Release Build** - Verify release compilation
   ```bash
   cargo build --release
   ```

## Output Format

Report each step as:
- PASS: Step completed successfully
- FAIL: Step failed with error summary

If any step fails, provide:
1. Clear error message
2. Suggested fix (e.g., "Run `cargo fmt` to fix formatting")
3. Stop execution (don't continue to next steps)

If all steps pass, report: "All quality checks passed. Ready for PR."
