---
description: Install Rehoboam hooks to Claude Code projects with intelligent discovery
allowed-tools: Bash(cargo:*), Bash(ls:*), Read
argument-hint: [path or --all or --list]
---

# Hook Installation

Install Rehoboam hooks to Claude Code projects.

## Arguments

- `$ARGUMENTS` can be:
  - Empty: Install to current directory
  - A path: Install to specific project
  - `--all`: Discover and install to multiple repos
  - `--list`: List discovered repositories
  - `--force`: Force overwrite existing hooks

## Commands

### Install to Current Project
```bash
cargo run -- init
```

### Install to Specific Path
```bash
cargo run -- init $ARGUMENTS
```

### Discover Repositories
```bash
cargo run -- init --all
```

### List Discovered Repos
```bash
cargo run -- init --list
```

### Force Overwrite
```bash
cargo run -- init --force $ARGUMENTS
```

## Post-Installation Checklist

After installation, verify:

1. Check hooks are installed:
   ```bash
   ls -la .claude/settings.local.json
   ```

2. Verify hook configuration contains:
   - `hooks.PreToolUse` with rehoboam command
   - `hooks.PostToolUse` with rehoboam command
   - `hooks.Stop` with rehoboam command

3. Ensure `$REHOBOAM_SOCKET` environment variable is set or will be auto-discovered

## Troubleshooting

Common issues:
- "Socket not found": Ensure Rehoboam TUI is running (`cargo run`)
- "Permission denied": Check file permissions on .claude directory
- "Hooks not firing": Restart Claude Code session after installation
