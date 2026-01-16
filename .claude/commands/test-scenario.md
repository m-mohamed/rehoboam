---
description: Run specific test scenarios for Rehoboam event handling and loop modes
allowed-tools: Bash(./scripts/*), Bash(cargo:*)
argument-hint: [basic|askuser|permission|loop|all|unit]
---

# Test Scenarios

Run specific test scenarios to validate Rehoboam event handling.

## Available Scenarios

### Unit Tests
```bash
cargo test --all-features
```

### Event Simulation Scenarios

Located in `./scripts/test-events.sh`:

1. **basic** - Test Working → Idle state transitions
   ```bash
   ./scripts/test-events.sh basic
   ```

2. **askuser** - Test AskUserQuestion attention flow
   ```bash
   ./scripts/test-events.sh askuser
   ```

3. **permission** - Test PermissionRequest attention handling
   ```bash
   ./scripts/test-events.sh permission
   ```

4. **loop** - Test loop mode state transitions
   ```bash
   ./scripts/test-events.sh loop
   ```

### Hook Testing
```bash
./scripts/test-hooks.sh
```

## Scenario Selection

Based on `$ARGUMENTS`:

- `unit` or empty: Run `cargo test --all-features`
- `basic`: Run basic event scenario
- `askuser`: Run AskUserQuestion scenario
- `permission`: Run PermissionRequest scenario
- `loop`: Run loop mode scenario
- `all`: Run all event scenarios sequentially
- `hooks`: Run hook integration test

## Output Interpretation

Each scenario tests specific state machine transitions:

- **Working state**: Agent is actively processing
- **Idle state**: Agent is waiting for input
- **Attention state**: Agent needs user interaction
- **Loop iteration**: Ralph loop completed an iteration

Watch the Rehoboam TUI (in another terminal) to see state changes.

## Pre-requisites

1. Rehoboam TUI must be running (`cargo run -- --debug`)
2. Socket must be available
3. Scripts must be executable:
   ```bash
   chmod +x ./scripts/*.sh
   ```
