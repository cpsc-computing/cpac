# Branch Protection Rulesets

This document describes the recommended branch protection rulesets for the CPAC repository.

## Overview

CPAC uses GitFlow with two protected branches (`main` and `develop`) and supporting branches for features, bugfixes, releases, and hotfixes.

## Ruleset Configuration

### Ruleset 1: Protect `main` (Production)

**Ruleset Name**: `Protect main (Production)`

**Enforcement Status**: Active

**Bypass List**:
- Repository administrators (for emergency hotfixes only)

**Target Branches**:
- **Include**: `main`

**Branch Rules**:

#### ✅ Restrict deletions
- **Enabled**: Yes
- **Purpose**: Prevent accidental deletion of production branch

#### ✅ Require a pull request before merging
- **Enabled**: Yes
- **Required approvals**: 1
- **Dismiss stale reviews**: Yes
- **Require review from Code Owners**: No (unless CODEOWNERS file exists)
- **Purpose**: Ensure all changes to production are reviewed

#### ✅ Require status checks to pass
- **Enabled**: Yes
- **Required checks**:
  - `Test Suite / ubuntu-latest`
  - `Test Suite / macos-latest`
  - `Test Suite / windows-latest`
  - `Clippy`
  - `Rustfmt`
- **Require branches to be up to date**: Yes
- **Purpose**: Ensure all CI/CD checks pass before merge

#### ✅ Require linear history
- **Enabled**: Yes
- **Purpose**: Keep main history clean (squash merges only)

#### ✅ Block force pushes
- **Enabled**: Yes
- **Purpose**: Protect production history

#### ✅ Require signed commits
- **Enabled**: No (optional, enable if your team uses GPG signing)
- **Purpose**: Verify commit authenticity

#### ❌ Restrict creations
- **Enabled**: No
- **Purpose**: Branch already exists

#### ❌ Restrict updates
- **Enabled**: No
- **Purpose**: Allow merges from release/hotfix branches

### Ruleset 2: Protect `develop` (Integration)

**Ruleset Name**: `Protect develop (Integration)`

**Enforcement Status**: Active

**Bypass List**:
- Repository administrators (for emergency fixes only)

**Target Branches**:
- **Include**: `develop`

**Branch Rules**:

#### ✅ Restrict deletions
- **Enabled**: Yes
- **Purpose**: Prevent accidental deletion of integration branch

#### ✅ Require a pull request before merging
- **Enabled**: Yes
- **Required approvals**: 1
- **Dismiss stale reviews**: Yes
- **Require review from Code Owners**: No
- **Purpose**: Ensure all feature/bugfix changes are reviewed

#### ✅ Require status checks to pass
- **Enabled**: Yes
- **Required checks**:
  - `Test Suite / ubuntu-latest`
  - `Test Suite / macos-latest`
  - `Test Suite / windows-latest`
  - `Clippy`
  - `Rustfmt`
- **Require branches to be up to date**: Yes
- **Purpose**: Ensure all CI/CD checks pass before merge

#### ✅ Require linear history
- **Enabled**: Yes
- **Purpose**: Keep develop history clean (squash merges only)

#### ✅ Block force pushes
- **Enabled**: Yes
- **Purpose**: Protect development history

#### ✅ Require signed commits
- **Enabled**: No (optional)

#### ❌ Restrict creations
- **Enabled**: No

#### ❌ Restrict updates
- **Enabled**: No

### Ruleset 3: Release Branches

**Ruleset Name**: `Release Branches`

**Enforcement Status**: Active

**Bypass List**:
- (empty - no bypass needed for release branches)

**Target Branches**:
- **Include by pattern**: `release/*`

**Branch Rules**:

#### ✅ Restrict deletions
- **Enabled**: Yes (until merged to main and develop)
- **Purpose**: Protect release branches during preparation

#### ✅ Require status checks to pass
- **Enabled**: Yes
- **Required checks**:
  - `Test Suite / ubuntu-latest`
  - `Test Suite / macos-latest`
  - `Test Suite / windows-latest`
  - `Clippy`
  - `Rustfmt`
- **Purpose**: Ensure release is stable before merging to main

#### ❌ Require a pull request before merging
- **Enabled**: No
- **Purpose**: Allow direct commits for version bumps and changelog updates

#### ❌ Block force pushes
- **Enabled**: No
- **Purpose**: Allow rebasing if needed during release preparation

### Ruleset 4: Hotfix Branches

**Ruleset Name**: `Hotfix Branches`

**Enforcement Status**: Active

**Bypass List**:
- (empty)

**Target Branches**:
- **Include by pattern**: `hotfix/*`

**Branch Rules**:

#### ✅ Restrict deletions
- **Enabled**: Yes (until merged to main and develop)
- **Purpose**: Protect hotfix branches during critical fixes

#### ✅ Require status checks to pass
- **Enabled**: Yes
- **Required checks**:
  - `Test Suite / ubuntu-latest`
  - `Test Suite / macos-latest`
  - `Test Suite / windows-latest`
  - `Clippy`
  - `Rustfmt`
- **Purpose**: Ensure hotfix doesn't introduce new issues

#### ❌ Require a pull request before merging
- **Enabled**: No
- **Purpose**: Allow rapid iteration on critical fixes

#### ❌ Block force pushes
- **Enabled**: No
- **Purpose**: Allow rebasing if needed

## Implementation Steps

### Step 1: Create `develop` Branch

```bash
# From main branch
git checkout main
git pull origin main
git checkout -b develop
git push origin develop
```

### Step 2: Configure Rulesets in GitHub

1. Go to **Settings** → **Rules** → **Rulesets**
2. Click **New ruleset** → **New branch ruleset**
3. Configure each ruleset as described above
4. Save and activate

### Step 3: Set Default Branch

1. Go to **Settings** → **General**
2. Under **Default branch**, change from `main` to `develop`
3. This ensures new PRs default to `develop`

### Step 4: Verify Protection

```bash
# Try to push directly to main (should fail)
git checkout main
echo "test" >> test.txt
git add test.txt
git commit -m "test"
git push origin main  # Should be rejected

# Try to push directly to develop (should fail)
git checkout develop
git push origin develop  # Should be rejected
```

## Status Check Names

Based on `.github/workflows/ci.yml`, the following status checks should be required:

### Test Suite
- `Test Suite / ubuntu-latest`
- `Test Suite / macos-latest`
- `Test Suite / windows-latest`

### Code Quality
- `Clippy`
- `Rustfmt`

### Optional
- `Code Coverage` (nice to have, but may slow down merges)
- `Quick Benchmark` (only runs on PRs)

## Merge Strategy

### For `main`:
- **Merge method**: Squash and merge
- **Delete branch after merge**: Yes
- **Require linear history**: Yes

### For `develop`:
- **Merge method**: Squash and merge
- **Delete branch after merge**: Yes
- **Require linear history**: Yes

## Bypass Permissions

Only repository administrators should have bypass permissions, and only for:
- Emergency hotfixes when CI is down
- Repository maintenance tasks
- Critical security patches that can't wait for CI

**Never bypass for**:
- Regular feature development
- Non-critical bug fixes
- Documentation updates

## Additional Recommendations

### 1. Enable Branch Name Restrictions

If you want to enforce branch naming conventions:

**Restrict branch names** (under each ruleset):
- Pattern: `^(main|develop|feature\/.*|bugfix\/.*|release\/.*|hotfix\/.*)$`
- This allows only: `main`, `develop`, `feature/*`, `bugfix/*`, `release/*`, `hotfix/*`

### 2. Enable Commit Message Validation

Consider using a GitHub Action to validate conventional commit format:

```yaml
# .github/workflows/commit-lint.yml
name: Commit Lint
on: [pull_request]
jobs:
  commitlint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: wagoid/commitlint-github-action@v5
```

### 3. Enable CODEOWNERS

Create `.github/CODEOWNERS`:

```
# Default owners for everything
* @cpsc-computing/maintainers

# Specific ownership
crates/cpac-crypto/* @cpsc-computing/security-team
.github/workflows/* @cpsc-computing/devops-team
```

### 4. Enable Auto-delete Branches

**Settings** → **General** → **Pull Requests**:
- ✅ Automatically delete head branches

## Troubleshooting

### CI Status Checks Not Appearing

If status checks don't appear in the required checks list:
1. Ensure CI has run at least once on the target branch
2. Check that the status check names match exactly (case-sensitive)
3. Wait a few minutes for GitHub to index the checks

### Unable to Merge Release Branch to Main

Ensure:
1. All required status checks pass
2. You have approval (if required)
3. Branch is up to date with main
4. No merge conflicts exist

### Bypass Not Working

Verify:
1. You're listed in the bypass list
2. You have appropriate permissions (admin/maintain)
3. The ruleset is enforcing on your repository visibility (public vs private)

## References

- [GitHub Branch Protection Rules](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets)
- [GitFlow Workflow](https://www.atlassian.com/git/tutorials/comparing-workflows/gitflow-workflow)
- [Semantic Versioning](https://semver.org/)

---

**Last Updated**: 2026-03-03  
**Version**: 1.0
