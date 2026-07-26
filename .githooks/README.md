# Git Hooks

This directory contains shared git hooks for s3-gallery.

## Hooks

- **pre-commit**: Runs CI gate checks before each commit:
  1. `cargo fmt --all --check` — format consistency
  2. `cargo clippy --workspace -- -D warnings` — zero warnings
  3. `cargo test --lib` — unit tests
  4. `cargo test --doc` — doc tests

## Installation

```bash
git config core.hooksPath .githooks
```

This is already done if you cloned the repo after the hooks were added.
If hooks don't run, run the command above.

## Bypass

To skip hooks for a single commit:

```bash
git commit --no-verify -m "your message"
```

Use sparingly — hooks exist to catch issues before they reach CI.