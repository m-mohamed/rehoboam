# Claude Code Compatibility

Version compatibility documentation for Rehoboam integration with Claude Code.

## Compliance Status

> **Last Audit:** January 2026 | **Status:** COMPLIANT

### What Rehoboam Does (Allowed)

Rehoboam operates within Claude Code's official integration patterns:

| Integration Method | Status | Description |
|-------------------|--------|-------------|
| **Official Hooks API** | Yes | Uses stdin JSON from registered hooks in `.claude/settings.json` |
| **Official CLI** | Yes | Spawns `claude` command directly via tmux |
| **Monitoring Only** | Yes | Observes hook events, does not intercept API traffic |
| **No Token Abuse** | Yes | Does not use consumer OAuth tokens for automation |
| **additionalContext Output** | Yes | Returns context via stdout for Claude Code to inject (v2.1.x) |

### What Rehoboam Does NOT Do (Would Be Blocked)

Following Anthropic's January 2026 crackdown on unauthorized "harnesses":

| Blocked Pattern | Rehoboam Status | Notes |
|-----------------|-----------------|-------|
| Spoof Claude Code headers | Not done | We use official `claude` CLI |
| Use OAuth from Pro/Max plans | Not done | We don't access Anthropic APIs directly |
| Act as third-party "harness" | Not done | We orchestrate, not replace |
| Intercept API calls | Not done | Hooks receive events post-facto |
| Bypass pricing via token spoofing | Not done | No API token usage |

### Why Rehoboam Is Compliant

1. **Official Hook System**: Rehoboam registers as a hook handler in Claude Code's settings, receiving events through the official stdin JSON protocol
2. **CLI Spawning**: Agents are spawned using the official `claude` command, not by impersonating Claude Code
3. **Observability Only**: We monitor agent states via hook events, not by intercepting or modifying Claude's communication
4. **No API Access**: Rehoboam never directly calls Anthropic's API or uses subscription tokens
5. **Official Output Protocol**: `additionalContext` is returned via stdout, which Claude Code reads and injects

### Anthropic's January 2026 Policy

Anthropic blocked third-party tools that:
- Spoof Claude Code headers to access models at consumer plan rates
- Use OAuth tokens from Pro/Max subscriptions with external tools
- Create "harnesses" that wrap Claude Code for automated workflows

**Rehoboam is NOT affected** because:
- We don't access Anthropic's API directly
- We use Claude Code as designed (via hooks and CLI)
- We're an orchestration/monitoring layer, not a replacement

### References

- [Anthropic Blocks Unauthorized Harnesses](https://venturebeat.com/technology/anthropic-cracks-down-on-unauthorized-claude-usage-by-third-party-harnesses) (VentureBeat, Jan 2026)
- [Claude Code Legal & Compliance](https://code.claude.com/docs/en/legal-and-compliance)
- [Official Hooks Documentation](https://code.claude.com/docs/en/hooks)

---

## Target Version

| Field | Value |
|-------|-------|
| **Minimum Version** | 2.1.0 |
| **Tested Version** | 2.1.11 |
| **Last Updated** | January 2026 |

Rehoboam should work with any Claude Code version >= 2.1.0, though we recommend staying on the latest release for best hook support.

## Hook Events Used

These are the Claude Code hook events Rehoboam processes. The mapping shows how each event translates to Rehoboam's 3-state model.

| Hook Event | Rehoboam Status | Attention Type | Description |
|------------|-----------------|----------------|-------------|
| `UserPromptSubmit` | working | - | User sent a message, agent processing |
| `PreToolUse` | working | - | Tool execution starting |
| `PostToolUse` | working | - | Tool execution complete |
| `SubagentStart` | working | - | Subagent spawned |
| `SubagentStop` | working | - | Subagent finished |
| `Setup` | working | - | Claude Code 2.1.x initialization/setup phase |
| `PermissionRequest` | attention | permission | Blocking approval needed |
| `SessionStart` | attention | waiting | Session ready for input |
| `Stop` | attention | waiting | Response complete |
| `SessionEnd` | attention | waiting | Session closed |
| `Notification` | attention | notification | Informational alert |
| `PreCompact` | compacting | - | Context compaction in progress |

**Source:** `src/event/mod.rs:273-293`

## Hook Input Fields

Fields extracted from Claude Code's stdin JSON (ClaudeHookInput structure).

### Required Fields

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | String | Unique session identifier |
| `hook_event_name` | String | Hook type that triggered the event |

### Tool Event Fields (Optional)

| Field | Type | Used By | Description |
|-------|------|---------|-------------|
| `tool_name` | String | PreToolUse, PostToolUse | Tool being executed (Bash, Read, Edit, etc.) |
| `tool_input` | JSON | PreToolUse | Tool parameters, used for tracking modified files |
| `tool_use_id` | String | PreToolUse, PostToolUse | Correlates events for latency measurement |

### Event-Specific Fields (Optional)

| Field | Type | Used By | Description |
|-------|------|---------|-------------|
| `reason` | String | Stop, SubagentStop, SessionEnd | Stop reason, used for loop control and stall detection |
| `message` | String | Notification | Notification content |

### Subagent Fields (Optional)

| Field | Type | Used By | Description |
|-------|------|---------|-------------|
| `subagent_id` | String | SubagentStart, SubagentStop | Subagent session identifier |
| `description` | String | SubagentStart | Subagent task description (used for role inference) |
| `duration_ms` | u64 | SubagentStop | Subagent execution duration |

### Claude Code 2.1.x Fields (Implemented)

| Field | Type | Description | TUI Display |
|-------|------|-------------|-------------|
| `context_window.used_percentage` | f64 | Context usage 0-100% | Progress bar when >= 80% |
| `context_window.total_tokens` | u64 | Total context capacity | Stored for reference |
| `agent_type` | String | Agent type from --agent flag ("explore", "plan") | Badge override [E], [P] |
| `permission_mode` | String | "default", "plan", "acceptEdits", etc. | [PLAN] indicator |
| `cwd` | String | Current working directory | Stored for reference |
| `transcript_path` | String | Path to conversation .jsonl | Stored for reference |

**Source:** `src/event/mod.rs:239-254`

## Hook Output Capabilities

### Implemented: additionalContext Return

Rehoboam can inject loop state context into Claude's conversation via the `additionalContext` hook return:

```bash
# Enable context injection for loop mode
rehoboam hook --inject-context
```

When `--inject-context` is enabled and a `.rehoboam/` directory exists:
1. PreToolUse and PostToolUse hooks return JSON with `additionalContext`
2. Claude Code injects this context into the conversation
3. Agent receives loop state, task queue, guardrails on each tool call

**Source:** `src/main.rs:241-261`, `src/rehoboam_loop.rs:1443-1510`

### Not Implemented

| Capability | Available Since | Notes |
|------------|-----------------|-------|
| `updatedInput` in PreToolUse | 2.1.0 | Could modify tool inputs |
| `permissionDecision` | 2.0.0 | Could auto-approve/deny |
| Prompt-based hooks (Haiku) | 2.1.0 | Would require API calls - out of scope |

## CLI Commands Used

Rehoboam spawns and controls Claude Code via these CLI patterns.

### Agent Spawning

```bash
# Basic spawn (no initial prompt)
claude

# Spawn with inline prompt
claude 'your prompt here'

# Loop mode: pipe prompt file to stdin
cat '/path/to/.rehoboam/iteration_prompt.md' | claude
```

### Tmux Integration

Rehoboam uses tmux to manage agent terminals:
- `tmux split-window` - Create new pane for agent
- `tmux send-keys` - Send commands/input to agent
- `tmux kill-pane` - Terminate agent session

**Source:** `src/app/spawn.rs:552-597`, `src/tmux.rs`

## Feature Implementation Status

### Implemented (v2.1.x Integration)

| Feature | Version | Status | Source |
|---------|---------|--------|--------|
| `context_window` fields | 2.1.6+ | Implemented | `event/mod.rs`, `state/agent.rs` |
| `agent_type` field | 2.1.0+ | Implemented | Auto-classifies agent role |
| `permission_mode` field | 2.1.0+ | Implemented | Shows [PLAN] in TUI |
| `cwd` field | 2.0.0+ | Implemented | Stored per agent |
| `transcript_path` field | 2.0.0+ | Implemented | Stored per agent |
| `Setup` hook event | 2.1.10 | Implemented | Maps to "working" status |
| `once: true` for hooks | 2.1.0+ | Implemented | Used in SessionStart |
| `additionalContext` return | 2.1.9+ | Implemented | `--inject-context` flag |

### Context Usage Display

When context usage >= 80%, agent cards show a visual warning:

```
[████████░░] 85% HIGH    # Yellow warning at 80%+
[██████████] 97% FULL    # Red critical at 95%+
```

This replaces the elapsed time display when context is getting full.

### Deferred Items

| Feature | Reason |
|---------|--------|
| Prompt-based hooks (Haiku) | Would require direct Anthropic API calls |
| `${CLAUDE_SESSION_ID}` substitution | Skills-only feature, handled by Claude Code |

## Breaking Changes to Watch

These Claude Code changes could affect Rehoboam functionality.

### Version 2.1.0

- Hook timeout increased from 60s to 10min (no impact, only benefits)
- New `agent_type` field in SessionStart (now implemented)

### Version 2.0.0

- Major hook API restructure (check if upgrading from 1.x)
- Session ID format changes (verify session tracking still works)

## Testing Compatibility

When upgrading Claude Code versions:

```bash
# 1. Check Claude Code version
claude --version

# 2. Run Rehoboam tests
cargo test

# 3. Test hook event handling with 2.1.x fields
echo '{"session_id":"test","hook_event_name":"PreToolUse","context_window":{"used_percentage":75.5},"agent_type":"explore","permission_mode":"plan"}' | rehoboam hook

# 4. Test additionalContext injection
echo '{"session_id":"test","hook_event_name":"PreToolUse"}' | rehoboam hook --inject-context

# 5. Test spawn flow
rehoboam spawn --loop  # Verify loop mode works
```

## References

- [Claude Code Changelog](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md)
- [Claude Code Releases](https://github.com/anthropics/claude-code/releases)
- [Hooks Documentation](https://github.com/anthropics/claude-code/blob/main/docs/hooks.md)
