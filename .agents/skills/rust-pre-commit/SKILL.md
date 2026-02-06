---
name: rust-pre-commit
description: Validate rust code before committing 
---
This skill defines the standard pre-commit validation steps for Rust work in this repo.
Use it whenever you are about to commit changes or state that work is complete.

Goal
Run a consistent set of build, check, and test commands to catch compile errors,
lint failures, and test regressions before finishing work.

When to use
- Before creating a git commit that includes Rust changes.
- Before replying that a Rust task is complete if you have edited Rust files.

Do not run
- If the user explicitly asks you not to run checks.
- If there are no Rust changes and no Rust build/test impact.

Commands (run from repo root)
1) Format check (do not auto-fix unless asked):
	 cargo fmt --all -- --check

2) Lint check:
	 cargo clippy --all-targets --all-features -- -D warnings

3) Build:
	 cargo build --all-targets --all-features

4) Tests:
	 cargo test --all-targets --all-features

Execution rules
- Prefer the order above; stop early only if a command fails.
- If a command fails, report the failure and ask how the user wants to proceed.
- When Clippy fails under `-D warnings`, prefer fixing the warnings unless the user asks to defer or suppress them.
- Do not auto-fix formatting or lint warnings unless the user asks.
- If the repo has a workspace, these commands should cover all members.
- If tests are long-running and the user is time-sensitive, ask before running tests.

Reporting
- Summarize the result of each command (pass/fail) in the response.
- If any command is skipped, explain why.

Examples
Example: normal pre-commit run
Commands:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo build --all-targets --all-features
	cargo test --all-targets --all-features
Response:
	Format: pass; Clippy: pass; Build: pass; Tests: pass.

Example: clippy failure
Command:
	cargo clippy --all-targets --all-features -- -D warnings
Response:
	Clippy failed with warnings treated as errors. I stopped here; want me to fix
	the warnings or keep them as-is?