#!/bin/bash
# Debug script to check tmux environment for Rehoboam
#
# Run this in the same terminal/pane where Claude Code runs to verify
# the environment is correctly set up for hook events.

echo "=== Rehoboam Environment Debug ==="
echo ""

# Check tmux
echo "TMUX_PANE: ${TMUX_PANE:-NOT SET}"
echo "TMUX: ${TMUX:-NOT SET}"

# Check other terminals (for reference)
echo "WEZTERM_PANE: ${WEZTERM_PANE:-NOT SET}"
echo "KITTY_WINDOW_ID: ${KITTY_WINDOW_ID:-NOT SET}"
echo "ITERM_SESSION_ID: ${ITERM_SESSION_ID:-NOT SET}"
echo ""

# Check socket
SOCKET="${REHOBOAM_SOCKET:-${XDG_RUNTIME_DIR:-/tmp}/rehoboam.sock}"
echo "Socket path: $SOCKET"
if [ -S "$SOCKET" ]; then
    echo "Socket status: EXISTS (TUI is running)"
else
    echo "Socket status: NOT FOUND (TUI not running?)"
fi
echo ""

# Check if we're actually in tmux
if [ -n "$TMUX" ]; then
    echo "✓ Running inside tmux"
    echo "  Current pane: $(tmux display-message -p '#{pane_id}')"
    echo "  All panes:"
    tmux list-panes -F "    #{pane_id} - #{pane_title} (#{pane_current_command})"
else
    echo "✗ NOT running in tmux!"
    echo "  Rehoboam's Enter key (jump to pane) won't work without tmux"
fi
echo ""

# Check hooks in current project
if [ -f ".claude/settings.json" ]; then
    echo "✓ Hooks installed in current project"
    if grep -q "rehoboam hook" .claude/settings.json; then
        echo "  Using v1.0 hooks (rehoboam hook)"
    elif grep -q "rehoboam send" .claude/settings.json; then
        echo "  Using legacy hooks (rehoboam send) - consider re-running: rehoboam init"
    fi
else
    echo "✗ No hooks in current project"
    echo "  Run: rehoboam init"
fi
