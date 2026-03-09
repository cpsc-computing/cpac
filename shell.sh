#!/bin/bash
# CPAC Unified Shell Script (Linux/macOS)
# Handles both bootstrap (first-time setup) and command execution.
#
# Usage:
#   ./shell.sh                    # Enter interactive venv shell
#   ./shell.sh build --release    # Run cpac.py build --release
#   ./shell.sh test               # Run cpac.py test
#   ./shell.sh bench file --quick # Run cpac.py bench file --quick

set -e

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
WORK_DIR="$REPO_ROOT/.work"
VENV_DIR="$WORK_DIR/env"
VENV_PYTHON="$VENV_DIR/bin/python3"

# Check if venv exists
if [ ! -f "$VENV_PYTHON" ]; then
    echo "========================================"
    echo "CPAC First-Time Setup"
    echo "========================================"

    # 1. Check Python 3
    echo ""
    echo "[1/3] Checking Python..."

    if command -v python3 &> /dev/null; then
        PYTHON_CMD="python3"
        echo "Found: $($PYTHON_CMD --version)"
    elif command -v python &> /dev/null; then
        VERSION=$(python --version 2>&1)
        if [[ $VERSION == *"Python 3"* ]]; then
            PYTHON_CMD="python"
            echo "Found: $VERSION"
        else
            echo "ERROR: Python 3 required, found: $VERSION"
            echo "Install: sudo apt install python3 python3-venv (Ubuntu)"
            echo "     or: brew install python@3.12 (macOS)"
            exit 1
        fi
    else
        echo "ERROR: Python 3 not found"
        echo "Install: sudo apt install python3 python3-venv (Ubuntu)"
        echo "     or: brew install python@3.12 (macOS)"
        exit 1
    fi

    # 2. Create venv
    echo ""
    echo "[2/3] Creating virtual environment..."
    mkdir -p "$WORK_DIR"
    $PYTHON_CMD -m venv "$VENV_DIR"
    echo "Created venv at $VENV_DIR"

    # 3. Install requirements
    echo ""
    echo "[3/3] Installing dependencies..."
    REQUIREMENTS_FILE="$REPO_ROOT/requirements.txt"

    "$VENV_PYTHON" -m pip install --quiet --upgrade pip
    if [ -f "$REQUIREMENTS_FILE" ]; then
        "$VENV_PYTHON" -m pip install --quiet -r "$REQUIREMENTS_FILE"
    fi
    echo "Dependencies installed"

    echo ""
    echo "========================================"
    echo "Setup Complete!"
    echo "========================================"
    echo ""
fi

# Ensure cargo is on PATH
if [ -d "$HOME/.cargo/bin" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# Execute command in venv
if [ $# -eq 0 ]; then
    # No args: enter interactive shell inside the venv
    echo "Entering CPAC venv shell. Type 'exit' to leave."
    echo "  Python: $VENV_PYTHON"
    echo "  Cargo:  $(command -v cargo 2>/dev/null || echo 'not found')"
    echo ""
    # Activate and drop into a subshell
    BASH_ENV="$VENV_DIR/bin/activate" exec bash --rcfile "$VENV_DIR/bin/activate" -i
else
    # Run command via cpac.py
    "$VENV_PYTHON" "$REPO_ROOT/scripts/cpac.py" "$@"
fi
