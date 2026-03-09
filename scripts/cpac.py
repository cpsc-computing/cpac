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
CORPUS_DIR = REPO_ROOT / "benches" / "corpora"
PROFILE_DIR = REPO_ROOT / "benches" / "profiles"


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

    Scans all files in benches/profiles/ and returns the one whose
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

            # Collect files from corpus directory
            files = sorted(
                f for f in data_dir.rglob("*")
                if f.is_file() and f.suffix != ".cpac"
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
                size_kb = f.stat().st_size / 1024
                rel = f.relative_to(data_dir)
                print(f"  {rel} ({size_kb:.1f} KB)...", end=" ", flush=True)

                cmd = [binary, "benchmark", str(f), "-n", str(iterations)]
                if track1:
                    cmd.append("--track1")
                if skip_baselines:
                    cmd.append("--skip-baselines")

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
        if len(urls) > 1 or kind == "http_file_multi":
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
                    help="Benchmark profile id (default: balanced). See benches/profiles/")
    p.add_argument("--skip-baselines", action="store_true", help="Skip baseline engines")

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
        "build": cmd_build,
        "test": cmd_test,
        "clippy": cmd_clippy,
        "fmt": cmd_fmt,
        "check": cmd_check,
        "bench": cmd_bench,
        "benchmark-all": cmd_benchmark_all,
        "criterion": cmd_criterion,
        "pgo-build": cmd_pgo_build,
        "download-corpus": cmd_download_corpus,
        "setup": cmd_setup,
        "info": cmd_info,
        "analyze": cmd_analyze,
        "calibrate": cmd_calibrate,
        "train-dict": cmd_train_dict,
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
