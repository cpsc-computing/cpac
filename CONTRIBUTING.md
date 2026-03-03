# Contributing to CPAC

Thank you for your interest in contributing to CPAC! This document provides guidelines and workflows for contributing to the project.

## Table of Contents

- [Development Workflow](#development-workflow)
- [Branch Strategy (GitFlow)](#branch-strategy-gitflow)
- [Semantic Versioning](#semantic-versioning)
- [Getting Started](#getting-started)
- [Making Changes](#making-changes)
- [Pull Request Process](#pull-request-process)
- [Code Standards](#code-standards)
- [Testing Requirements](#testing-requirements)
- [Commit Message Guidelines](#commit-message-guidelines)
- [Release Process](#release-process)
- [Legal](#legal)

## Development Workflow

CPAC uses **GitFlow** as our branching model with **Semantic Versioning (SemVer)** for releases.

### Quick Reference

| Branch Type | Naming | Purpose | Base Branch | Merge To |
|-------------|--------|---------|-------------|----------|
| `main` | `main` | Production releases only | - | - |
| `develop` | `develop` | Integration branch | `main` | `main` |
| Feature | `feature/<name>` | New features | `develop` | `develop` |
| Bugfix | `bugfix/<name>` | Non-critical bugs | `develop` | `develop` |
| Hotfix | `hotfix/<version>` | Critical production fixes | `main` | `main` + `develop` |
| Release | `release/<version>` | Release preparation | `develop` | `main` + `develop` |

## Branch Strategy (GitFlow)

### Main Branches

#### `main` (Production)
- **Protected**: Always stable, production-ready code
- **Source of Truth**: Reflects what's currently in production
- **Tags**: All releases are tagged here (e.g., `v0.1.0`, `v1.2.3`)
- **Direct commits**: ❌ Never commit directly
- **Merges from**: `release/*` and `hotfix/*` branches only

#### `develop` (Integration)
- **Protected**: Integration branch for ongoing development
- **Latest Features**: Contains the latest delivered development changes
- **Direct commits**: ❌ Never commit directly
- **Merges from**: `feature/*`, `bugfix/*`, `release/*`, and `hotfix/*` branches

### Supporting Branches

#### Feature Branches (`feature/*`)

**Purpose**: Develop new features for upcoming releases

**Naming**: `feature/<short-description>`
- ✅ `feature/parallel-compression`
- ✅ `feature/add-lz4-backend`
- ❌ `my-feature` (missing prefix)

**Workflow**:
```bash
# Create feature branch from develop
git checkout develop
git pull origin develop
git checkout -b feature/my-awesome-feature

# Work on your feature
git add .
git commit -m "feat: add awesome feature"
git push origin feature/my-awesome-feature

# Create PR to develop
# After review and approval, squash merge to develop
```

**Lifecycle**:
- Branch from: `develop`
- Merge to: `develop`
- Delete after: Merged to `develop`

#### Bugfix Branches (`bugfix/*`)

**Purpose**: Fix non-critical bugs in development

**Naming**: `bugfix/<short-description>`
- ✅ `bugfix/fix-memory-leak`
- ✅ `bugfix/correct-ratio-calculation`

**Workflow**:
```bash
# Same as feature branches
git checkout develop
git pull origin develop
git checkout -b bugfix/fix-memory-leak

# Fix the bug
git add .
git commit -m "fix: resolve memory leak in pool"
git push origin bugfix/fix-memory-leak

# Create PR to develop
```

**Lifecycle**:
- Branch from: `develop`
- Merge to: `develop`
- Delete after: Merged to `develop`

#### Release Branches (`release/*`)

**Purpose**: Prepare for production release (version bump, changelog, final testing)

**Naming**: `release/<version>`
- ✅ `release/0.2.0`
- ✅ `release/1.0.0-rc.1`

**Workflow**:
```bash
# Create release branch from develop
git checkout develop
git pull origin develop
git checkout -b release/0.2.0

# Update version in Cargo.toml
# Update CHANGELOG.md
# Fix release-blocking bugs only

git add .
git commit -m "chore: prepare release 0.2.0"
git push origin release/0.2.0

# Create PR to main
# After approval:
# 1. Merge to main (creates release)
# 2. Merge back to develop
# 3. Delete release branch
```

**Lifecycle**:
- Branch from: `develop`
- Merge to: `main` AND `develop`
- Delete after: Merged to both

**Rules**:
- ✅ Version bumps
- ✅ Changelog updates
- ✅ Documentation fixes
- ✅ Release-blocking bug fixes
- ❌ New features
- ❌ Non-critical bugs

#### Hotfix Branches (`hotfix/*`)

**Purpose**: Critical fixes for production issues

**Naming**: `hotfix/<version>`
- ✅ `hotfix/0.1.1`
- ✅ `hotfix/1.2.4`

**Workflow**:
```bash
# Create hotfix branch from main
git checkout main
git pull origin main
git checkout -b hotfix/0.1.1

# Fix the critical issue
# Update version (patch bump)
# Update CHANGELOG.md

git add .
git commit -m "fix: critical security vulnerability (CVE-2026-12345)"
git push origin hotfix/0.1.1

# Create PR to main
# After approval:
# 1. Merge to main (creates hotfix release)
# 2. Merge back to develop
# 3. Delete hotfix branch
```

**Lifecycle**:
- Branch from: `main`
- Merge to: `main` AND `develop`
- Delete after: Merged to both

**When to use**:
- 🔴 Security vulnerabilities
- 🔴 Critical production bugs
- 🔴 Data corruption issues

## Semantic Versioning

CPAC follows [Semantic Versioning 2.0.0](https://semver.org/):

### Version Format: `MAJOR.MINOR.PATCH`

- **MAJOR** (1.0.0): Breaking API changes
- **MINOR** (0.1.0): New features, backward compatible
- **PATCH** (0.0.1): Bug fixes, backward compatible

### Pre-1.0 Versions

For versions `0.y.z`:
- **MINOR** bumps MAY include breaking changes
- **PATCH** bumps should be backward compatible

### Pre-release Versions

- `0.2.0-alpha.1`: Alpha release
- `0.2.0-beta.1`: Beta release
- `0.2.0-rc.1`: Release candidate

### Examples

| Change | Current | Next Version |
|--------|---------|--------------|
| Bug fix | 0.1.5 | 0.1.6 |
| New feature | 0.1.5 | 0.2.0 |
| Breaking change (pre-1.0) | 0.1.5 | 0.2.0 |
| Breaking change (post-1.0) | 1.5.2 | 2.0.0 |
| Critical hotfix | 1.5.2 | 1.5.3 |

## Getting Started

### Prerequisites

- Rust 1.75+ (stable)
- Git 2.30+
- PowerShell 7+ (Windows) or Bash (Linux/macOS)

### Initial Setup

```bash
# Clone the repository
git clone https://github.com/cpsc-computing/cpac.git
cd cpac

# Checkout develop branch
git checkout develop
git pull origin develop

# Install dependencies and build
cargo build

# Run tests to verify setup
cargo test --workspace

# Run clippy and fmt
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

### Development Environment

```bash
# Build in debug mode (fast compilation)
cargo build

# Build in release mode (optimized)
cargo build --release

# Run specific tests
cargo test -p cpac-engine
cargo test --test cross_engine

# Run benchmarks
cargo run --release --bin cpac -- benchmark test.txt --quick
```

## Making Changes

### 1. Choose the Right Branch Type

- **New feature**? → `feature/*` from `develop`
- **Bug fix (non-critical)**? → `bugfix/*` from `develop`
- **Critical production bug**? → `hotfix/*` from `main`
- **Preparing release**? → `release/*` from `develop`

### 2. Create Your Branch

```bash
# For features/bugfixes
git checkout develop
git pull origin develop
git checkout -b feature/my-feature

# For hotfixes
git checkout main
git pull origin main
git checkout -b hotfix/1.2.3
```

### 3. Make Your Changes

- Write clean, documented code
- Add tests for new functionality
- Update documentation as needed
- Run tests frequently: `cargo test`

### 4. Commit Your Changes

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```bash
git add .
git commit -m "feat: add new compression backend"
```

See [Commit Message Guidelines](#commit-message-guidelines) below.

### 5. Push and Create PR

```bash
git push origin feature/my-feature
```

Then create a Pull Request on GitHub targeting the appropriate branch:
- Feature/bugfix PRs → `develop`
- Hotfix/release PRs → `main`

## Pull Request Process

### Before Creating a PR

- [ ] All tests pass: `cargo test --workspace`
- [ ] No clippy warnings: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] Code is formatted: `cargo fmt --all`
- [ ] Documentation is updated (if applicable)
- [ ] New `.rs` files have copyright header
- [ ] Commits follow conventional commit format

### PR Title Format

Use conventional commit format:
- `feat: add LZ4 compression backend`
- `fix: resolve memory leak in buffer pool`
- `docs: update benchmarking guide`
- `perf: optimize SSR analysis`

### PR Description Template

```markdown
## Description
Brief description of changes

## Type of Change
- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update

## Testing
- [ ] Unit tests added/updated
- [ ] Integration tests pass
- [ ] Benchmarks run (if performance-related)

## Checklist
- [ ] Code follows project style guidelines
- [ ] Self-review completed
- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] No new warnings introduced
- [ ] Copyright headers added to new files
```

### Review Process

1. **Automated Checks**: CI must pass (tests, clippy, fmt)
2. **Code Review**: At least 1 approval required (for protected branches)
3. **Merge**: Squash and merge to keep history clean

## Code Standards

### Rust Style

- Follow Rust API Guidelines
- Use `cargo fmt` with default settings
- Pass `cargo clippy` with no warnings
- Write rustdoc comments for public APIs
- See `AGENTS.md` for coding conventions and gotchas

### Copyright Headers

All new `.rs` files must include:

```rust
// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
```

### Code Quality

```rust
// ✅ Good: Clear, documented, tested
/// Compresses data using the specified backend.
///
/// # Examples
/// ```
/// use cpac_engine::{compress, CompressConfig, Backend};
/// let result = compress(b"test", &CompressConfig::default())?;
/// ```
#[must_use = "compression result is returned"]
pub fn compress(data: &[u8], config: &CompressConfig) -> CpacResult<CompressResult> {
    // Implementation
}

// ❌ Bad: Undocumented, unclear
pub fn do_thing(x: &[u8], y: &Thing) -> Res {
    // ...
}
```

### Error Handling

```rust
// ✅ Use Result types with descriptive errors
Err(CpacError::CompressFailed(format!("zstd: {e}")))

// ❌ Don't panic or unwrap in library code
panic!("compression failed");
data.unwrap(); // Use ? or proper error handling instead
```

## Testing Requirements

### Test Coverage

All PRs must include tests:

- **Unit tests**: For individual functions/modules
- **Integration tests**: For cross-module functionality
- **Property tests**: For edge cases (using proptest)

### Running Tests

```bash
# All tests
cargo test --workspace

# Specific package
cargo test -p cpac-engine

# Specific test
cargo test --test cross_engine

# With output
cargo test -- --nocapture

# Coverage (requires tarpaulin)
cargo tarpaulin --workspace --timeout 300
```

### Test Guidelines

- Test both success and error paths
- Use descriptive test names
- Keep tests focused and independent
- Mock external dependencies
- Add property tests for complex logic

## Commit Message Guidelines

We use [Conventional Commits](https://www.conventionalcommits.org/) for automated changelog generation.

### Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Types

- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `style`: Code style (formatting, missing semicolons, etc.)
- `refactor`: Code change that neither fixes a bug nor adds a feature
- `perf`: Performance improvement
- `test`: Adding missing tests
- `chore`: Changes to build process, tools, dependencies
- `ci`: Changes to CI/CD configuration

### Scopes (Optional)

- `engine`: cpac-engine
- `cli`: cpac-cli
- `transforms`: cpac-transforms
- `entropy`: cpac-entropy
- `archive`: cpac-archive
- `crypto`: cpac-crypto
- `streaming`: cpac-streaming
- `dict`: cpac-dict

### Examples

```bash
# Simple feature
git commit -m "feat: add LZ4 backend support"

# With scope
git commit -m "feat(engine): implement parallel decompression"

# With body and footer
git commit -m "fix: resolve memory leak in buffer pool

The buffer pool was not properly releasing buffers when capacity
exceeded the configured limit.

Fixes #123"

# Breaking change
git commit -m "feat!: change CompressConfig API

BREAKING CHANGE: backend field is now an Option<Backend>"
```

### Co-authorship

When pairing or using AI assistance:

```bash
git commit -m "feat: add awesome feature

Co-Authored-By: Oz <oz-agent@warp.dev>
Co-Authored-By: Alice <alice@example.com>"
```

## Release Process

### Creating a Release

#### 1. Create Release Branch

```bash
git checkout develop
git pull origin develop
git checkout -b release/0.2.0
```

#### 2. Prepare Release

```bash
# Update version in Cargo.toml
[workspace.package]
version = "0.2.0"

# Create/update CHANGELOG.md
# Update README.md (if needed)

git add Cargo.toml CHANGELOG.md
git commit -m "chore: prepare release 0.2.0"
git push origin release/0.2.0
```

#### 3. Create PR to Main

- Title: `Release 0.2.0`
- Description: Link to changelog, highlight key features
- Wait for CI to pass
- Get approval

#### 4. Merge and Tag

```bash
# Merge release PR to main
# Then tag the release
git checkout main
git pull origin main
git tag v0.2.0
git push origin v0.2.0

# This triggers GitHub Actions release workflow
# Builds binaries for all platforms
# Creates GitHub release
# Publishes to crates.io (if configured)
```

#### 5. Merge Back to Develop

```bash
git checkout develop
git pull origin develop
git merge main
git push origin develop
```

#### 6. Clean Up

```bash
git branch -d release/0.2.0
git push origin --delete release/0.2.0
```

### Hotfix Process

#### 1. Create Hotfix Branch

```bash
git checkout main
git pull origin main
git checkout -b hotfix/0.1.1
```

#### 2. Fix and Update Version

```bash
# Fix the critical issue
# Update version (patch bump)
# Update CHANGELOG.md

git add .
git commit -m "fix: critical security vulnerability"
git push origin hotfix/0.1.1
```

#### 3. Create PR to Main

- Title: `Hotfix 0.1.1 - Critical Security Fix`
- Description: Explain the issue and fix
- **Priority review required**

#### 4. Merge, Tag, and Backport

```bash
# After merge to main
git checkout main
git pull origin main
git tag v0.1.1
git push origin v0.1.1

# Merge to develop
git checkout develop
git pull origin develop
git merge main
git push origin develop

# Clean up
git branch -d hotfix/0.1.1
git push origin --delete hotfix/0.1.1
```

## Legal

### License

By contributing to CPAC, you agree that your contributions will be licensed under the `LicenseRef-CPAC-Research-Evaluation-1.0` license.

### Contribution Terms

By submitting a pull request or patch, you agree to grant BitConcepts, LLC a perpetual, irrevocable, worldwide, royalty-free license to your contributions as specified in the [LICENSE](LICENSE).

### Security Issues

See [SECURITY.md](SECURITY.md) for reporting security vulnerabilities. Do **not** file security issues as public GitHub issues.

## Questions?

- Open an issue for bugs or feature requests
- Start a discussion for questions
- Check existing issues and PRs before creating new ones

---

**Last Updated**: 2026-03-03  
**Version**: 2.0
