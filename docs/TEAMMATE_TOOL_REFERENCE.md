# Claude Code's Hidden TeammateTool Feature

## Overview
Claude Code, a command-line interface tool developed by Anthropic for interacting with their Claude AI models, contains a hidden multi-agent orchestration system called **TeammateTool**. This feature allows for the creation and management of AI agent teams (or "swarms") to handle complex tasks collaboratively, such as software development, code reviews, and research. It was first spotlighted in a GitHub commit to the "claude-code-system-prompts" repository, which documents system prompts extracted from Claude Code. The tool enables spawning teams, assigning tasks, inter-agent communication, plan approvals, and shutdowns, simulating human-like team workflows but with AI agents.

The feature is described as "secret" or "turned off" because it is **feature-flagged** and not enabled by default for most users. It exists in the Claude Code binary (e.g., version 2.1.19) but is gated behind two internal flags (`I9()` and `qFB()`), with no public toggle available. Users have discovered it by extracting strings from the binary and testing prompts, revealing that Claude initially denies knowledge of the tool until provided with its source code.

This capability aligns with Anthropic's broader vision for agentic AI, where self-managed AI teams could emerge by 2026-2027, leading to an explosion in software creation and quality. It's seen as a step toward "software factories" where AI teams handle end-to-end development autonomously.

## Key Components and Operations
TeammateTool is invoked through structured JSON tool calls in Claude's prompt system. The core operations include:

- **spawnTeam**: Creates a new team with a given `team_name`. This sets up directories for configuration, messages, and tasks (e.g., `~/.claude/teams/{team-name}/`).
- **discoverTeams**: Lists available teams.
- **requestJoin / approveJoin / rejectJoin**: Allows agents to request and manage team membership.
- **write**: Sends a direct message to a specific teammate (e.g., `{"operation": "write", "target": "agent_id", "message": "content"}`).
- **broadcast**: Sends a message to all team members.
- **approvePlan / rejectPlan**: Leader approves or rejects proposed plans from agents.
- **requestShutdown / approveShutdown / rejectShutdown**: Initiates and manages graceful team shutdowns.
- **cleanup**: Removes team resources after shutdown.

Task management integrates with tools like `TaskCreate` and `TaskList`:
- **TaskCreate**: Defines tasks with subjects, descriptions, owners, blockers, and status (e.g., active, blocked, done).
- **TaskList**: Views all tasks for coordination.

Environment variables control behavior:
- `CLAUDE_CODE_TEAM_NAME`: Specifies the team.
- `CLAUDE_CODE_AGENT_ID / _NAME / _TYPE`: Identifies individual agents.
- `CLAUDE_CODE_PLAN_MODE_REQUIRED`: Enforces plan approval workflows.

Spawn back-ends support different execution modes:
- iTerm2 split panes (macOS for visual debugging).
- tmux windows (cross-platform).
- In-process (single process for efficiency).

## System Prompts and Workflow
The feature relies on specialized system prompts added in updates like v2.1.16:

- **Exit Plan Mode with Swarm**: Guides the leader to create tasks, spawn teammates, assign work, and synthesize results when `isSwarm=true`.
- **Teammate Communication**: Ensures agents communicate only via TeammateTool (plain text is invisible to others).
- **Delegate Mode**: Restricts tools to TeammateTool and task-related ones during delegation.
- **Plan Mode (5-Phase)**: Structured planning in phases (Explore, Design, Review, Final Plan, Exit), supporting multi-agent exploration.
- **Team Shutdown**: Handles shutdown requests and approvals.
- **Tool Execution Denied**: Provides workarounds for denied tools without malicious bypasses.

Workflow example:
1. Leader spawns a team and agents.
2. Agents claim tasks from a shared queue.
3. Communication via write/broadcast.
4. Leader approves plans and shutdowns.
5. Heartbeats monitor agent health; timeouts release tasks.

Interaction patterns include leader-swarm, pipeline, council, and watchdog setups.

## Enabling and Accessing the Feature
Currently, there is **no official way to enable TeammateTool** as it's internally gated. Users have verified its presence by:

```bash
# Check version
claude --version

# Extract strings for TeammateTool
strings ~/.local/share/claude/versions/$(claude --version | cut -d' ' -f1) | grep -i TeammateTool

# List operations
strings ~/.local/share/claude/versions/$(claude --version | cut -d' ' -f1) | grep -E "spawnTeam|discoverTeams|requestJoin|approveJoin"

# Spot env vars
strings ~/.local/share/claude/versions/$(claude --version | cut -d' ' -f1) | grep "CLAUDE_CODE_TEAM"
```

To test knowledge in Claude:
- Ask about TeammateTool; it may deny existence.
- Provide prompt source code (from repos like claude-code-system-prompts); it then acknowledges and explains.

Speculation: Future updates may expose it publicly, as Anthropic has hinted at self-managed AI teams.

## Capabilities and Use Cases
TeammateTool enables scalable AI collaboration:

- **Code-Review Swarm**: Specialized agents (e.g., security, performance) review code in parallel.
- **Feature Factory**: Architect, backend, frontend, and test agents build features end-to-end.
- **Bug-Hunt Squad**: Agents analyze logs, reproduce issues, and identify root causes.
- **Self-Organizing Refactor**: Agents scout files, claim refactor tasks, and verify with tests.
- **Research Council**: Agents debate technologies and provide collective recommendations.
- **Deployment Guardian**: Pre- and post-flight checks with approval gates.
- **Living Documentation**: Auto-update docs based on code changes.
- **Infinite Context Window**: Distribute large codebases across agents for specialized queries.

## Failure Handling
Built-in mitigations:
- Agent crashes: 5-minute heartbeat timeout releases tasks.
- Leader failure: Workers idle after current tasks.
- Infinite loops: Forced shutdown timeouts.
- Deadlocks: Dependency cycle detection.
- Resource issues: Limits on agents per team.

## Implications
Once enabled, this could multiply software output quality by 10x, as predicted. It represents a shift toward AI-driven "software factories," potentially disrupting development workflows and economies.

## References
- GitHub Commit (System Prompts): https://github.com/Piebald-AI/claude-code-system-prompts/commit/e8da828?diff=unified
- Gist on TeammateTool: https://gist.github.com/kieranklaassen/d2b35569be2c7f1412c64861a219d51f
- Reddit Discussion: https://www.reddit.com/r/ClaudeCode/comments/1plz7st/new_swarm_feature_coming_to_claude_code_soon
- X Posts: Various discussions on TeammateTool as an orchestrator.

---

## Relevance to Rehoboam

### Current Rehoboam Concepts That Align
- **Agent Roles**: Planner/Worker separation mirrors TeammateTool's leader/teammate model
- **Loop Mode**: `.rehoboam/` state management similar to `~/.claude/teams/{team-name}/`
- **Task Queue**: `tasks.md` concept aligns with TaskCreate/TaskList operations
- **Spawn Dialog**: Already handles agent spawning

### Preparation Areas
1. **Team State Management**: Extend beyond single-agent to team tracking
2. **Inter-Agent Communication**: Add message/broadcast visualization
3. **Heartbeat Monitoring**: Already have agent status; extend to health tracking
4. **Plan Approval Flow**: UI for approve/reject workflows
5. **Environment Variables**: Support new CLAUDE_CODE_TEAM_* vars

---
*Last Updated: 2026-01-23*
