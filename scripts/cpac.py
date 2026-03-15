#!/usr/bin/env python3
# Copyright (c) 2026 BitConcepts, LLC
# SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
#
# This file is part of the CPAC compression engine.
# For full license terms, see LICENSE in the repository root.
"""CPAC Unified Build System — Single Cross-Platform CLI.

Consolidated build system with automatic venv management.
Use shell.ps1 (Windows) or shell.sh (Linux/macOS) for easy invocation.

Direct usage:
  python cpac.py build --release
  python cpac.py test
  python cpac.py bench .work/benchdata/silesia/dickens --quick

"""

import argparse
import hashlib
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from typing import List, Optional


# ==============================================================================
# VENV BOOTSTRAP: Ensure running in venv (must happen before other imports)
# ==============================================================================

REPO_ROOT_BOOTSTRAP = pathlib.Path(__file__).resolve().parent.parent
_WORK_DIR_BOOTSTRAP = REPO_ROOT_BOOTSTRAP / ".work"
VENV_DIR_BOOTSTRAP = _WORK_DIR_BOOTSTRAP / "env"


def _get_venv_python():
    """Get venv Python executable (cross-platform)."""
    if platform.system() == "Windows":
        return VENV_DIR_BOOTSTRAP / "Scripts" / "python.exe"
    else:
        return VENV_DIR_BOOTSTRAP / "bin" / "python3"


def _is_in_venv():
    """Check if currently running in the venv."""
    venv_python = _get_venv_python()
    if not venv_python.exists():
        return False
    return pathlib.Path(sys.prefix).resolve() == VENV_DIR_BOOTSTRAP.resolve()


def _activate_venv_and_reexec():
    """Re-execute this script inside the venv."""
    venv_python = _get_venv_python()

    if not venv_python.exists():
        print(f"ERROR: Virtual environment not found at {VENV_DIR_BOOTSTRAP}", file=sys.stderr)
        print("Run shell script first:", file=sys.stderr)
        if platform.system() == "Windows":
            print("  .\\shell.ps1", file=sys.stderr)
        else:
            print("  ./shell.sh", file=sys.stderr)
        sys.exit(1)

    # Re-exec with venv Python
    os.execv(str(venv_python), [str(venv_python)] + sys.argv)


# Ensure venv activation (will re-exec if not in venv)
if not _is_in_venv():
    _activate_venv_and_reexec()


# ==============================================================================
# MAIN CLI IMPLEMENTATION
# ==============================================================================


class CommandError(Exception):
    """Custom exception for CLI-friendly errors."""
    pass


REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
_WORK_DIR = REPO_ROOT / ".work"
VENV_DIR = _WORK_DIR / "env"
BENCHDATA_DIR = _WORK_DIR / "benchdata"
CORPUS_DIR = REPO_ROOT / "benches" / "cpac" / "corpora"
PROFILE_DIR = REPO_ROOT / "benches" / "cpac" / "profiles"


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def run(cmd: List[str], cwd: Optional[pathlib.Path] = None,
        env: Optional[dict] = None, capture: bool = False) -> subprocess.CompletedProcess:
    """Run a command, echoing it, raising on failure."""
    cwd = cwd or REPO_ROOT
    cmd_str = " ".join(cmd)
    print(f"[cpac] RUN ({cwd}): {cmd_str}")
    if capture:
        completed = subprocess.run(cmd, cwd=str(cwd), env=env,
                                   capture_output=True, text=True)
    else:
        completed = subprocess.run(cmd, cwd=str(cwd), env=env)
    if completed.returncode != 0:
        raise subprocess.CalledProcessError(completed.returncode, cmd)
    return completed


def ensure_tool(name: str) -> str:
    """Ensure a tool is on PATH, return its path."""
    path = shutil.which(name)
    if path is None:
        print(f"[cpac] ERROR: required tool '{name}' not found in PATH", file=sys.stderr)
        sys.exit(1)
    return path


def resolve_cargo() -> str:
    """Resolve the cargo executable, working around broken rustup shims on Windows."""
    # Try rustup toolchain binary directly (works around 0-byte shim issue)
    if platform.system() == "Windows":
        home = pathlib.Path(os.environ.get("USERPROFILE", ""))
        toolchain_cargo = home / ".rustup" / "toolchains" / "stable-x86_64-pc-windows-msvc" / "bin" / "cargo.exe"
        if toolchain_cargo.exists() and toolchain_cargo.stat().st_size > 0:
            return str(toolchain_cargo)
    # Fall back to PATH
    cargo = shutil.which("cargo")
    if cargo is None:
        print("[cpac] ERROR: cargo not found. Install Rust: https://rustup.rs/", file=sys.stderr)
        sys.exit(1)
    return cargo


def resolve_cpac_binary() -> str:
    """Resolve the CPAC release binary."""
    ext = ".exe" if platform.system() == "Windows" else ""
    binary = REPO_ROOT / "target" / "release" / f"cpac{ext}"
    if not binary.exists():
        print(f"[cpac] ERROR: Release binary not found at {binary}", file=sys.stderr)
        print("  Run: shell.ps1 build --release", file=sys.stderr)
        sys.exit(1)
    return str(binary)


def cargo_env() -> dict:
    """Build environment dict with Rust toolchain on PATH."""
    env = os.environ.copy()
    if platform.system() == "Windows":
        home = pathlib.Path(os.environ.get("USERPROFILE", ""))
        toolchain_bin = home / ".rustup" / "toolchains" / "stable-x86_64-pc-windows-msvc" / "bin"
        if toolchain_bin.exists():
            env["PATH"] = f"{toolchain_bin}{os.pathsep}{env.get('PATH', '')}"
    return env


# ---------------------------------------------------------------------------
# Commands: Build & Quality
# ---------------------------------------------------------------------------


def cmd_update(args: argparse.Namespace) -> None:
    """Update Cargo dependencies (runs cargo update)."""
    cargo = resolve_cargo()
    cmd = [cargo, "update"]
    if args.package:
        cmd.extend(["-p", args.package])
    if args.dry_run:
        cmd.append("--dry-run")
    run(cmd, env=cargo_env())


def cmd_build(args: argparse.Namespace) -> None:
    """Build the CPAC workspace."""
    cargo = resolve_cargo()
    cmd = [cargo, "build", "--workspace"]
    if args.release:
        cmd.append("--release")
    if args.package:
        cmd = [cargo, "build", "-p", args.package]
        if args.release:
            cmd.append("--release")
    run(cmd, env=cargo_env())


def cmd_test(args: argparse.Namespace) -> None:
    """Run workspace tests."""
    cargo = resolve_cargo()
    cmd = [cargo, "test"]
    if args.package:
        cmd.extend(["-p", args.package])
    else:
        cmd.append("--workspace")
    if args.release:
        cmd.append("--release")
    # Append any extra args after --
    if args.extra:
        cmd.append("--")
        cmd.extend(args.extra)
    run(cmd, env=cargo_env())


def cmd_clippy(args: argparse.Namespace) -> None:
    """Run clippy lints."""
    cargo = resolve_cargo()
    cmd = [cargo, "clippy", "--workspace", "--", "-D", "warnings"]
    run(cmd, env=cargo_env())


def cmd_fmt(args: argparse.Namespace) -> None:
    """Run cargo fmt."""
    cargo = resolve_cargo()
    cmd = [cargo, "fmt", "--all"]
    if args.check:
        cmd.extend(["--", "--check"])
    run(cmd, env=cargo_env())


def cmd_check(args: argparse.Namespace) -> None:
    """Run full presubmit: build + test + clippy + fmt check."""
    print("=== CPAC Presubmit Check ===")
    cargo = resolve_cargo()
    env = cargo_env()

    print("\n[1/4] Build...")
    run([cargo, "build", "--workspace"], env=env)

    print("\n[2/4] Tests...")
    run([cargo, "test", "--workspace"], env=env)

    print("\n[3/4] Clippy...")
    run([cargo, "clippy", "--workspace", "--", "-D", "warnings"], env=env)

    print("\n[4/4] Format check...")
    run([cargo, "fmt", "--all", "--", "--check"], env=env)

    print("\n=== Presubmit PASSED ===")


# ---------------------------------------------------------------------------
# Profile & Corpus Resolution
# ---------------------------------------------------------------------------


def _parse_yaml_simple(path: pathlib.Path) -> dict:
    """Minimal YAML parser for corpus/profile config files.

    Handles scalar fields and list items (- value).  No external dependency.
    """
    lines = path.read_text(encoding="utf-8").splitlines()
    cfg: dict = {}
    current_list_key: Optional[str] = None

    for line in lines:
        stripped = line.rstrip()

        # Skip comments and blank lines
        if not stripped or stripped.lstrip().startswith("#"):
            if current_list_key and not stripped.lstrip().startswith("#"):
                current_list_key = None
            continue

        # List item under current key
        if current_list_key and stripped.startswith("  - "):
            val = stripped.lstrip(" -").strip().split("#")[0].strip().strip("'\"")
            if val:
                cfg.setdefault(current_list_key, []).append(val)
            continue

        # End of list block
        if current_list_key and stripped and not stripped.startswith(" "):
            current_list_key = None

        # Indented non-list line (nested map) — skip for simplicity
        if stripped.startswith(" ") and not stripped.startswith("  - "):
            continue

        # Top-level key: value
        if ": " in stripped and not stripped.startswith(" "):
            key, val = stripped.split(": ", 1)
            key = key.strip()
            # Strip trailing inline comments (" #" outside brackets/quotes)
            if " #" in val:
                # Find the comment boundary — naive split is safe for our
                # YAML subset (no # inside values)
                bracket_depth = 0
                for ci, ch in enumerate(val):
                    if ch == '[':
                        bracket_depth += 1
                    elif ch == ']':
                        bracket_depth -= 1
                    elif ch == '#' and bracket_depth == 0 and ci > 0 and val[ci - 1] == ' ':
                        val = val[:ci - 1]
                        break
            val = val.strip().strip("'\"")
            if val:
                # Inline list: [a, b]
                if val.startswith("[") and val.endswith("]"):
                    items = [v.strip().strip("'\"")
                             for v in val[1:-1].split(",") if v.strip()]
                    cfg[key] = items
                else:
                    cfg[key] = val
            continue

        # Key with no inline value — start of list or map block
        if stripped.endswith(":") and not stripped.startswith(" "):
            key = stripped[:-1].strip()
            current_list_key = key
            cfg.setdefault(key, [])
            continue

    return cfg


def resolve_profile(profile_id: str) -> dict:
    """Find and parse a benchmark profile YAML by id.

    Scans all files in benches/cpac-profiles/ and returns the one whose
    'id' field matches *profile_id*.
    """
    if not PROFILE_DIR.is_dir():
        raise CommandError(f"Profile directory not found: {PROFILE_DIR}")

    for p in sorted(PROFILE_DIR.glob("*.yaml")):
        cfg = _parse_yaml_simple(p)
        if cfg.get("id") == profile_id:
            return cfg
    for p in sorted(PROFILE_DIR.glob("*.yml")):
        cfg = _parse_yaml_simple(p)
        if cfg.get("id") == profile_id:
            return cfg

    available = []
    for p in sorted(list(PROFILE_DIR.glob("*.yaml")) + list(PROFILE_DIR.glob("*.yml"))):
        cfg = _parse_yaml_simple(p)
        if "id" in cfg:
            available.append(cfg["id"])
    raise CommandError(
        f"Unknown profile: '{profile_id}'\n"
        f"  Available profiles: {', '.join(available) if available else '(none)'}\n"
        f"  Profile directory:  {PROFILE_DIR}"
    )


def resolve_corpus_config(corpus_id: str) -> dict:
    """Find and parse a corpus YAML by id.

    Scans benches/corpora/corpus_*.yaml for matching 'id' field.
    """
    config_path = CORPUS_DIR / f"corpus_{corpus_id}.yaml"
    if config_path.exists():
        return _parse_yaml_simple(config_path)
    # Fallback: scan all files
    for p in sorted(CORPUS_DIR.glob("*.yaml")):
        cfg = _parse_yaml_simple(p)
        if cfg.get("id") == corpus_id:
            return cfg
    raise CommandError(f"Unknown corpus: '{corpus_id}' (not found in {CORPUS_DIR})")


def corpus_data_dir(corpus_cfg: dict) -> pathlib.Path:
    """Return the local data directory for a corpus config."""
    subdir = corpus_cfg.get("target_subdir", corpus_cfg.get("id", "unknown"))
    return BENCHDATA_DIR / subdir


def corpora_for_profile(profile_id: str) -> list:
    """Return list of corpus IDs for a given profile."""
    prof = resolve_profile(profile_id)
    return prof.get("corpora", [])


# ---------------------------------------------------------------------------
# Commands: Benchmark
# ---------------------------------------------------------------------------


def cmd_bench(args: argparse.Namespace) -> None:
    """Run CPAC benchmark on a file (wraps cpac CLI bench subcommand)."""
    binary = resolve_cpac_binary()
    cmd = [binary, "benchmark", str(args.input)]
    if args.quick:
        cmd.append("--quick")
    elif args.full:
        cmd.append("--full")
    if args.track1:
        cmd.append("--track1")
    if args.discovery:
        cmd.append("--discovery")
    if args.skip_baselines:
        cmd.append("--skip-baselines")
    if args.json:
        cmd.append("--json")
    if args.iterations:
        cmd.extend(["-n", str(args.iterations)])
    run(cmd)


def cmd_benchmark_all(args: argparse.Namespace) -> None:
    """Profile-driven benchmark suite.

    Resolves a benchmark profile by id, iterates over all corpora listed
    in the profile, and runs the cpac benchmark on each file.
    Results are saved to .work/benchmarks/.
    """
    binary = resolve_cpac_binary()
    profile_id = args.profile
    profile = resolve_profile(profile_id)
    corpus_ids = profile.get("corpora", [])

    if not corpus_ids:
        raise CommandError(f"Profile '{profile_id}' has no corpora listed.")

    # Profile knobs
    iterations = int(profile.get("iterations", 3))
    timeout_sec = int(profile.get("timeout_seconds", 600))
    track1 = str(profile.get("track1", "true")).lower() in ("true", "1", "yes")
    skip_baselines = str(profile.get("skip_baselines", "false")).lower() in ("true", "1", "yes")
    large_file_threshold = int(profile.get("large_file_threshold_mb", 0)) * 1024 * 1024
    large_file_iters = int(profile.get("large_file_iterations", iterations))
    # Backend / level configuration from profile YAML
    backends_list = profile.get("backends", [])  # e.g. [zstd, brotli, gzip, lzma, raw]
    cpac_levels = profile.get("cpac_levels", [])  # e.g. [fast, default, best]
    large_file_levels = profile.get("large_file_levels", [])  # reduced levels for big files
    very_large_threshold = int(profile.get("very_large_file_threshold_mb", 0)) * 1024 * 1024
    very_large_levels = profile.get("very_large_file_levels", [])  # minimal levels for huge files
    discovery = str(profile.get("discovery", "false")).lower() in ("true", "1", "yes")
    # CLI overrides
    if args.skip_baselines:
        skip_baselines = True

    # Output directory
    timestamp = time.strftime("%Y-%m-%d_%H-%M")
    out_dir = _WORK_DIR / "benchmarks" / f"benchmark-{profile_id}_{timestamp}"
    out_dir.mkdir(parents=True, exist_ok=True)

    print(f"CPAC Benchmark Suite (profile: {profile_id})")
    print(f"  Description: {profile.get('description', '')}")
    print(f"  Corpora:     {', '.join(corpus_ids)}")
    print(f"  Iterations:  {iterations}")
    print(f"  Timeout:     {timeout_sec}s per file")
    print(f"  Track 1:     {track1}")
    if backends_list:
        print(f"  Backends:    {', '.join(str(b) for b in backends_list)}")
    if cpac_levels:
        print(f"  CPAC levels: {', '.join(str(l) for l in cpac_levels)}")
    print(f"  Discovery:   {discovery}")
    print(f"  Output:      {out_dir}")
    print()

    summary_path = out_dir / "summary.txt"
    total_files = 0
    total_ok = 0
    missing_corpora = []

    with summary_path.open("w", encoding="utf-8") as summary:
        summary.write(f"CPAC Benchmark Results — {timestamp} (profile: {profile_id})\n")
        summary.write(f"Iterations: {iterations}\n")
        summary.write("=" * 70 + "\n\n")

        for corpus_id in corpus_ids:
            try:
                corpus_cfg = resolve_corpus_config(corpus_id)
            except CommandError:
                print(f"  WARNING: corpus '{corpus_id}' not found in {CORPUS_DIR}, skipping")
                missing_corpora.append(corpus_id)
                continue

            data_dir = corpus_data_dir(corpus_cfg)
            if not data_dir.exists() or not any(data_dir.rglob("*")):
                print(f"  WARNING: corpus '{corpus_id}' not downloaded ({data_dir}), skipping")
                print(f"           Download with: shell.ps1 download-corpus --corpus {corpus_id}")
                missing_corpora.append(corpus_id)
                continue

            # Collect files from corpus directory, respecting exclude_extensions
            exclude_exts = set(
                corpus_cfg.get("exclude_extensions", [])
            )
            exclude_exts.add(".cpac")  # always skip .cpac
            files = sorted(
                f for f in data_dir.rglob("*")
                if f.is_file() and f.suffix not in exclude_exts
            )
            if not files:
                print(f"  WARNING: no files in {data_dir}, skipping")
                missing_corpora.append(corpus_id)
                continue

            total_size = sum(f.stat().st_size for f in files)
            print(f"[{corpus_id}] {len(files)} files, {total_size / (1024 * 1024):.1f} MB")
            summary.write(f"\n=== {corpus_id} ({len(files)} files, "
                          f"{total_size / (1024 * 1024):.1f} MB) ===\n\n")

            corpus_out = out_dir / corpus_id
            corpus_out.mkdir(parents=True, exist_ok=True)

            for f in files:
                total_files += 1
                file_size = f.stat().st_size
                size_kb = file_size / 1024
                rel = f.relative_to(data_dir)

                # Adaptive iterations and levels: fewer for large files
                file_iters = iterations
                file_levels = cpac_levels
                if very_large_threshold and file_size > very_large_threshold:
                    file_iters = large_file_iters
                    if very_large_levels:
                        file_levels = very_large_levels
                elif large_file_threshold and file_size > large_file_threshold:
                    file_iters = large_file_iters
                    if large_file_levels:
                        file_levels = large_file_levels

                print(f"  {rel} ({size_kb:.1f} KB)...", end=" ", flush=True)

                cmd = [binary, "benchmark", str(f), "-n", str(file_iters)]
                if track1:
                    cmd.append("--track1")
                if skip_baselines:
                    cmd.append("--skip-baselines")
                if backends_list:
                    cmd.extend(["--backends", ",".join(str(b) for b in backends_list)])
                if file_levels:
                    cmd.extend(["--levels", ",".join(str(l) for l in file_levels)])
                if discovery:
                    cmd.append("--discovery")

                log_name = str(rel).replace(os.sep, "_").replace("/", "_")
                log_file = corpus_out / f"{log_name}.txt"
                try:
                    result = subprocess.run(
                        cmd, cwd=str(REPO_ROOT),
                        capture_output=True, text=True, timeout=timeout_sec,
                    )
                    log_file.write_text(result.stdout, encoding="utf-8")
                    summary.write(result.stdout + "\n")
                    print("OK")
                    total_ok += 1
                except subprocess.TimeoutExpired:
                    print("TIMEOUT")
                    summary.write(f"TIMEOUT: {corpus_id}/{rel}\n\n")
                except Exception as e:
                    print(f"FAILED: {e}")
                    summary.write(f"ERROR: {corpus_id}/{rel}: {e}\n\n")

    print(f"\n{'=' * 50}")
    print(f"Results: {total_ok}/{total_files} files OK")
    if missing_corpora:
        print(f"Missing: {', '.join(missing_corpora)}")
    print(f"Output:  {out_dir}")
    print(f"Summary: {summary_path}")


def _archive_mode_per_file(binary: str, files: list, timeout: int) -> dict:
    """Compress each file independently with CPAC and sum compressed sizes."""
    total_orig = 0
    total_comp = 0
    t0 = time.time()
    for f in files:
        data = f.read_bytes()
        total_orig += len(data)
        out_path = f.with_suffix(f.suffix + ".cpac")
        try:
            result = subprocess.run(
                [binary, "compress", str(f), "-o", str(out_path), "--force"],
                capture_output=True, text=True, timeout=timeout,
            )
            if result.returncode == 0 and out_path.exists():
                total_comp += out_path.stat().st_size
            else:
                total_comp += len(data)  # fallback: no gain
        except Exception:
            total_comp += len(data)
        finally:
            if out_path.exists():
                out_path.unlink(missing_ok=True)
    elapsed = time.time() - t0
    return {"orig": total_orig, "comp": total_comp, "time": elapsed}


def _archive_mode_cpar(binary: str, corpus_dir: pathlib.Path, timeout: int) -> dict:
    """Create a CPAR archive from a directory."""
    out_path = corpus_dir.with_suffix(".cpar")
    total_orig = sum(f.stat().st_size for f in corpus_dir.rglob("*") if f.is_file())
    t0 = time.time()
    try:
        result = subprocess.run(
            [binary, "archive-create", str(corpus_dir), "-o", str(out_path)],
            capture_output=True, text=True, timeout=timeout,
        )
        elapsed = time.time() - t0
        if result.returncode == 0 and out_path.exists():
            total_comp = out_path.stat().st_size
        else:
            total_comp = total_orig
    except Exception:
        elapsed = time.time() - t0
        total_comp = total_orig
    finally:
        if out_path.exists():
            out_path.unlink(missing_ok=True)
    return {"orig": total_orig, "comp": total_comp, "time": elapsed}


def _archive_mode_concat_cpac(binary: str, files: list, timeout: int) -> dict:
    """Concatenate all files and compress as single blob with CPAC.

    Simulates solid-archive upper bound for cross-file redundancy.
    """
    total_orig = 0
    concat = bytearray()
    for f in files:
        data = f.read_bytes()
        total_orig += len(data)
        concat.extend(data)
    # Write concat to temp file
    tmp = pathlib.Path(tempfile.mktemp(suffix=".concat"))
    out_path = tmp.with_suffix(".concat.cpac")
    try:
        tmp.write_bytes(bytes(concat))
        t0 = time.time()
        result = subprocess.run(
            [binary, "compress", str(tmp), "-o", str(out_path), "--force"],
            capture_output=True, text=True, timeout=timeout,
        )
        elapsed = time.time() - t0
        if result.returncode == 0 and out_path.exists():
            total_comp = out_path.stat().st_size
        else:
            total_comp = total_orig
    except Exception:
        elapsed = time.time() - t0 if 't0' in dir() else 0
        total_comp = total_orig
    finally:
        tmp.unlink(missing_ok=True)
        out_path.unlink(missing_ok=True)
    return {"orig": total_orig, "comp": total_comp, "time": elapsed}


def _archive_mode_tar_zstd(files: list, corpus_dir: pathlib.Path, timeout: int) -> dict:
    """Create tar archive and compress with zstd -3."""
    import tarfile
    import io
    total_orig = sum(f.stat().st_size for f in files)
    tar_buf = io.BytesIO()
    with tarfile.open(fileobj=tar_buf, mode="w") as tar:
        for f in files:
            rel = f.relative_to(corpus_dir)
            tar.add(str(f), arcname=str(rel))
    tar_data = tar_buf.getvalue()
    tar_tmp = pathlib.Path(tempfile.mktemp(suffix=".tar"))
    zst_tmp = tar_tmp.with_suffix(".tar.zst")
    try:
        tar_tmp.write_bytes(tar_data)
        t0 = time.time()
        # Try Python zstandard first
        try:
            import zstandard
            cctx = zstandard.ZstdCompressor(level=3)
            comp_data = cctx.compress(tar_data)
            zst_tmp.write_bytes(comp_data)
        except ImportError:
            # Fallback to zstd CLI
            zstd_bin = shutil.which("zstd")
            if zstd_bin:
                subprocess.run(
                    [zstd_bin, "-3", str(tar_tmp), "-o", str(zst_tmp), "--force"],
                    capture_output=True, timeout=timeout,
                )
            else:
                # No zstd available — skip
                return {"orig": total_orig, "comp": total_orig, "time": 0, "skip": True}
        elapsed = time.time() - t0
        total_comp = zst_tmp.stat().st_size if zst_tmp.exists() else total_orig
    except Exception:
        elapsed = 0
        total_comp = total_orig
    finally:
        tar_tmp.unlink(missing_ok=True)
        zst_tmp.unlink(missing_ok=True)
    return {"orig": total_orig, "comp": total_comp, "time": elapsed}


def _archive_mode_tar_gzip(files: list, corpus_dir: pathlib.Path) -> dict:
    """Create tar archive and compress with gzip -9."""
    import tarfile
    import io
    import gzip
    total_orig = sum(f.stat().st_size for f in files)
    tar_buf = io.BytesIO()
    with tarfile.open(fileobj=tar_buf, mode="w") as tar:
        for f in files:
            rel = f.relative_to(corpus_dir)
            tar.add(str(f), arcname=str(rel))
    tar_data = tar_buf.getvalue()
    t0 = time.time()
    comp_data = gzip.compress(tar_data, compresslevel=9)
    elapsed = time.time() - t0
    return {"orig": total_orig, "comp": len(comp_data), "time": elapsed}


def _fmt_ratio(orig: int, comp: int) -> str:
    """Format compression ratio as 'N.NNx'."""
    if comp == 0:
        return "inf"
    return f"{orig / comp:.2f}x"


def _fmt_savings(orig: int, comp: int) -> str:
    """Format savings as percentage."""
    if orig == 0:
        return "0.0%"
    return f"{(1 - comp / orig) * 100:.1f}%"


def cmd_benchmark_archive(args: argparse.Namespace) -> None:
    """Archive benchmark: compare per-file vs CPAR vs solid vs tar baselines.

    Tests how well CPAC exploits cross-file redundancy when archiving
    multiple files from the same corpus together.
    """
    binary = resolve_cpac_binary()
    profile_id = args.profile
    profile = resolve_profile(profile_id)
    corpus_ids = profile.get("corpora", [])
    timeout_sec = int(profile.get("timeout_seconds", 600))

    if not corpus_ids:
        raise CommandError(f"Profile '{profile_id}' has no corpora listed.")

    timestamp = time.strftime("%Y-%m-%d_%H-%M")
    out_dir = _WORK_DIR / "benchmarks" / f"benchmark-archive_{timestamp}"
    out_dir.mkdir(parents=True, exist_ok=True)

    modes_to_run = [
        "per_file", "cpar", "cpar_solid", "concat_compress",
        "tar_zstd", "tar_gzip",
    ]

    print(f"CPAC Archive Benchmark (profile: {profile_id})")
    print(f"  Description: {profile.get('description', '')}")
    print(f"  Corpora:     {', '.join(corpus_ids)}")
    print(f"  Modes:       {', '.join(modes_to_run)}")
    print(f"  Output:      {out_dir}")
    print()

    summary_path = out_dir / "summary.txt"
    all_results = {}

    with summary_path.open("w", encoding="utf-8") as summary:
        summary.write(f"CPAC Archive Benchmark — {timestamp} (profile: {profile_id})\n")
        summary.write("=" * 70 + "\n\n")

        for corpus_id in corpus_ids:
            try:
                corpus_cfg = resolve_corpus_config(corpus_id)
            except CommandError:
                print(f"  WARNING: corpus '{corpus_id}' not found, skipping")
                continue

            data_dir = corpus_data_dir(corpus_cfg)
            if not data_dir.exists() or not any(data_dir.rglob("*")):
                print(f"  WARNING: corpus '{corpus_id}' not downloaded ({data_dir}), skipping")
                continue

            files = sorted(
                f for f in data_dir.rglob("*")
                if f.is_file() and f.suffix != ".cpac"
            )
            if not files:
                print(f"  WARNING: no files in {data_dir}, skipping")
                continue

            total_size = sum(f.stat().st_size for f in files)
            print(f"[{corpus_id}] {len(files)} files, {total_size / (1024 * 1024):.1f} MB")
            summary.write(f"\n=== {corpus_id} ({len(files)} files, "
                          f"{total_size / (1024 * 1024):.1f} MB) ===\n\n")

            results = {}

            # --- per_file ---
            print(f"  per_file...", end=" ", flush=True)
            r = _archive_mode_per_file(binary, files, timeout_sec)
            results["per_file"] = r
            print(f"{_fmt_ratio(r['orig'], r['comp'])} ({_fmt_savings(r['orig'], r['comp'])}) "
                  f"in {r['time']:.1f}s")

            # --- cpar ---
            print(f"  cpar...", end=" ", flush=True)
            r = _archive_mode_cpar(binary, data_dir, timeout_sec)
            results["cpar"] = r
            print(f"{_fmt_ratio(r['orig'], r['comp'])} ({_fmt_savings(r['orig'], r['comp'])}) "
                  f"in {r['time']:.1f}s")

            # --- cpar_solid (real: archive-create --solid) ---
            print(f"  cpar_solid...", end=" ", flush=True)
            solid_path = data_dir.with_suffix(".solid.cpar")
            try:
                solid_t0 = time.time()
                solid_result = subprocess.run(
                    [binary, "archive-create", "--solid", str(data_dir),
                     "-o", str(solid_path)],
                    capture_output=True, text=True, timeout=timeout_sec,
                )
                solid_elapsed = time.time() - solid_t0
                if solid_result.returncode == 0 and solid_path.exists():
                    r = {"orig": total_size, "comp": solid_path.stat().st_size,
                         "time": solid_elapsed}
                else:
                    r = {"orig": total_size, "comp": total_size, "time": 0}
            except Exception:
                r = {"orig": total_size, "comp": total_size, "time": 0}
            finally:
                if solid_path.exists():
                    solid_path.unlink(missing_ok=True)
            results["cpar_solid"] = r
            print(f"{_fmt_ratio(r['orig'], r['comp'])} ({_fmt_savings(r['orig'], r['comp'])}) "
                  f"in {r['time']:.1f}s")

            # --- concat_compress (upper bound: concat all files + cpac compress) ---
            print(f"  concat_compress...", end=" ", flush=True)
            r = _archive_mode_concat_cpac(binary, files, timeout_sec)
            results["concat_compress"] = r
            print(f"{_fmt_ratio(r['orig'], r['comp'])} ({_fmt_savings(r['orig'], r['comp'])}) "
                  f"in {r['time']:.1f}s")

            # --- tar_zstd ---
            print(f"  tar_zstd...", end=" ", flush=True)
            r = _archive_mode_tar_zstd(files, data_dir, timeout_sec)
            results["tar_zstd"] = r
            if r.get("skip"):
                print("SKIPPED (no zstd available)")
            else:
                print(f"{_fmt_ratio(r['orig'], r['comp'])} ({_fmt_savings(r['orig'], r['comp'])}) "
                      f"in {r['time']:.1f}s")

            # --- tar_gzip ---
            print(f"  tar_gzip...", end=" ", flush=True)
            r = _archive_mode_tar_gzip(files, data_dir)
            results["tar_gzip"] = r
            print(f"{_fmt_ratio(r['orig'], r['comp'])} ({_fmt_savings(r['orig'], r['comp'])}) "
                  f"in {r['time']:.1f}s")

            # --- Write summary for this corpus ---
            hdr = f"{'Mode':<22} {'Compressed':>12} {'Ratio':>8} {'Savings':>8} {'Time':>8}"
            sep = "-" * len(hdr)
            summary.write(hdr + "\n")
            summary.write(sep + "\n")
            for mode in modes_to_run:
                mr = results.get(mode, {})
                if not mr or mr.get("skip"):
                    summary.write(f"{mode:<22} {'SKIPPED':>12}\n")
                    continue
                comp_str = f"{mr['comp']:,}"
                ratio_str = _fmt_ratio(mr['orig'], mr['comp'])
                savings_str = _fmt_savings(mr['orig'], mr['comp'])
                time_str = f"{mr['time']:.1f}s"
                summary.write(f"{mode:<22} {comp_str:>12} {ratio_str:>8} "
                              f"{savings_str:>8} {time_str:>8}\n")
            summary.write(f"\nOriginal total: {total_size:,} bytes "
                          f"({total_size / (1024 * 1024):.1f} MB)\n")

            # Cross-file gain: how much better is solid vs per-file?
            pf = results.get("per_file", {})
            sol = results.get("cpar_solid", {})
            if pf.get("comp") and sol.get("comp") and pf["comp"] > 0:
                xgain = (1 - sol["comp"] / pf["comp"]) * 100
                summary.write(f"Cross-file gain (solid vs per-file): {xgain:+.1f}%\n")
            summary.write("\n")

            all_results[corpus_id] = results
            print()

        # --- Grand summary ---
        summary.write("\n" + "=" * 70 + "\n")
        summary.write("GRAND SUMMARY\n")
        summary.write("=" * 70 + "\n\n")
        for corpus_id, results in all_results.items():
            pf = results.get("per_file", {})
            sol = results.get("cpar_solid", {})
            cpar = results.get("cpar", {})
            tzst = results.get("tar_zstd", {})
            summary.write(f"{corpus_id}:\n")
            if pf.get("orig"):
                summary.write(f"  Original:      {pf['orig']:>12,} bytes\n")
            if pf.get("comp"):
                summary.write(f"  CPAC per-file: {pf['comp']:>12,} bytes  "
                              f"({_fmt_ratio(pf['orig'], pf['comp'])})\n")
            if cpar.get("comp"):
                summary.write(f"  CPAR archive:  {cpar['comp']:>12,} bytes  "
                              f"({_fmt_ratio(cpar['orig'], cpar['comp'])})\n")
            if sol.get("comp"):
                summary.write(f"  CPAC solid:    {sol['comp']:>12,} bytes  "
                              f"({_fmt_ratio(sol['orig'], sol['comp'])})\n")
            if tzst.get("comp") and not tzst.get("skip"):
                summary.write(f"  tar+zstd-3:    {tzst['comp']:>12,} bytes  "
                              f"({_fmt_ratio(tzst['orig'], tzst['comp'])})\n")
            summary.write("\n")

    print(f"{'=' * 50}")
    print(f"Archive benchmark complete.")
    print(f"Output:  {out_dir}")
    print(f"Summary: {summary_path}")


def cmd_benchmark_external(args: argparse.Namespace) -> None:
    """External codec comparison benchmark.

    Benchmarks CPAC against zstd, brotli, lz4, gzip, xz, snappy (and
    optional custom codecs) on a corpus directory.  Cross-platform
    replacement for the former benchmark-external.ps1 script.

    xz uses Python stdlib lzma (always available).
    snappy uses python-snappy (pip install python-snappy) or CLI fallback.

    Results are written as CSV to .work/benchmarks/.
    """
    corpus_dir = pathlib.Path(args.corpus)
    if not corpus_dir.is_dir():
        raise CommandError(f"Corpus directory not found: {corpus_dir}")

    codecs = [c.strip() for c in args.codecs.split(",") if c.strip()]
    profile = args.profile
    iterations = {"quick": 1, "default": 3, "full": 5}.get(profile, 3)

    # Lazy-import lzma (stdlib) for xz codec
    import lzma as _lzma
    # Try importing snappy (python-snappy); None if unavailable
    try:
        import snappy as _snappy  # type: ignore[import-untyped]
    except ImportError:
        _snappy = None

    timestamp = time.strftime("%Y-%m-%d_%H-%M")
    out_dir = _WORK_DIR / "benchmarks"
    out_dir.mkdir(parents=True, exist_ok=True)
    output_csv = pathlib.Path(args.output) if args.output else (
        out_dir / f"benchmark-external_{timestamp}.csv"
    )

    # Collect corpus files
    files = sorted(
        f for f in corpus_dir.rglob("*")
        if f.is_file() and f.stat().st_size > 0
    )
    if not files:
        raise CommandError(f"No files found in corpus: {corpus_dir}")

    print("=== CPAC External Benchmark Framework ===")
    print(f"  Corpus:     {corpus_dir} ({len(files)} files)")
    print(f"  Codecs:     {', '.join(codecs)}")
    print(f"  Profile:    {profile} ({iterations} iterations)")
    print(f"  Output:     {output_csv}")
    print()

    # Verify codec availability
    codec_cmds = {}
    for codec in codecs:
        if codec == "cpac":
            try:
                codec_cmds[codec] = resolve_cpac_binary()
            except SystemExit:
                print(f"  WARNING: cpac binary not found, skipping")
        elif codec == "xz":
            # xz uses Python stdlib lzma — always available
            codec_cmds[codec] = "__builtin_lzma__"
        elif codec == "snappy":
            if _snappy is not None:
                codec_cmds[codec] = "__builtin_snappy__"
            else:
                path = shutil.which("snappy")
                if path:
                    codec_cmds[codec] = path
                else:
                    print(f"  WARNING: 'snappy' not found (pip install python-snappy), skipping")
        else:
            path = shutil.which(codec)
            if path:
                codec_cmds[codec] = path
            else:
                print(f"  WARNING: '{codec}' not found on PATH, skipping")
    codecs = [c for c in codecs if c in codec_cmds]

    # CSV header
    with output_csv.open("w", encoding="utf-8") as csvf:
        csvf.write("file,size_bytes,codec,compressed_bytes,ratio,"
                   "compress_ms,decompress_ms,throughput_mbs\n")

    for fpath in files:
        size = fpath.stat().st_size
        try:
            rel = str(fpath.relative_to(corpus_dir))
        except ValueError:
            rel = fpath.name

        for codec in codecs:
            comp_times = []
            dec_times = []
            comp_size = 0
            tmp_out = pathlib.Path(tempfile.mktemp(suffix=".compressed"))
            tmp_dec = pathlib.Path(tempfile.mktemp(suffix=".decompressed"))

            for _i in range(iterations):
                try:
                    # --- Compress ---
                    t0 = time.perf_counter()
                    if codec == "cpac":
                        subprocess.run(
                            [codec_cmds[codec], "compress", str(fpath),
                             "-o", str(tmp_out), "--force"],
                            capture_output=True, timeout=120,
                        )
                    elif codec == "zstd":
                        subprocess.run(
                            [codec_cmds[codec], "-f", "-q",
                             str(fpath), "-o", str(tmp_out)],
                            capture_output=True, timeout=120,
                        )
                    elif codec == "brotli":
                        subprocess.run(
                            [codec_cmds[codec], "-f",
                             str(fpath), "-o", str(tmp_out)],
                            capture_output=True, timeout=120,
                        )
                    elif codec == "lz4":
                        subprocess.run(
                            [codec_cmds[codec], "-f", "-q",
                             str(fpath), str(tmp_out)],
                            capture_output=True, timeout=120,
                        )
                    elif codec == "gzip":
                        with fpath.open("rb") as fin, tmp_out.open("wb") as fout:
                            import gzip as _gz
                            fout.write(_gz.compress(fin.read()))
                    elif codec == "xz":
                        raw = fpath.read_bytes()
                        tmp_out.write_bytes(
                            _lzma.compress(raw, format=_lzma.FORMAT_XZ,
                                           preset=6))
                    elif codec == "snappy":
                        raw = fpath.read_bytes()
                        if _snappy is not None:
                            tmp_out.write_bytes(_snappy.compress(raw))
                        else:
                            subprocess.run(
                                [codec_cmds[codec], "-c", str(fpath)],
                                capture_output=True, timeout=120,
                                stdout=tmp_out.open("wb"),
                            )
                    else:
                        # Generic: <codec> compress <input> -o <output>
                        subprocess.run(
                            [codec_cmds[codec], "compress",
                             str(fpath), "-o", str(tmp_out)],
                            capture_output=True, timeout=120,
                        )
                    comp_ms = (time.perf_counter() - t0) * 1000
                    comp_times.append(comp_ms)
                    if tmp_out.exists():
                        comp_size = tmp_out.stat().st_size

                    # --- Decompress ---
                    t0 = time.perf_counter()
                    if codec == "cpac":
                        subprocess.run(
                            [codec_cmds[codec], "decompress", str(tmp_out),
                             "-o", str(tmp_dec), "--force"],
                            capture_output=True, timeout=120,
                        )
                    elif codec == "zstd":
                        subprocess.run(
                            [codec_cmds[codec], "-d", "-f", "-q",
                             str(tmp_out), "-o", str(tmp_dec)],
                            capture_output=True, timeout=120,
                        )
                    elif codec == "brotli":
                        subprocess.run(
                            [codec_cmds[codec], "-d", "-f",
                             str(tmp_out), "-o", str(tmp_dec)],
                            capture_output=True, timeout=120,
                        )
                    elif codec == "lz4":
                        subprocess.run(
                            [codec_cmds[codec], "-d", "-f", "-q",
                             str(tmp_out), str(tmp_dec)],
                            capture_output=True, timeout=120,
                        )
                    elif codec == "gzip":
                        import gzip as _gz
                        with tmp_out.open("rb") as fin, tmp_dec.open("wb") as fout:
                            fout.write(_gz.decompress(fin.read()))
                    elif codec == "xz":
                        tmp_dec.write_bytes(
                            _lzma.decompress(tmp_out.read_bytes()))
                    elif codec == "snappy":
                        compressed = tmp_out.read_bytes()
                        if _snappy is not None:
                            tmp_dec.write_bytes(
                                _snappy.decompress(compressed))
                        else:
                            subprocess.run(
                                [codec_cmds[codec], "-d", "-c",
                                 str(tmp_out)],
                                capture_output=True, timeout=120,
                                stdout=tmp_dec.open("wb"),
                            )
                    else:
                        subprocess.run(
                            [codec_cmds[codec], "decompress",
                             str(tmp_out), "-o", str(tmp_dec)],
                            capture_output=True, timeout=120,
                        )
                    dec_ms = (time.perf_counter() - t0) * 1000
                    dec_times.append(dec_ms)

                except Exception:
                    comp_times.append(-1)
                    dec_times.append(-1)

            # Compute averages
            valid_comp = [t for t in comp_times if t >= 0]
            valid_dec = [t for t in dec_times if t >= 0]
            avg_comp = sum(valid_comp) / len(valid_comp) if valid_comp else -1
            avg_dec = sum(valid_dec) / len(valid_dec) if valid_dec else -1
            ratio = round(size / comp_size, 3) if comp_size > 0 and size > 0 else 0
            throughput = round(size / (1024 * 1024) / (avg_comp / 1000), 2) if avg_comp > 0 else 0

            with output_csv.open("a", encoding="utf-8") as csvf:
                csvf.write(
                    f"{rel},{size},{codec},{comp_size},{ratio},"
                    f"{round(avg_comp, 1)},{round(avg_dec, 1)},{throughput}\n"
                )

            # Cleanup
            tmp_out.unlink(missing_ok=True)
            tmp_dec.unlink(missing_ok=True)

        print(f"  {rel} ({size} B) done")

    print()
    print(f"Results written to {output_csv}")


def cmd_criterion(args: argparse.Namespace) -> None:
    """Run Criterion micro-benchmarks."""
    cargo = resolve_cargo()
    cmd = [cargo, "bench"]
    if args.package:
        cmd.extend(["-p", args.package])
    else:
        cmd.append("--workspace")
    run(cmd, env=cargo_env())


# ---------------------------------------------------------------------------
# Commands: PGO Build
# ---------------------------------------------------------------------------


def cmd_pgo_build(args: argparse.Namespace) -> None:
    """Profile-Guided Optimization build.

    Replaces pgo-build.ps1 / pgo-build.sh.
    """
    cargo = resolve_cargo()
    env = cargo_env()
    profile_dir = REPO_ROOT / "target" / "pgo-profiles"
    merged_profile = REPO_ROOT / "target" / "pgo-merged.profdata"

    print("=== CPAC PGO Build ===")

    # Step 1: Clean previous profiles
    if profile_dir.exists():
        shutil.rmtree(profile_dir)
    profile_dir.mkdir(parents=True, exist_ok=True)

    # Step 2: Build instrumented binary
    print("\n[1/4] Building instrumented binary...")
    pgo_env = env.copy()
    pgo_env["RUSTFLAGS"] = f"-Cprofile-generate={profile_dir}"
    run([cargo, "build", "--release", "-p", "cpac-cli"], env=pgo_env)

    # Step 3: Run profiling workloads
    print("\n[2/4] Running profiling workloads...")
    ext = ".exe" if platform.system() == "Windows" else ""
    binary = REPO_ROOT / "target" / "release" / f"cpac{ext}"

    corpus_dir = REPO_ROOT / "target" / "pgo-corpus"
    corpus_dir.mkdir(parents=True, exist_ok=True)

    # Generate test corpus
    text_file = corpus_dir / "text.txt"
    text_file.write_text("The quick brown fox jumps over the lazy dog. " * 10000,
                         encoding="utf-8")

    csv_file = corpus_dir / "data.csv"
    lines = ["id,name,value,status"]
    for i in range(5000):
        status = "ok" if i % 2 == 0 else "err"
        lines.append(f"{i},item_{i},{i * 7 % 1000},{status}")
    csv_file.write_text("\n".join(lines), encoding="utf-8")

    bin_file = corpus_dir / "binary.bin"
    bin_file.write_bytes(bytes(range(256)) * 100)

    for f in [text_file, csv_file, bin_file]:
        for backend in ["zstd", "brotli", "raw"]:
            out = f.with_suffix(".cpac")
            try:
                subprocess.run(
                    [str(binary), "compress", str(f), "-o", str(out),
                     "--backend", backend, "--force"],
                    capture_output=True, timeout=30,
                )
                if out.exists():
                    dec = f.with_suffix(".dec")
                    subprocess.run(
                        [str(binary), "decompress", str(out), "-o", str(dec), "--force"],
                        capture_output=True, timeout=30,
                    )
                    dec.unlink(missing_ok=True)
                    out.unlink(missing_ok=True)
            except Exception:
                pass

    # Step 4: Merge profiles
    print("\n[3/4] Merging profile data...")
    llvm_profdata = shutil.which("llvm-profdata")
    if llvm_profdata is None:
        # Try Rust sysroot
        try:
            result = subprocess.run(
                [cargo, "rustc", "--", "--print", "sysroot"],
                capture_output=True, text=True, env=env,
            )
            sysroot = pathlib.Path(result.stdout.strip())
            # Try to find llvm-profdata under sysroot
            for candidate in sysroot.rglob("llvm-profdata*"):
                if candidate.is_file():
                    llvm_profdata = str(candidate)
                    break
        except Exception:
            pass

    if llvm_profdata is None:
        print("[cpac] WARNING: llvm-profdata not found. Falling back to standard release build.")
        clean_env = env.copy()
        clean_env.pop("RUSTFLAGS", None)
        run([cargo, "build", "--release", "-p", "cpac-cli"], env=clean_env)
        return

    run([llvm_profdata, "merge", "-o", str(merged_profile), str(profile_dir)])

    # Step 5: PGO-optimized build
    print("\n[4/4] Building PGO-optimized binary...")
    pgo_use_env = env.copy()
    pgo_use_env["RUSTFLAGS"] = f"-Cprofile-use={merged_profile}"
    run([cargo, "build", "--release", "-p", "cpac-cli"], env=pgo_use_env)

    print(f"\n=== PGO build complete ===")
    print(f"Binary: {binary}")


# ---------------------------------------------------------------------------
# Commands: Corpus Download
# ---------------------------------------------------------------------------


def _download_single_corpus(corpus_id: str, target_dir: pathlib.Path) -> None:
    """Download a single corpus by id into target_dir."""
    try:
        cfg = resolve_corpus_config(corpus_id)
    except CommandError:
        print(f"  WARNING: Config not found for '{corpus_id}' — skipping")
        return

    print(f"[{corpus_id}]")
    subdir = cfg.get("target_subdir", corpus_id)
    dest = target_dir / subdir

    # Skip if already populated
    if dest.exists():
        existing = list(dest.rglob("*"))
        file_count = sum(1 for f in existing if f.is_file())
        if file_count > 0:
            total_mb = sum(f.stat().st_size for f in existing if f.is_file()) / (1024 * 1024)
            print(f"  Already present: {dest} ({file_count} files, {total_mb:.1f} MB)\n")
            return

    dest.mkdir(parents=True, exist_ok=True)
    urls = cfg.get("download_url", [])
    if isinstance(urls, str):
        urls = [urls]
    kind = cfg.get("download_kind", "")

    try:
        if kind == "http_zip_multi":
            import zipfile as _zf
            for i, url in enumerate(urls, 1):
                filename = url.split("?")[0].split("/")[-1]
                print(f"  [{i}/{len(urls)}] {filename}")
                tmp = pathlib.Path(tempfile.mktemp(suffix=".zip"))
                _download_file(url, tmp)
                print(f"    Extracting...")
                try:
                    with _zf.ZipFile(tmp) as zf:
                        zf.extractall(dest)
                finally:
                    tmp.unlink(missing_ok=True)
        elif len(urls) > 1 or kind == "http_file_multi":
            for i, url in enumerate(urls, 1):
                filename = url.split("?")[0].split("/")[-1]
                out_path = dest / filename
                print(f"  [{i}/{len(urls)}] {filename}")
                _download_file(url, out_path)
        elif urls:
            url = urls[0]
            print(f"  Downloading: {url}")

            if kind == "http_targz" or url.endswith((".tar.gz", ".tgz")):
                tmp = pathlib.Path(tempfile.mktemp(suffix=".tar.gz"))
                _download_file(url, tmp)
                print("  Extracting TAR.GZ...")
                subprocess.run(["tar", "-xzf", str(tmp), "-C", str(dest)], check=True)
                tmp.unlink(missing_ok=True)
            elif kind == "http_zip" or url.endswith(".zip"):
                tmp = pathlib.Path(tempfile.mktemp(suffix=".zip"))
                _download_file(url, tmp)
                print("  Extracting ZIP...")
                import zipfile
                with zipfile.ZipFile(tmp) as zf:
                    zf.extractall(dest)
                tmp.unlink(missing_ok=True)
            else:
                filename = url.split("?")[0].split("/")[-1]
                _download_file(url, dest / filename)

        file_count = sum(1 for f in dest.rglob("*") if f.is_file())
        total_mb = sum(f.stat().st_size for f in dest.rglob("*") if f.is_file()) / (1024 * 1024)
        print(f"  Done: {file_count} files, {total_mb:.1f} MB\n")

    except Exception as e:
        print(f"  Download failed for '{corpus_id}': {e}")
        if dest.exists() and not any(dest.iterdir()):
            shutil.rmtree(dest, ignore_errors=True)
        print()


def cmd_download_corpus(args: argparse.Namespace) -> None:
    """Download benchmark corpora.

    Supports two modes:
      --corpus <id,...>    Download specific corpora by id
      --profile <id>       Download all corpora required by a benchmark profile
    """
    # Resolve corpus list from profile or explicit --corpus flag
    if args.profile:
        corpus_list = corpora_for_profile(args.profile)
        print(f"CPAC Corpus Downloader (profile: {args.profile})")
    else:
        corpus_list = [c.strip() for c in args.corpus.split(",") if c.strip()]
        print("CPAC Corpus Downloader")

    target_dir = REPO_ROOT / args.target_dir
    print("======================")
    print(f"Corpora:  {', '.join(corpus_list)}")
    print(f"Target:   {target_dir}\n")

    target_dir.mkdir(parents=True, exist_ok=True)

    for corpus_id in corpus_list:
        _download_single_corpus(corpus_id, target_dir)

    # Summary
    print("Summary:")
    if target_dir.exists():
        for d in sorted(target_dir.iterdir()):
            if d.is_dir():
                files = list(d.rglob("*"))
                fc = sum(1 for f in files if f.is_file())
                mb = sum(f.stat().st_size for f in files if f.is_file()) / (1024 * 1024)
                print(f"  {d.name:<22} {mb:>8.1f} MB   {fc} files")


def _download_file(url: str, dest: pathlib.Path) -> None:
    """Download a URL to a local file using urllib (no external deps).

    Tries with default SSL verification first.  If the server has a
    self-signed or expired certificate (common on academic corpus hosts)
    the download is retried with verification disabled and a warning.
    """
    import urllib.request
    import urllib.error
    import ssl

    dest.parent.mkdir(parents=True, exist_ok=True)
    req = urllib.request.Request(url, headers={"User-Agent": "CPAC-Corpus-Downloader/1.0"})

    # Attempt 1: normal TLS verification
    try:
        ctx = ssl.create_default_context()
        with urllib.request.urlopen(req, timeout=600, context=ctx) as resp:
            with dest.open("wb") as f:
                shutil.copyfileobj(resp, f)
        return
    except (ssl.SSLCertVerificationError, urllib.error.URLError) as e:
        # URLError wraps SSL errors as e.reason; only retry on cert issues
        reason = str(getattr(e, "reason", e))
        if "CERTIFICATE_VERIFY_FAILED" not in reason and "SSL" not in reason:
            raise
        # fall through to retry

    # Attempt 2: skip verification (academic servers with self-signed certs)
    print(f"    WARNING: TLS certificate verification failed for {url}")
    print(f"    Retrying without verification (academic corpus host)...")
    ctx_unverified = ssl.create_default_context()
    ctx_unverified.check_hostname = False
    ctx_unverified.verify_mode = ssl.CERT_NONE
    req = urllib.request.Request(url, headers={"User-Agent": "CPAC-Corpus-Downloader/1.0"})
    with urllib.request.urlopen(req, timeout=600, context=ctx_unverified) as resp:
        with dest.open("wb") as f:
            shutil.copyfileobj(resp, f)


# ---------------------------------------------------------------------------
# Commands: Rust setup
# ---------------------------------------------------------------------------


def cmd_setup(args: argparse.Namespace) -> None:
    """Install Rust toolchain and verify workspace builds.

    Replaces setup.ps1 / setup.sh.
    """
    print("=== CPAC Rust Engine Setup ===")

    # Check for rustup
    rustup = shutil.which("rustup")
    if rustup is None:
        print("Rust not found. Install from: https://rustup.rs/")
        sys.exit(1)

    print(f"rustup found: {rustup}")
    run(["rustc", "--version"])
    run(["cargo", "--version"])

    # Ensure components
    subprocess.run(["rustup", "component", "add", "rustfmt", "clippy"],
                    capture_output=True)

    # Build workspace
    print("\nBuilding workspace...")
    cargo = resolve_cargo()
    run([cargo, "build", "--workspace"], env=cargo_env())

    print("\nSetup complete!")


# ---------------------------------------------------------------------------
# Commands: Info / Analyze
# ---------------------------------------------------------------------------


def cmd_info(args: argparse.Namespace) -> None:
    """Show file info or host system details (wraps cpac CLI)."""
    binary = resolve_cpac_binary()
    cmd = [binary, "info"]
    if args.host:
        cmd.append("--host")
    if args.input:
        cmd.append(str(args.input))
    run(cmd)


def cmd_analyze(args: argparse.Namespace) -> None:
    """Analyze file structure (wraps cpac CLI)."""
    binary = resolve_cpac_binary()
    run([binary, "analyze", str(args.input)])


def cmd_auto_analyze(args: argparse.Namespace) -> None:
    """Auto-analyze a directory for optimal compression settings (wraps cpac CLI)."""
    binary = resolve_cpac_binary()
    cmd = [binary, "auto-analyze", str(args.input)]
    if args.quick:
        cmd.append("--quick")
    if args.output:
        cmd.extend(["--output", str(args.output)])
    if args.write_config:
        cmd.append("--write-config")
    run(cmd)


def cmd_profile_corpus(args: argparse.Namespace) -> None:
    """Profile all files in a corpus/directory with cpac profile.

    Runs `cpac profile` on each file and collects results into
    .work/profiles/ for consumption by benchmark and dev loops.
    """
    binary = resolve_cpac_binary()
    profile_id = args.profile
    profile = resolve_profile(profile_id)
    corpus_ids = profile.get("corpora", [])
    timeout_sec = int(profile.get("timeout_seconds", 300))
    quick = args.quick

    if not corpus_ids:
        raise CommandError(f"Profile '{profile_id}' has no corpora listed.")

    timestamp = time.strftime("%Y-%m-%d_%H-%M")
    out_dir = _WORK_DIR / "profiles" / f"profile-{profile_id}_{timestamp}"
    out_dir.mkdir(parents=True, exist_ok=True)

    print(f"CPAC Corpus Profiling (profile: {profile_id})")
    print(f"  Corpora: {', '.join(corpus_ids)}")
    print(f"  Mode:    {'quick' if quick else 'full'}")
    print(f"  Output:  {out_dir}")
    print()

    summary_path = out_dir / "summary.txt"
    total_files = 0
    total_ok = 0

    with summary_path.open("w", encoding="utf-8") as summary:
        summary.write(f"CPAC Corpus Profile — {timestamp} (profile: {profile_id})\n")
        summary.write("=" * 70 + "\n\n")

        for corpus_id in corpus_ids:
            try:
                corpus_cfg = resolve_corpus_config(corpus_id)
            except CommandError:
                print(f"  WARNING: corpus '{corpus_id}' not found, skipping")
                continue

            data_dir = corpus_data_dir(corpus_cfg)
            if not data_dir.exists():
                print(f"  WARNING: corpus '{corpus_id}' not downloaded, skipping")
                continue

            files = sorted(
                f for f in data_dir.rglob("*")
                if f.is_file() and f.suffix != ".cpac"
            )
            if not files:
                continue

            print(f"[{corpus_id}] {len(files)} files")
            summary.write(f"\n=== {corpus_id} ({len(files)} files) ===\n\n")

            corpus_out = out_dir / corpus_id
            corpus_out.mkdir(parents=True, exist_ok=True)

            for f in files:
                total_files += 1
                rel = f.relative_to(data_dir)
                print(f"  {rel}...", end=" ", flush=True)

                cmd = [binary, "profile", str(f)]
                if quick:
                    cmd.append("--quick")

                log_name = str(rel).replace(os.sep, "_").replace("/", "_")
                log_file = corpus_out / f"{log_name}.txt"
                try:
                    result = subprocess.run(
                        cmd, cwd=str(REPO_ROOT),
                        capture_output=True, text=True, timeout=timeout_sec,
                    )
                    log_file.write_text(result.stdout, encoding="utf-8")
                    summary.write(f"--- {corpus_id}/{rel} ---\n")
                    summary.write(result.stdout + "\n")
                    print("OK")
                    total_ok += 1
                except subprocess.TimeoutExpired:
                    print("TIMEOUT")
                    summary.write(f"TIMEOUT: {corpus_id}/{rel}\n\n")
                except Exception as e:
                    print(f"FAILED: {e}")
                    summary.write(f"ERROR: {corpus_id}/{rel}: {e}\n\n")

    print(f"\n{'=' * 50}")
    print(f"Profiled: {total_ok}/{total_files} files")
    print(f"Output:   {out_dir}")
    print(f"Summary:  {summary_path}")


def _byte_frequency(data: bytes) -> list:
    """Return normalized 256-bin byte frequency histogram."""
    hist = [0] * 256
    for b in data:
        hist[b] += 1
    total = len(data) or 1
    return [h / total for h in hist]


def _jensen_shannon(p: list, q: list) -> float:
    """Jensen-Shannon divergence between two distributions."""
    import math
    m = [(a + b) / 2 for a, b in zip(p, q)]
    def kl(a, b):
        return sum(ai * math.log2(ai / bi) for ai, bi in zip(a, b) if ai > 0 and bi > 0)
    return (kl(p, m) + kl(q, m)) / 2


def _extract_ngrams(data: bytes, n: int, top_k: int = 50) -> list:
    """Extract top-k most frequent n-grams from data."""
    from collections import Counter
    if len(data) < n:
        return []
    counts = Counter(data[i:i+n] for i in range(len(data) - n + 1))
    return counts.most_common(top_k)


def cmd_analyze_multi(args: argparse.Namespace) -> None:
    """Cross-file pattern analysis for archive optimization.

    Analyzes multiple files together to identify shared patterns,
    byte-frequency similarity, common n-grams, and estimated
    cross-file/dictionary compression gains.
    """
    binary = resolve_cpac_binary()
    input_dir = pathlib.Path(args.input_dir)

    # Resolve files from corpus id or directory
    if not input_dir.exists():
        # Try as corpus id
        try:
            corpus_cfg = resolve_corpus_config(str(input_dir))
            input_dir = corpus_data_dir(corpus_cfg)
        except CommandError:
            raise CommandError(f"Not a directory or corpus id: {args.input_dir}")

    if not input_dir.is_dir():
        raise CommandError(f"Not a directory: {input_dir}")

    files = sorted(
        f for f in input_dir.rglob("*")
        if f.is_file() and f.suffix != ".cpac"
    )
    if not files:
        raise CommandError(f"No files found in {input_dir}")

    max_files = args.max_files or 200
    if len(files) > max_files:
        print(f"  Limiting analysis to first {max_files} of {len(files)} files")
        files = files[:max_files]

    # Sample limit per file for n-gram analysis (avoid OOM on huge files)
    sample_limit = 1024 * 1024  # 1 MB per file for n-gram work

    print(f"=== CPAC Cross-File Analysis ===")
    print(f"  Directory: {input_dir}")
    print(f"  Files:     {len(files)}")
    total_size = sum(f.stat().st_size for f in files)
    print(f"  Total:     {total_size / (1024 * 1024):.1f} MB")
    print()

    # --- Step 1: Per-file SSR analysis ---
    print("[1/5] Per-file structure analysis...")
    file_profiles = []
    for f in files:
        result = subprocess.run(
            [binary, "analyze", str(f)],
            capture_output=True, text=True, timeout=60,
        )
        # Parse key fields from output
        profile = {"file": f, "size": f.stat().st_size, "raw": result.stdout}
        for line in result.stdout.splitlines():
            if "Entropy:" in line:
                try:
                    profile["entropy"] = float(line.split(":")[1].strip().split()[0])
                except (ValueError, IndexError):
                    pass
            elif "ASCII ratio:" in line:
                try:
                    profile["ascii_ratio"] = float(line.split(":")[1].strip().rstrip("%")) / 100
                except (ValueError, IndexError):
                    pass
            elif "Track:" in line:
                profile["track"] = line.split(":")[1].strip()
            elif "Domain:" in line:
                profile["domain"] = line.split(":")[1].strip()
        file_profiles.append(profile)
    print(f"  Analyzed {len(file_profiles)} files")

    # --- Step 2: Byte frequency similarity ---
    print("[2/5] Byte frequency similarity...")
    histograms = []
    file_data_samples = []
    for fp in file_profiles:
        data = fp["file"].read_bytes()[:sample_limit]
        histograms.append(_byte_frequency(data))
        file_data_samples.append(data)

    # Compute pairwise JS divergence (sample if too many files)
    n = len(histograms)
    js_sum = 0.0
    js_count = 0
    js_min = float("inf")
    js_max = 0.0
    max_pairs = 5000
    if n * (n - 1) // 2 > max_pairs:
        import random
        pairs = random.sample([(i, j) for i in range(n) for j in range(i+1, n)], max_pairs)
    else:
        pairs = [(i, j) for i in range(n) for j in range(i+1, n)]
    for i, j in pairs:
        jsd = _jensen_shannon(histograms[i], histograms[j])
        js_sum += jsd
        js_count += 1
        js_min = min(js_min, jsd)
        js_max = max(js_max, jsd)
    js_avg = js_sum / max(js_count, 1)
    similarity = max(0, 1 - js_avg)  # higher = more similar
    print(f"  Avg JS divergence: {js_avg:.4f} (0=identical, 1=unrelated)")
    print(f"  Range: {js_min:.4f} - {js_max:.4f}")
    print(f"  Byte-frequency similarity: {similarity * 100:.1f}%")

    # --- Step 3: Common n-gram analysis ---
    print("[3/5] Common n-gram analysis...")
    from collections import Counter
    # Aggregate 4-grams across all files
    global_4grams = Counter()
    for data in file_data_samples:
        if len(data) >= 4:
            for i in range(min(len(data) - 3, sample_limit)):
                global_4grams[data[i:i+4]] += 1
    # Find n-grams that appear in multiple files
    per_file_4grams = []
    for data in file_data_samples:
        file_set = set()
        for i in range(min(len(data) - 3, sample_limit)):
            file_set.add(data[i:i+4])
        per_file_4grams.append(file_set)
    # Count how many files each n-gram appears in
    shared_ngrams = Counter()
    all_unique_ngrams = set()
    for fs in per_file_4grams:
        all_unique_ngrams.update(fs)
        for ng in fs:
            shared_ngrams[ng] += 1
    # N-grams in 50%+ of files
    threshold = max(2, len(files) // 2)
    common = [(ng, cnt) for ng, cnt in shared_ngrams.items() if cnt >= threshold]
    common.sort(key=lambda x: -x[1])
    print(f"  Unique 4-grams across corpus: {len(all_unique_ngrams):,}")
    print(f"  4-grams in {threshold}+ files: {len(common):,}")
    if common:
        top_5 = common[:5]
        for ng, cnt in top_5:
            display = ng.decode("utf-8", errors="replace")
            if not display.isprintable():
                display = ng.hex()
            print(f"    {repr(display):>12}: in {cnt}/{len(files)} files")

    # --- Step 4: Domain clustering ---
    print("[4/5] Domain & track clustering...")
    from collections import Counter as _Counter
    tracks = _Counter(fp.get("track", "unknown") for fp in file_profiles)
    domains = _Counter(fp.get("domain", "none") for fp in file_profiles)
    print(f"  Tracks:  {dict(tracks)}")
    print(f"  Domains: {dict(domains)}")
    # Entropy distribution
    entropies = [fp["entropy"] for fp in file_profiles if "entropy" in fp]
    if entropies:
        import statistics
        print(f"  Entropy: min={min(entropies):.2f}, max={max(entropies):.2f}, "
              f"mean={statistics.mean(entropies):.2f}, stdev={statistics.stdev(entropies) if len(entropies) > 1 else 0:.2f}")

    # --- Step 5: Dictionary gain estimation ---
    print("[5/5] Dictionary gain estimation...")
    dict_gain_str = "N/A (install: pip install zstandard)"
    try:
        import zstandard
        # Train dictionary from file samples
        train_data = [d for d in file_data_samples if len(d) >= 64]
        if len(train_data) >= 3:
            dict_obj = zstandard.train_dictionary(128 * 1024, train_data)
            # Compress each file with and without dict
            cctx_nodict = zstandard.ZstdCompressor(level=3)
            cctx_dict = zstandard.ZstdCompressor(level=3, dict_data=dict_obj)
            total_nodict = 0
            total_dict = 0
            for d in train_data:
                total_nodict += len(cctx_nodict.compress(d))
                total_dict += len(cctx_dict.compress(d))
            if total_nodict > 0:
                dict_benefit = (1 - total_dict / total_nodict) * 100
                dict_gain_str = f"{dict_benefit:+.1f}% (zstd-3 with 128KB dict vs without)"
            else:
                dict_gain_str = "0.0%"
        else:
            dict_gain_str = "N/A (need 3+ files with 64+ bytes)"
    except ImportError:
        pass
    print(f"  Dictionary gain: {dict_gain_str}")

    # --- Summary ---
    print(f"\n{'=' * 60}")
    print(f"CROSS-FILE ANALYSIS SUMMARY")
    print(f"{'=' * 60}")
    print(f"  Files:               {len(files)}")
    print(f"  Total size:          {total_size / (1024 * 1024):.1f} MB")
    print(f"  Byte similarity:     {similarity * 100:.1f}%")
    print(f"  Common 4-grams:      {len(common):,} (in {threshold}+ files)")
    print(f"  Dictionary gain:     {dict_gain_str}")
    homogeneous = similarity > 0.8 and len(tracks) <= 2
    print(f"  Homogeneity:         {'HIGH' if homogeneous else 'MIXED'}")
    print()
    if homogeneous:
        print("  Recommendation: Use SOLID archive mode for best cross-file compression.")
        print("  Expected benefit: significant ratio improvement over per-file compression.")
    elif similarity > 0.5:
        print("  Recommendation: Dictionary-aided compression may provide moderate gains.")
        print("  Consider: train-dict on this corpus, then compress with --dict.")
    else:
        print("  Recommendation: Per-file compression is likely optimal for this mix.")
        print("  Solid mode may hurt ratio on heterogeneous content.")

    # Write report to file if --output specified
    if args.output:
        report_path = pathlib.Path(args.output)
        report_path.parent.mkdir(parents=True, exist_ok=True)
        with report_path.open("w", encoding="utf-8") as rpt:
            rpt.write(f"CPAC Cross-File Analysis — {time.strftime('%Y-%m-%d %H:%M')}\n")
            rpt.write(f"Directory: {input_dir}\n")
            rpt.write(f"Files: {len(files)}, Total: {total_size / (1024 * 1024):.1f} MB\n")
            rpt.write(f"\nByte similarity: {similarity * 100:.1f}%\n")
            rpt.write(f"Common 4-grams (in {threshold}+ files): {len(common):,}\n")
            rpt.write(f"Dictionary gain: {dict_gain_str}\n")
            rpt.write(f"Homogeneity: {'HIGH' if homogeneous else 'MIXED'}\n")
            rpt.write(f"\nPer-file profiles:\n")
            for fp in file_profiles:
                rel = fp["file"].relative_to(input_dir)
                ent = fp.get('entropy', '?')
                trk = fp.get('track', '?')
                dom = fp.get('domain', 'none')
                rpt.write(f"  {rel}: {fp['size']:,}B entropy={ent} track={trk} domain={dom}\n")
        print(f"  Report saved to: {report_path}")


# ---------------------------------------------------------------------------
# Commands: Calibrate / Train-Dict
# ---------------------------------------------------------------------------


def cmd_calibrate(args: argparse.Namespace) -> None:
    """Run calibration: compute transform win-rates from benchmark CSVs.

    Wraps `cpac lab calibrate`.  If no --dir is given, defaults to
    .work/benchmarks/transform-study/.
    """
    binary = resolve_cpac_binary()
    cmd = [binary, "lab", "calibrate"]
    if args.dir:
        cmd.extend(["--dir", str(args.dir)])
    if args.output:
        cmd.extend(["--output", str(args.output)])
    if args.stdout:
        cmd.append("--stdout")
    run(cmd)


def cmd_train_dict(args: argparse.Namespace) -> None:
    """Train a CPAC compression dictionary from a corpus directory.

    Reads all files in the given directory, trains a zstd dictionary,
    and writes it as a .cpac-dict file.
    """
    corpus_dir = pathlib.Path(args.corpus_dir)
    if not corpus_dir.is_dir():
        raise CommandError(f"Corpus directory not found: {corpus_dir}")

    out_path = pathlib.Path(args.output) if args.output else (
        _WORK_DIR / "benchmarks" / f"{corpus_dir.name}.cpac-dict"
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)
    max_size = args.max_size

    # Collect corpus files
    samples = []
    for f in sorted(corpus_dir.iterdir()):
        if f.is_file() and not f.name.endswith(".cpac"):
            samples.append(f)
    if not samples:
        raise CommandError(f"No files found in {corpus_dir}")

    print(f"Training dictionary from {len(samples)} files in {corpus_dir}")
    print(f"  Max dict size: {max_size // 1024} KB")

    # Use cpac CLI compress with --dict would require dict to exist.
    # Instead, invoke a small Rust helper via the binary.
    # For now, use Python zstd bindings if available, else delegate to CLI.
    try:
        import zstandard
        file_data = []
        for f in samples:
            data = f.read_bytes()
            if data:
                file_data.append(data)
        if not file_data:
            raise CommandError("All corpus files are empty")

        print(f"  Total corpus: {sum(len(d) for d in file_data) / (1024 * 1024):.1f} MB")
        dict_data = zstandard.train_dictionary(max_size, file_data)
        raw_dict = dict_data.as_bytes()

        # Write as CPAC dictionary format (CPDI header + raw zstd dict)
        import struct
        dict_id = 0
        try:
            dict_id = int(hashlib.sha256(raw_dict).hexdigest()[:16], 16)
        except Exception:
            pass
        header = b"CPDI"  # magic
        header += struct.pack("<B", 1)  # version
        header += struct.pack("<Q", dict_id)  # dict_id
        header += struct.pack("<I", len(raw_dict))  # size
        header += struct.pack("<I", len(file_data))  # samples
        header += struct.pack("<Q", sum(len(d) for d in file_data))  # corpus_size
        header += struct.pack("<Q", int(time.time()))  # created_at
        out_path.write_bytes(header + raw_dict)

        print(f"  Dictionary: {len(raw_dict)} bytes ({len(raw_dict) / 1024:.1f} KB)")
        print(f"  Saved to:   {out_path}")
    except ImportError:
        print("  Python zstandard not available, falling back to zstd CLI...")
        # Fallback: use zstd CLI --train
        zstd_bin = shutil.which("zstd")
        if zstd_bin is None:
            raise CommandError(
                "Neither 'zstandard' Python package nor 'zstd' CLI found.\n"
                "  Install: pip install zstandard  OR  install zstd CLI"
            )
        sample_paths = [str(f) for f in samples]
        raw_dict_path = out_path.with_suffix(".raw")
        cmd = [zstd_bin, "--train"] + sample_paths + [
            "-o", str(raw_dict_path),
            "--maxdict", str(max_size),
        ]
        run(cmd)
        raw_dict = raw_dict_path.read_bytes()

        # Wrap in CPAC dictionary format
        import struct
        dict_id = int(hashlib.sha256(raw_dict).hexdigest()[:16], 16)
        header = b"CPDI"
        header += struct.pack("<B", 1)
        header += struct.pack("<Q", dict_id)
        header += struct.pack("<I", len(raw_dict))
        header += struct.pack("<I", len(samples))
        header += struct.pack("<Q", sum(f.stat().st_size for f in samples))
        header += struct.pack("<Q", int(time.time()))
        out_path.write_bytes(header + raw_dict)
        raw_dict_path.unlink(missing_ok=True)

        print(f"  Dictionary: {len(raw_dict)} bytes ({len(raw_dict) / 1024:.1f} KB)")
        print(f"  Saved to:   {out_path}")


# ---------------------------------------------------------------------------
# Commands: Compress / Decompress (convenience wrappers)
# ---------------------------------------------------------------------------


def cmd_compress(args: argparse.Namespace) -> None:
    """Compress a file (wraps cpac CLI)."""
    binary = resolve_cpac_binary()
    cmd = [binary, "compress", str(args.input)]
    if args.output:
        cmd.extend(["-o", str(args.output)])
    if args.backend:
        cmd.extend(["--backend", args.backend])
    if args.force:
        cmd.append("--force")
    if args.smart:
        cmd.append("--smart")
    if args.preset:
        cmd.extend(["--preset", args.preset])
    if getattr(args, "dict", None):
        cmd.extend(["--dict", str(args.dict)])
    if args.verbose:
        cmd.extend(["-" + "v" * args.verbose])
    run(cmd)


def cmd_decompress(args: argparse.Namespace) -> None:
    """Decompress a file (wraps cpac CLI)."""
    binary = resolve_cpac_binary()
    cmd = [binary, "decompress", str(args.input)]
    if args.output:
        cmd.extend(["-o", str(args.output)])
    if args.force:
        cmd.append("--force")
    run(cmd)


# ---------------------------------------------------------------------------
# Argparse Setup
# ---------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="cpac",
        description="CPAC — Unified Build System & CLI Wrapper",
    )
    sub = parser.add_subparsers(dest="command", help="Available commands")

    # update
    p = sub.add_parser("update", help="Update Cargo dependencies")
    p.add_argument("-p", "--package", help="Update a specific package")
    p.add_argument("--dry-run", action="store_true", help="Show what would be updated")

    # build
    p = sub.add_parser("build", help="Build workspace")
    p.add_argument("--release", action="store_true", help="Release build")
    p.add_argument("-p", "--package", help="Build a specific package")

    # test
    p = sub.add_parser("test", help="Run tests")
    p.add_argument("--release", action="store_true", help="Test in release mode")
    p.add_argument("-p", "--package", help="Test a specific package")
    p.add_argument("extra", nargs="*", help="Extra args passed after --")

    # clippy
    sub.add_parser("clippy", help="Run clippy lints")

    # fmt
    p = sub.add_parser("fmt", help="Run cargo fmt")
    p.add_argument("--check", action="store_true", help="Check only (no modifications)")

    # check (presubmit)
    sub.add_parser("check", help="Full presubmit: build + test + clippy + fmt")

    # bench
    p = sub.add_parser("bench", help="Benchmark a single file")
    p.add_argument("input", type=pathlib.Path, help="Input file to benchmark")
    p.add_argument("--quick", action="store_true", help="Quick mode (1 iteration)")
    p.add_argument("--full", action="store_true", help="Full mode (50 iterations)")
    p.add_argument("--track1", action="store_true", help="Include Track 1 results")
    p.add_argument("--discovery", action="store_true", help="Discovery mode")
    p.add_argument("--skip-baselines", action="store_true", help="Skip baseline engines")
    p.add_argument("--json", action="store_true", help="JSON output")
    p.add_argument("-n", "--iterations", type=int, help="Override iteration count")

    # benchmark-all
    p = sub.add_parser("benchmark-all", help="Run profile-driven corpus benchmark suite")
    p.add_argument("--profile", default="balanced",
                    help="Benchmark profile id (default: balanced). See benches/cpac/profiles/")
    p.add_argument("--skip-baselines", action="store_true", help="Skip baseline engines")

    # benchmark-archive
    p = sub.add_parser("benchmark-archive",
                       help="Archive benchmark: per-file vs CPAR vs solid vs tar baselines")
    p.add_argument("--profile", default="archive",
                    help="Benchmark profile id (default: archive). See benches/cpac/profiles/")

    # benchmark-external
    p = sub.add_parser("benchmark-external",
                       help="Compare CPAC against external codecs (zstd, brotli, lz4, gzip)")
    p.add_argument("--corpus", default=".work/benchdata",
                    help="Corpus directory to benchmark (default: .work/benchdata)")
    p.add_argument("--codecs", default="cpac,zstd,brotli,lz4,gzip,xz,snappy",
                    help="Comma-separated codecs (default: cpac,zstd,brotli,lz4,gzip,xz,snappy)")
    p.add_argument("--profile", default="default",
                    help="Benchmark profile: quick (1 iter), default (3), full (5)")
    p.add_argument("-o", "--output", type=pathlib.Path,
                    help="Output CSV path (default: .work/benchmarks/benchmark-external_<ts>.csv)")

    # criterion
    p = sub.add_parser("criterion", help="Run Criterion micro-benchmarks")
    p.add_argument("-p", "--package", help="Bench a specific package")

    # pgo-build
    sub.add_parser("pgo-build", help="Profile-Guided Optimization build")

    # download-corpus
    p = sub.add_parser("download-corpus", help="Download benchmark corpora")
    p.add_argument("--corpus", default="canterbury,calgary,silesia,loghub2_2k,enwik8",
                    help="Comma-separated corpus IDs (default: canterbury,calgary,silesia,loghub2_2k,enwik8)")
    p.add_argument("--profile",
                    help="Download all corpora for a benchmark profile (overrides --corpus)")
    p.add_argument("--target-dir", default=".work/benchdata",
                    help="Target directory (default: .work/benchdata)")

    # setup
    sub.add_parser("setup", help="Install Rust toolchain and verify build")

    # info
    p = sub.add_parser("info", help="Show file info or host details")
    p.add_argument("input", nargs="?", type=pathlib.Path, help="Input file")
    p.add_argument("--host", action="store_true", help="Show host system info")

    # analyze
    p = sub.add_parser("analyze", help="Analyze file structure")
    p.add_argument("input", type=pathlib.Path, help="Input file to analyze")

    # profile-corpus
    p = sub.add_parser("profile-corpus",
                       help="Profile all files in a corpus with trial compression matrix")
    p.add_argument("--profile", default="balanced",
                    help="Benchmark profile id (default: balanced). See benches/cpac/profiles/")
    p.add_argument("--quick", action="store_true",
                    help="Quick mode (fewer trials per file)")

    # analyze-multi
    p = sub.add_parser("analyze-multi",
                       help="Cross-file pattern analysis for archive optimization")
    p.add_argument("input_dir", help="Directory or corpus id to analyze")
    p.add_argument("-o", "--output", type=pathlib.Path,
                    help="Save report to file")
    p.add_argument("--max-files", type=int, default=200,
                    help="Max files to analyze (default: 200)")

    # calibrate
    p = sub.add_parser("calibrate", help="Compute transform win-rates from benchmark CSVs")
    p.add_argument("--dir", type=pathlib.Path,
                    help="Directory with benchmark CSV files (default: .work/benchmarks/transform-study/)")
    p.add_argument("-o", "--output", type=pathlib.Path,
                    help="Output calibration.json path")
    p.add_argument("--stdout", action="store_true",
                    help="Print JSON to stdout instead of writing a file")

    # train-dict
    p = sub.add_parser("train-dict", help="Train a compression dictionary from a corpus")
    p.add_argument("corpus_dir", type=pathlib.Path, help="Directory of corpus files")
    p.add_argument("-o", "--output", help="Output .cpac-dict file")
    p.add_argument("--max-size", type=int, default=128 * 1024,
                    help="Max dictionary size in bytes (default: 131072 = 128 KB)")

    # auto-analyze
    p = sub.add_parser("auto-analyze", help="Auto-analyze a directory for optimal compression settings")
    p.add_argument("input", type=pathlib.Path, help="Directory to analyze")
    p.add_argument("-o", "--output", type=pathlib.Path, help="Output Markdown report file")
    p.add_argument("--quick", action="store_true", help="Quick mode (fewer files, smaller cap)")
    p.add_argument("--write-config", action="store_true", help="Write .cpac-config.yml to directory")

    # compress
    p = sub.add_parser("compress", help="Compress a file")
    p.add_argument("input", type=pathlib.Path, help="Input file")
    p.add_argument("-o", "--output", type=pathlib.Path, help="Output file")
    p.add_argument("--backend", help="Entropy backend")
    p.add_argument("-f", "--force", action="store_true", help="Overwrite existing")
    p.add_argument("--smart", action="store_true", help="Smart transform selection")
    p.add_argument("--preset", help="Named preset")
    p.add_argument("--dict", type=pathlib.Path, help="Pre-trained dictionary (.cpac-dict)")
    p.add_argument("-v", "--verbose", action="count", default=0, help="Verbosity level")

    # decompress
    p = sub.add_parser("decompress", help="Decompress a file")
    p.add_argument("input", type=pathlib.Path, help="Input file")
    p.add_argument("-o", "--output", type=pathlib.Path, help="Output file")
    p.add_argument("-f", "--force", action="store_true", help="Overwrite existing")

    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        sys.exit(0)

    dispatch = {
        "update": cmd_update,
        "build": cmd_build,
        "test": cmd_test,
        "clippy": cmd_clippy,
        "fmt": cmd_fmt,
        "check": cmd_check,
        "bench": cmd_bench,
        "benchmark-all": cmd_benchmark_all,
        "benchmark-archive": cmd_benchmark_archive,
        "benchmark-external": cmd_benchmark_external,
        "criterion": cmd_criterion,
        "pgo-build": cmd_pgo_build,
        "download-corpus": cmd_download_corpus,
        "setup": cmd_setup,
        "info": cmd_info,
        "analyze": cmd_analyze,
        "analyze-multi": cmd_analyze_multi,
        "profile-corpus": cmd_profile_corpus,
        "calibrate": cmd_calibrate,
        "train-dict": cmd_train_dict,
        "auto-analyze": cmd_auto_analyze,
        "compress": cmd_compress,
        "decompress": cmd_decompress,
    }

    handler = dispatch.get(args.command)
    if handler is None:
        parser.print_help()
        sys.exit(1)

    try:
        handler(args)
    except CommandError as e:
        print(f"[cpac] ERROR: {e}", file=sys.stderr)
        sys.exit(1)
    except subprocess.CalledProcessError as e:
        sys.exit(e.returncode)
    except KeyboardInterrupt:
        sys.exit(130)


if __name__ == "__main__":
    main()
