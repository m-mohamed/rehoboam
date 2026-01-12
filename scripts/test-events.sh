#!/bin/bash
# Direct hook event testing for Rehoboam
#
# Sends simulated hook events directly to Rehoboam's socket
# without needing Claude Code. Useful for testing status transitions.
#
# Usage: ./scripts/test-events.sh [test_name]
#
# Tests:
#   basic      - Simple Working → Idle transition
#   askuser    - AskUserQuestion attention flow (the bug we fixed)
#   permission - PermissionRequest attention flow
#   loop       - Loop mode transitions

set -e

SOCKET="${REHOBOAM_SOCKET:-${XDG_RUNTIME_DIR:-/tmp}/rehoboam.sock}"
PANE_ID="test-$$"  # Unique pane ID for this test
PROJECT="test-project"
TIMESTAMP=$(date +%s)

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

send_event() {
    local json="$1"
    echo "$json" | nc -U "$SOCKET" -w 1 2>/dev/null || echo "$json" | socat - UNIX-CONNECT:"$SOCKET" 2>/dev/null || {
        echo -e "${RED}Failed to send event. Is rehoboam running?${NC}"
        return 1
    }
    echo -e "${CYAN}Sent:${NC} $json"
}

test_basic() {
    echo -e "${YELLOW}=== Test: Basic Working → Idle ===${NC}"

    # Session start
    send_event "{\"event\":\"SessionStart\",\"status\":\"idle\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$TIMESTAMP}"
    sleep 0.5

    # User prompt
    send_event "{\"event\":\"UserPromptSubmit\",\"status\":\"working\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+1))}"
    sleep 1

    # Tool use
    send_event "{\"event\":\"PreToolUse\",\"status\":\"working\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+2)),\"tool_name\":\"Read\"}"
    sleep 1

    send_event "{\"event\":\"PostToolUse\",\"status\":\"working\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+3)),\"tool_name\":\"Read\"}"
    sleep 0.5

    # Stop
    send_event "{\"event\":\"Stop\",\"status\":\"idle\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+4))}"

    echo -e "${GREEN}✓ Check Rehoboam: Agent should be in IDLE column${NC}"
}

test_askuser() {
    echo -e "${YELLOW}=== Test: AskUserQuestion (the bug we fixed) ===${NC}"
    echo -e "Expected: Agent stays in ATTENTION while waiting for user response"
    echo ""

    # Session start
    send_event "{\"event\":\"SessionStart\",\"status\":\"idle\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$TIMESTAMP}"
    sleep 0.5

    # User prompt
    send_event "{\"event\":\"UserPromptSubmit\",\"status\":\"working\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+1))}"
    sleep 1

    # AskUserQuestion PreToolUse - sets current_tool
    echo -e "${CYAN}→ PreToolUse(AskUserQuestion) - should set current_tool${NC}"
    send_event "{\"event\":\"PreToolUse\",\"status\":\"working\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+2)),\"tool_name\":\"AskUserQuestion\"}"
    sleep 1

    # Stop fires BEFORE PostToolUse (this is the bug scenario)
    echo -e "${CYAN}→ Stop - with fix, should go to ATTENTION (not IDLE)${NC}"
    send_event "{\"event\":\"Stop\",\"status\":\"idle\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+3))}"

    echo ""
    echo -e "${GREEN}✓ Check Rehoboam now!${NC}"
    echo -e "  - Card should show 'AskUserQues...' as tool"
    echo -e "  - Card should be in ${YELLOW}ATTENTION${NC} column (not IDLE)"
    echo ""
    echo "Press Enter to simulate user responding (sends PostToolUse + new Stop)..."
    read

    # User responds - PostToolUse fires
    send_event "{\"event\":\"PostToolUse\",\"status\":\"working\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+10)),\"tool_name\":\"AskUserQuestion\"}"
    sleep 0.5

    # Final stop
    send_event "{\"event\":\"Stop\",\"status\":\"idle\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+11))}"

    echo -e "${GREEN}✓ Now agent should be in IDLE column${NC}"
}

test_permission() {
    echo -e "${YELLOW}=== Test: PermissionRequest ===${NC}"

    # Session start
    send_event "{\"event\":\"SessionStart\",\"status\":\"idle\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$TIMESTAMP}"
    sleep 0.5

    # User prompt
    send_event "{\"event\":\"UserPromptSubmit\",\"status\":\"working\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+1))}"
    sleep 1

    # Permission request
    echo -e "${CYAN}→ PermissionRequest - should go to ATTENTION${NC}"
    send_event "{\"event\":\"PermissionRequest\",\"status\":\"attention\",\"attention_type\":\"permission\",\"pane_id\":\"$PANE_ID\",\"project\":\"$PROJECT\",\"timestamp\":$((TIMESTAMP+2)),\"tool_name\":\"Bash\"}"

    echo -e "${GREEN}✓ Check Rehoboam: Agent should be in ATTENTION column${NC}"
}

# Main
case "${1:-basic}" in
    basic)
        test_basic
        ;;
    askuser)
        test_askuser
        ;;
    permission)
        test_permission
        ;;
    all)
        test_basic
        echo ""
        PANE_ID="test-askuser-$$"
        test_askuser
        echo ""
        PANE_ID="test-perm-$$"
        test_permission
        ;;
    *)
        echo "Usage: $0 [basic|askuser|permission|all]"
        exit 1
        ;;
esac
