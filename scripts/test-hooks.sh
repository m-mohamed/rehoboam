#!/bin/bash
# Test script for Rehoboam hook events using Claude Code headless mode
#
# This tests that hook events flow correctly from Claude Code to Rehoboam.
# Run rehoboam in another terminal first, then run this script.
#
# Usage: ./scripts/test-hooks.sh

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}=== Rehoboam Hook Event Test ===${NC}"
echo ""

# Check if rehoboam hooks are installed
HOOK_FILE=".claude/settings.local.json"
if [ ! -f "$HOOK_FILE" ]; then
    echo -e "${RED}Error: Rehoboam hooks not installed in this project${NC}"
    echo "Run: rehoboam init"
    exit 1
fi

echo -e "${GREEN}✓ Hooks installed${NC}"

# Check if rehoboam socket exists (TUI is running)
SOCKET="${REHOBOAM_SOCKET:-${XDG_RUNTIME_DIR:-/tmp}/rehoboam.sock}"
if [ ! -S "$SOCKET" ]; then
    echo -e "${RED}Error: Rehoboam TUI not running (no socket at $SOCKET)${NC}"
    echo "Start rehoboam in another terminal first"
    exit 1
fi

echo -e "${GREEN}✓ Rehoboam TUI running${NC}"
echo ""

# Test 1: Simple prompt that triggers tool use
echo -e "${YELLOW}Test 1: Basic tool use (Read tool)${NC}"
echo "Running: claude -p 'Read the first 5 lines of Cargo.toml' --allowedTools Read"
claude -p "Read the first 5 lines of Cargo.toml and tell me the package name" \
    --allowedTools Read \
    --output-format text \
    2>/dev/null || true

echo -e "${GREEN}✓ Test 1 complete - check Rehoboam for Working→Idle transitions${NC}"
echo ""

# Test 2: Multiple tool calls
echo -e "${YELLOW}Test 2: Multiple tool calls${NC}"
echo "Running: claude -p 'List .rs files and count them'"
claude -p "List all .rs files in src/ and count how many there are" \
    --allowedTools Bash,Glob \
    --output-format text \
    2>/dev/null || true

echo -e "${GREEN}✓ Test 2 complete - check Rehoboam for tool latency tracking${NC}"
echo ""

# Test 3: Longer running task
echo -e "${YELLOW}Test 3: Multi-step task${NC}"
echo "Running: claude -p 'Analyze the codebase structure'"
claude -p "Briefly describe the structure of this Rust project - just list the main modules" \
    --allowedTools Read,Glob \
    --output-format text \
    2>/dev/null || true

echo -e "${GREEN}✓ Test 3 complete${NC}"
echo ""

echo -e "${GREEN}=== All tests complete ===${NC}"
echo "Check Rehoboam TUI for:"
echo "  - Agent appeared in Kanban view"
echo "  - Status transitions (Working → Idle)"
echo "  - Tool names displayed during execution"
echo "  - Elapsed time tracking"
