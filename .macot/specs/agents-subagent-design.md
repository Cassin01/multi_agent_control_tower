# Design: Inter-Expert Messaging Subagent Extraction

## 1. Overview

Extract the Inter-Expert Messaging section (~66 lines, ~2000-3000 tokens) from `core.md.tmpl` into a Claude CLI `--agents` subagent. This reduces each expert's system prompt context usage by making messaging an on-demand capability invoked only when needed.

Currently, every expert always has the full messaging protocol in their system prompt regardless of whether they ever send a message. By moving this to a subagent, the messaging knowledge is loaded only when an expert explicitly invokes the messaging agent.

## 2. Architecture

```mermaid
graph TD
    A[load_instruction_with_template] --> B[core.md.tmpl<br/>reduced ~120 lines]
    A --> C[role instructions]
    A --> D[agents::render_agents_json]

    D --> E[messaging.md.tmpl]
    D --> F[InstructionResult.agents_json]

    F --> G[write_agents_file]
    G --> H[.macot/system_prompt/<br/>expert{id}_agents.json]

    B --> I[write_instruction_file]
    I --> J[.macot/system_prompt/<br/>expert{id}.md]

    H --> K[launch_claude]
    J --> K
    K --> L["claude --dangerously-skip-permissions<br/>--append-system-prompt ...<br/>--agents ..."]
```

**Data flow**: Template loading assembles two outputs per expert: the system prompt file (existing) and the agents JSON file (new). Both are passed to `launch_claude()` which constructs the final Claude CLI command.

## 3. Components and Interfaces

### 3.1 Agent Template

- **File**: `instructions/templates/agents/messaging.md.tmpl`
- **Purpose**: Contains the full Inter-Expert Messaging protocol as a minijinja template
- **Template variables**: `{{ expert_id }}`, `{{ expert_name }}`

Content extracted from `core.md.tmpl` lines 109-175, preserving all messaging format documentation, recipient targeting, message types, and examples.

### 3.2 Agent Renderer

- **File**: `src/instructions/agents.rs`
- **Purpose**: Loads agent templates, renders them, produces JSON for `--agents`

```rust
pub fn render_agents_json(
    core_path: &Path,
    expert_id: u32,
    expert_name: &str,
) -> Result<Option<String>>
```

Returns `None` if no agent templates exist (backward compatible). Returns `Some(json_string)` with the format:

```json
{
  "messaging": {
    "description": "Send messages to other experts through the MACOT inter-expert messaging system",
    "prompt": "<rendered template content>"
  }
}
```

### 3.3 Agents File Writer

- **File**: `src/instructions/file_writer.rs` (existing, extended)
- **Purpose**: Writes agents JSON to disk for shell expansion

```rust
pub fn agents_file_path(queue_path: &Path, expert_id: u32) -> PathBuf
pub fn write_agents_file(queue_path: &Path, expert_id: u32, json: &str) -> Result<PathBuf>
```

Output path: `.macot/system_prompt/expert{id}_agents.json`

### 3.4 Extended InstructionResult

- **File**: `src/instructions/template.rs` (existing, modified)
- **Purpose**: Carries agents JSON alongside system prompt content

```rust
pub struct InstructionResult {
    pub content: String,
    pub requested_role: String,
    pub used_general_fallback: bool,
    pub agents_json: Option<String>,  // NEW
}
```

### 3.5 Extended launch_claude()

- **File**: `src/session/claude.rs` (existing, modified)
- **Purpose**: Passes `--agents` flag to Claude CLI

```rust
pub async fn launch_claude(
    &self,
    expert_id: u32,
    working_dir: &str,
    instruction_file: Option<&Path>,
    agents_file: Option<&Path>,  // NEW
) -> Result<()>
```

Generated command: `cd {dir} && claude --dangerously-skip-permissions --append-system-prompt "$(cat '{instruction_file}')" --agents "$(cat '{agents_file}')"`

### 3.6 Extended FeatureExecutor

- **File**: `src/feature/executor.rs` (existing, modified)
- **Purpose**: Stores agents file path for feature execution relaunches

```rust
// New field and getter
agents_file: Option<PathBuf>,
pub fn agents_file(&self) -> Option<&PathBuf>
```

## 4. Data Models

### Agents JSON Schema

```json
{
  "<agent_name>": {
    "description": "string - shown to Claude as agent description",
    "prompt": "string - full system prompt for the subagent"
  }
}
```

This matches the Claude CLI `--agents` format exactly.

### File Layout

```
.macot/
  system_prompt/
    expert0.md              # System prompt (existing)
    expert0_agents.json     # Agents definition (new)
    expert1.md
    expert1_agents.json
    ...
instructions/
  templates/
    core.md.tmpl            # Core template (modified, reduced)
    agents/
      messaging.md.tmpl     # Messaging agent template (new)
```

## 5. Error Handling

| Error Case | Handling |
|------------|----------|
| No agents template directory | `render_agents_json()` returns `Ok(None)`, no `--agents` flag passed |
| Template render failure | Propagates as `anyhow::Error`, prevents expert launch |
| JSON serialization failure | Propagates as `anyhow::Error`, prevents expert launch |
| File write failure | Propagates as `anyhow::Error`, prevents expert launch |
| Invalid JSON at runtime | Claude CLI rejects and won't start; mitigated by unit testing JSON output |

## 6. Correctness Properties

1. **Backward Compatibility** — If `instructions/templates/agents/` does not exist, the system behaves identically to the current version (no `--agents` flag).

2. **Template Variable Rendering** — `expert_id` and `expert_name` in the agents template are rendered with the same values as in `core.md.tmpl`.

3. **Valid JSON Output** — `render_agents_json()` always produces valid JSON parseable by Claude CLI's `--agents` handler.

4. **Call Site Completeness** — All 6 `launch_claude()` call sites pass the agents file when available. Adding the `agents_file` parameter to `launch_claude()` produces compile errors at uncovered call sites.

5. **Core Template Reduction** — `core.md.tmpl` no longer contains the Inter-Expert Messaging section (66 lines removed), replaced by a 3-line Available Agents reference.

6. **File Isolation** — Each expert gets their own `expert{id}_agents.json` with their specific `expert_id` rendered, not shared across experts.

## 7. Testing Strategy

All tests follow TDD Red-Green-Refactor cycle.

### Unit Tests (agents.rs)

| Test | Property |
|------|----------|
| `render_agents_json_returns_none_when_no_template` | Property 1 |
| `render_agents_json_returns_valid_json_with_template` | Property 3 |
| `render_agents_json_renders_expert_id` | Property 2 |
| `render_agents_json_renders_expert_name` | Property 2 |

### Unit Tests (file_writer.rs)

| Test | Property |
|------|----------|
| `agents_file_path_returns_expected_path` | Property 6 |
| `write_agents_file_creates_dir_and_file` | Property 6 |

### Unit Tests (template.rs)

| Test | Property |
|------|----------|
| `instruction_result_includes_agents_json` | Property 2 |
| `instruction_result_agents_json_none_without_template` | Property 1 |

### Unit Tests (claude.rs)

| Test | Property |
|------|----------|
| `launch_claude_with_agents_file` | Property 4 |
| `launch_claude_without_agents_file` | Property 1 |
| `launch_claude_with_both_instruction_and_agents` | Property 4 |

### Content Tests

| Test | Property |
|------|----------|
| `core_template_does_not_contain_messaging_section` | Property 5 |
| `core_template_references_messaging_agent` | Property 5 |

### Integration Verification

- `make test` — all tests pass
- `make build` — compiles successfully
- Manual: `macot start` and verify `--agents` in Claude command via tmux
