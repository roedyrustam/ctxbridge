# Contributing to `ctxbridge`

Thank you for contributing to `ctxbridge`!

## Development Setup
1. Install Rust (`1.75+`).
2. Clone the repository and run the test suite:
   ```bash
   git clone https://github.com/roedyrustam/ctxbridge.git
   cd ctxbridge
   cargo test
   ```

## Quality Checks
Before opening a pull request, please ensure all CI checks pass locally:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Guidelines
- Keep `ctxbridge` local-first, offline-capable, and free of runtime dependencies.
- Preserve non-destructive `<!-- ctxbridge:start -->` ... `<!-- ctxbridge:end -->` markers in adapter sync.
- Add unit or integration tests in `tests/` for new CLI flags, store queries, or MCP tools/resources.
