# Contributing to CPAC

Thank you for your interest in CPAC. This project is maintained by
BitConcepts, LLC under a research and evaluation license.

## Before Contributing

By submitting a pull request or patch, you agree to the contribution
terms in the [LICENSE](LICENSE), which grant BitConcepts, LLC a perpetual,
irrevocable, worldwide, royalty-free license to your contributions.

## Development Setup

1. Install Rust stable (1.75+): https://rustup.rs
2. Clone the repository
3. Build and test:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

## Pull Request Checklist

- [ ] All tests pass (`cargo test --workspace`)
- [ ] No clippy warnings (`cargo clippy --workspace -- -D warnings`)
- [ ] Code is formatted (`cargo fmt --all -- --check`)
- [ ] New public items have doc comments
- [ ] New `.rs` files have copyright header
- [ ] Tests added for new functionality

## Code Style

- See `AGENTS.md` for coding conventions and gotchas
- Error handling via `CpacError`, no `unwrap()` in library crates
- `#[must_use]` on Result-returning public functions

## Reporting Issues

Please file issues on GitHub with:
- Steps to reproduce
- Expected vs actual behavior
- Rust version (`rustc --version`)
- Platform (OS, architecture)

## Security Issues

See [SECURITY.md](SECURITY.md) for reporting security vulnerabilities.
Do **not** file security issues as public GitHub issues.
