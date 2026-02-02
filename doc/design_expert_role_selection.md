# Expert Role Selection Feature - Architecture Design

## Overview

This document describes the architecture for the Expert Role Selection feature, enabling dynamic assignment of roles to experts within a MACOT session.

## Requirements

1. **Default on Startup**: If expert configuration is not in session, use default settings
2. **Persist to Session**: Save each expert's role to session files on startup
3. **Role Selection**: Allow selecting and assigning roles to experts via UI
4. **Session Persistence**: Role information persists across tower restarts
5. **Role Application**: On role change, send `/clear` and resend role-specific instructions

---

## Architecture

### Data Flow

```
┌──────────────────────────────────────────────────────────────────────┐
│                        Tower Startup                                  │
│                                                                       │
│   ┌─────────────────┐    ┌─────────────────┐    ┌────────────────┐  │
│   │ Load Config     │───▶│ Check Session   │───▶│ Apply Defaults │  │
│   │ (defaults)      │    │ Roles File      │    │ if Missing     │  │
│   └─────────────────┘    └─────────────────┘    └────────────────┘  │
└──────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌──────────────────────────────────────────────────────────────────────┐
│                        Runtime Operation                              │
│                                                                       │
│   ┌─────────────────┐    ┌─────────────────┐    ┌────────────────┐  │
│   │ User Selects    │───▶│ Save to Session │───▶│ Send /clear    │  │
│   │ Role in UI      │    │ Roles File      │    │ + Instructions │  │
│   └─────────────────┘    └─────────────────┘    └────────────────┘  │
└──────────────────────────────────────────────────────────────────────┘
```

---

## Data Structures

### SessionExpertRoles (New)

**File Location**: `queue/sessions/{hash}/expert_roles.yaml`

```yaml
# Session-level role assignments
session_hash: "dc9c02aa"
created_at: "2026-02-01T13:00:00Z"
updated_at: "2026-02-01T13:30:00Z"
assignments:
  - expert_id: 0
    role: "architect"      # Maps to instructions/architect.md
    assigned_at: "2026-02-01T13:00:00Z"
  - expert_id: 1
    role: "frontend"
    assigned_at: "2026-02-01T13:00:00Z"
  - expert_id: 2
    role: "backend"
    assigned_at: "2026-02-01T13:15:00Z"
  - expert_id: 3
    role: "tester"
    assigned_at: "2026-02-01T13:00:00Z"
```

### Rust Structs

```rust
// src/context/role.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub expert_id: u32,
    pub role: String,
    pub assigned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExpertRoles {
    pub session_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub assignments: Vec<RoleAssignment>,
}

impl SessionExpertRoles {
    pub fn new(session_hash: String) -> Self { ... }
    pub fn get_role(&self, expert_id: u32) -> Option<&str> { ... }
    pub fn set_role(&mut self, expert_id: u32, role: String) { ... }
    pub fn initialize_defaults(&mut self, config: &Config) { ... }
}
```

### AvailableRoles (New)

```rust
// src/context/role.rs

#[derive(Debug, Clone)]
pub struct AvailableRoles {
    pub roles: Vec<RoleInfo>,
}

#[derive(Debug, Clone)]
pub struct RoleInfo {
    pub name: String,           // e.g., "architect"
    pub display_name: String,   // e.g., "Architect"
    pub description: String,    // First line of role's .md file
}

impl AvailableRoles {
    /// Scan instructions directory for available roles
    pub fn from_instructions_path(path: &Path) -> Result<Self> { ... }
}
```

---

## UI Components

### RoleSelector Widget (New)

**File**: `src/tower/widgets/role_selector.rs`

A popup/modal widget for selecting expert roles:

```
┌─────────────────────────────────────────┐
│       Select Role for Expert 0          │
│         (current: architect)            │
├─────────────────────────────────────────┤
│  > [1] architect  - System design       │
│    [2] frontend   - UI/UX development   │
│    [3] backend    - Server logic        │
│    [4] tester     - Quality assurance   │
│    [5] general    - General purpose     │
├─────────────────────────────────────────┤
│  Enter: Select  |  Esc: Cancel          │
└─────────────────────────────────────────┘
```

**Implementation**:

```rust
// src/tower/widgets/role_selector.rs

pub struct RoleSelector {
    visible: bool,
    expert_id: Option<u32>,
    current_role: String,
    available_roles: Vec<RoleInfo>,
    selected_index: usize,
}

impl RoleSelector {
    pub fn new() -> Self { ... }
    pub fn show(&mut self, expert_id: u32, current_role: &str) { ... }
    pub fn hide(&mut self) { ... }
    pub fn is_visible(&self) -> bool { ... }
    pub fn selected_role(&self) -> Option<&str> { ... }
    pub fn next(&mut self) { ... }
    pub fn prev(&mut self) { ... }
    pub fn render(&self, frame: &mut Frame, area: Rect) { ... }
}
```

### StatusDisplay Enhancement

Add role display to each expert entry:

```
Current:
  [0] architect  - idle

Enhanced:
  [0] architect (architect) - idle
      ^name      ^role
```

Or when role differs from name:
```
  [0] expert0 (frontend) - idle
```

---

## Workflow Integration

### Tower Startup Flow

```rust
// In TowerApp::new() or initialization

async fn initialize_session_roles(&mut self) -> Result<()> {
    let session_hash = self.config.session_hash();
    
    // Try to load existing session roles
    let mut roles = self.context_store
        .load_session_roles(&session_hash)
        .await?
        .unwrap_or_else(|| SessionExpertRoles::new(session_hash.clone()));
    
    // Initialize any missing experts with defaults from config
    for i in 0..self.config.num_experts() {
        if roles.get_role(i).is_none() {
            let default_role = self.config.get_expert(i)
                .map(|e| e.name.clone())
                .unwrap_or_else(|| "general".to_string());
            roles.set_role(i, default_role);
        }
    }
    
    // Save updated roles
    self.context_store.save_session_roles(&roles).await?;
    self.session_roles = roles;
    
    Ok(())
}
```

### Role Change Flow

```rust
// In TowerApp

pub async fn change_expert_role(&mut self, expert_id: u32, new_role: &str) -> Result<()> {
    // 1. Update session roles
    self.session_roles.set_role(expert_id, new_role.to_string());
    self.context_store.save_session_roles(&self.session_roles).await?;
    
    // 2. Send /clear to expert
    self.claude.send_clear(expert_id).await?;
    
    // 3. Load and send new role instructions
    let instruction = load_instruction_with_template(
        &self.config.instructions_path,
        new_role
    )?;
    
    if !instruction.is_empty() {
        self.claude.send_instruction(expert_id, &instruction).await?;
    }
    
    // 4. Update UI message
    self.set_message(format!(
        "Expert {} role changed to {}",
        expert_id, new_role
    ));
    
    Ok(())
}
```

### Keyboard Shortcuts

| Key | Context | Action |
|-----|---------|--------|
| `r` | Expert List focused | Open role selector for selected expert |
| `Enter` | Role Selector open | Confirm role selection |
| `Esc` | Role Selector open | Cancel role selection |
| `j/↓` | Role Selector open | Move selection down |
| `k/↑` | Role Selector open | Move selection up |

---

## ContextStore Extensions

```rust
// src/context/store.rs additions

impl ContextStore {
    pub async fn load_session_roles(&self, session_hash: &str) -> Result<Option<SessionExpertRoles>> {
        let path = self.session_path(session_hash).join("expert_roles.yaml");
        if !path.exists() {
            return Ok(None);
        }
        let content = tokio::fs::read_to_string(&path).await?;
        let roles: SessionExpertRoles = serde_yaml::from_str(&content)?;
        Ok(Some(roles))
    }
    
    pub async fn save_session_roles(&self, roles: &SessionExpertRoles) -> Result<()> {
        let path = self.session_path(&roles.session_hash).join("expert_roles.yaml");
        let content = serde_yaml::to_string(roles)?;
        tokio::fs::write(&path, content).await?;
        Ok(())
    }
}
```

---

## TowerApp Field Additions

```rust
// src/tower/app.rs

pub struct TowerApp {
    // ... existing fields ...
    
    // New fields for role management
    session_roles: SessionExpertRoles,
    available_roles: AvailableRoles,
    role_selector: RoleSelector,
}
```

---

## File Structure Changes

### New Files

```
src/
├── context/
│   ├── mod.rs          (add: pub mod role)
│   └── role.rs         (NEW: SessionExpertRoles, AvailableRoles)
└── tower/
    └── widgets/
        ├── mod.rs      (add: pub mod role_selector)
        └── role_selector.rs  (NEW: RoleSelector widget)

queue/sessions/{hash}/
└── expert_roles.yaml   (NEW: session role assignments)
```

### Modified Files

| File | Changes |
|------|---------|
| `src/tower/app.rs` | Add session_roles, available_roles, role_selector fields; add change_expert_role method; add role selector key handling |
| `src/tower/widgets/status_display.rs` | Display role alongside expert name |
| `src/context/store.rs` | Add load/save_session_roles methods |
| `src/context/mod.rs` | Export role module |
| `src/tower/widgets/mod.rs` | Export role_selector module |

---

## Implementation Priority

### Phase 1: Core Data Structures
1. Create `src/context/role.rs` with `SessionExpertRoles` and `AvailableRoles`
2. Add ContextStore methods for role persistence
3. Modify TowerApp initialization to load/create session roles

### Phase 2: UI Components
4. Create `RoleSelector` widget
5. Add keyboard shortcut 'r' to open role selector
6. Integrate role selector modal into tower rendering

### Phase 3: Role Change Flow
7. Implement `change_expert_role` method
8. Connect role selector confirmation to role change
9. Update StatusDisplay to show current roles

### Phase 4: Testing
10. Unit tests for SessionExpertRoles
11. Unit tests for RoleSelector widget
12. Integration tests for role change flow

---

## Error Handling

| Scenario | Handling |
|----------|----------|
| Instructions file not found | Fall back to core.md only, show warning |
| Session roles file corrupted | Recreate with defaults, log error |
| Role change during active task | Allow but warn user |
| Invalid role name | Reject selection, show available roles |

---

## Future Considerations

1. **Custom Role Definitions**: Allow users to create custom instruction files
2. **Role Presets**: Save favorite role configurations
3. **Role Capabilities**: Define what actions each role can perform
4. **Role Restrictions**: Limit certain roles to specific experts
