# CPAC Release Guide

## Overview

The CPAC project uses GitHub Actions for automated releases across multiple platforms. This document explains how to create releases and what gets built.

## Supported Platforms

Each release automatically builds binaries for:

- **Linux x86_64** (GNU)
- **Linux ARM64** (GNU) - via cross-compilation
- **macOS Intel** (x86_64)
- **macOS Apple Silicon** (ARM64)
- **Windows x86_64** (MSVC)

## Creating a Release

### Automatic Release (Recommended)

Create and push a version tag:

```bash
# Update version in all Cargo.toml files (if needed)
# Then create and push the tag:
git tag v0.3.0
git push origin v0.3.0
```

The GitHub Actions workflow will automatically:
1. Create a GitHub release with the tag
2. Build binaries for all platforms
3. Generate SHA256 checksums
4. Upload all artifacts to the release
5. Publish crates to crates.io (if `CARGO_REGISTRY_TOKEN` secret is set)

### Manual Release (Workflow Dispatch)

You can also trigger a release manually from the GitHub Actions UI:

1. Go to **Actions** → **Release** workflow
2. Click **Run workflow**
3. Enter the tag (e.g., `v0.1.0`)
4. Click **Run workflow**

## Release Workflow Details

### Build Process

Each platform build:
- Uses the `release` profile from `Cargo.toml`:
  - Link-Time Optimization (LTO): `fat`
  - Single codegen unit for maximum optimization
  - Stripped symbols for smaller binaries
  - Panic mode: `abort`
- Runs all tests before building
- Packages binaries:
  - Unix: `.tar.gz`
  - Windows: `.zip`

### Artifacts Generated

For release `v0.3.0`, the following files are created:

```
cpac-v0.3.0-x86_64-unknown-linux-gnu.tar.gz
cpac-v0.3.0-aarch64-unknown-linux-gnu.tar.gz
cpac-v0.3.0-x86_64-apple-darwin.tar.gz
cpac-v0.3.0-aarch64-apple-darwin.tar.gz
cpac-v0.3.0-x86_64-pc-windows-msvc.zip
checksums.txt
```

### Checksums

The `checksums.txt` file contains SHA256 hashes for verification:

```bash
# Verify downloaded binary
sha256sum -c checksums.txt
```

## Publishing to crates.io

### Setup

To enable automatic crates.io publishing:

1. Get your crates.io API token from https://crates.io/settings/tokens
2. Add it as a GitHub secret named `CARGO_REGISTRY_TOKEN`:
   - Go to repository **Settings** → **Secrets and variables** → **Actions**
   - Create new repository secret
   - Name: `CARGO_REGISTRY_TOKEN`
   - Value: Your crates.io token

### Publishing Order

Crates are published in dependency order with 30-second delays to allow crates.io indexing:

1. `cpac-types`
2. `cpac-lzham-sys`
3. `cpac-lizard-sys`
4. `cpac-ssr`
5. `cpac-transforms`
6. `cpac-entropy`
7. `cpac-frame`
8. `cpac-dag`
9. `cpac-crypto`
10. `cpac-dict`
11. `cpac-conditioning`
12. `cpac-predict`
13. `cpac-engine`
14. `cpac-streaming`
15. `cpac-archive`
16. `cpac-cas`
17. `cpac-domains`
18. `cpac-lab`
19. `cpac-transcode`
20. `cpac-ffi`
21. `cpac-cli`

## Version Management

### Updating Version Numbers

Update the version in `Cargo.toml` workspace:

```toml
[workspace.package]
version = "0.3.0"  # Change this
```

All workspace crates inherit this version, so you only need to update it once.

### Versioning Strategy

Follow [Semantic Versioning](https://semver.org/):

- **Major (1.0.0)**: Breaking API changes
- **Minor (0.1.0)**: New features, backward compatible
- **Patch (0.0.1)**: Bug fixes, backward compatible

For pre-1.0 versions:
- Minor version bumps MAY include breaking changes
- Patch version bumps should be backward compatible

## Release Checklist

Before creating a release:

- [ ] All CI/CD tests passing on `main` branch
- [ ] Version updated in `Cargo.toml` (if needed)
- [ ] `CHANGELOG.md` updated with release notes (if exists)
- [ ] `BENCHMARKING.md` updated with latest results
- [ ] Documentation reviewed and updated
- [ ] README.md reflects current features
- [ ] All clippy warnings resolved
- [ ] Code formatted with `cargo fmt`

## Testing Releases Locally

### Build for Current Platform

```bash
# Build release binary
cargo build --release -p cpac-cli

# Test binary
./target/release/cpac --version
./target/release/cpac benchmark test.txt --quick
```

### Cross-Compile for Other Platforms

Install `cross` for cross-compilation:

```bash
cargo install cross --git https://github.com/cross-rs/cross
```

Build for specific target:

```bash
# Linux ARM64
cross build --release --target aarch64-unknown-linux-gnu -p cpac-cli

# Windows (from Linux/macOS)
cross build --release --target x86_64-pc-windows-gnu -p cpac-cli
```

## Troubleshooting

### Release Creation Failed

**Symptom**: GitHub Actions fails at "Create Release" step

**Solutions**:
- Ensure tag follows `v*.*.*` format (e.g., `v0.1.0`)
- Check that tag doesn't already exist: `git tag -l`
- Verify `GITHUB_TOKEN` permissions in repository settings

### Build Failed for Specific Platform

**Symptom**: One platform build fails, others succeed

**Solutions**:
- Check platform-specific dependencies in `Cargo.toml`
- Review build logs for compilation errors
- Test locally with cross-compilation
- Ensure all features are platform-agnostic or properly gated

### Crates.io Publishing Failed

**Symptom**: GitHub release succeeds but crates.io publish fails

**Solutions**:
- Verify `CARGO_REGISTRY_TOKEN` secret is set correctly
- Check crates.io for name conflicts
- Ensure all `Cargo.toml` metadata is complete:
  - `license`
  - `description`
  - `repository`
  - `authors`
- Review crates.io publishing policies

### Checksum Generation Failed

**Symptom**: Checksums not uploaded to release

**Solutions**:
- Check that all platform builds completed successfully
- Verify artifact naming matches expected pattern
- Review download-artifact step logs

## Advanced Configuration

### Adding New Platforms

To add support for additional platforms, edit `.github/workflows/release.yml`:

```yaml
matrix:
  include:
    # Add new target
    - target: x86_64-unknown-freebsd
      os: ubuntu-latest
      archive: tar.gz
      cross: true
```

### Custom Build Profiles

To add specialized build profiles (e.g., `release-small`), edit `Cargo.toml`:

```toml
[profile.release-small]
inherits = "release"
opt-level = "z"
lto = true
```

Then update workflow to use the profile:

```bash
cargo build --profile release-small --target ${{ matrix.target }} -p cpac-cli
```

## Distribution Channels

### GitHub Releases (Primary)

Users download binaries directly from GitHub releases page:
```
https://github.com/cpsc-computing/cpac/releases
```

### crates.io (For Rust Developers)

Install via cargo:
```bash
cargo install cpac-cli
```

### Future: Package Managers

Consider adding distribution via:
- **Homebrew** (macOS/Linux): Create formula
- **Chocolatey** (Windows): Create package
- **Snap** (Linux): Create snapcraft.yaml
- **APT/RPM** (Linux): Create .deb/.rpm packages

## License

All release artifacts are distributed under the same license as the source code:
`LicenseRef-CPAC-Research-Evaluation-1.0`

Ensure license terms are acceptable before publishing to public repositories.

---

**Last Updated**: 2026-03-15  
**Workflow Version**: 1.1
