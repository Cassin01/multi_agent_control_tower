# Inter-Expert Messaging System Design Document

## 1. Overview

### 1.1 Purpose
Enable message-based communication between experts (Claude instances in tmux panes), with the Control Tower acting as a message router. Experts can coordinate, delegate tasks, and share knowledge directly with each other.

**Note:** The Control Tower only routes messages; it does not send messages itself. Task assignment from the Control Tower uses the existing task system (`assign_task()`).

### 1.2 Current State
- `src/models/` contains `effort.rs`, `report.rs`, `task.rs` — **No message model exists yet**
- `src/queue/manager.rs` has task and report operations — **No message queue methods yet**
- Control Tower UI (`src/tower/`) has no messaging components

**This document proposes new components to be implemented.**

### 1.3 Design Principles
| Principle | Description |
|-----------|-------------|
| Simplicity | Single message queue, minimal file structure |
| Idle-only delivery | Messages delivered only when recipient is idle |
| Role-based routing | Send to any idle expert with matching role |
| Non-blocking | Busy experts don't block the queue |

---

## 2. Architecture

### 2.1 High-Level Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                         Control Tower                           │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                  Message Router (Polling)                │   │
│  │                                                          │   │
│  │  1. Read queue                                           │   │
│  │  2. For each message:                                    │   │
│  │     - Role target → Find idle expert with role → Send    │   │
│  │     - Expert target → If idle, send; else skip           │   │
│  │  3. Delete sent messages from queue                      │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ [tmux send-keys]
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      File System (.macot/)                      │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    messages/queue/                       │   │
│  │                    msg-{timestamp}.yaml                  │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              ▲
                              │
        ┌─────────────────────┴─────────────────────┐
        │                                           │
┌───────┴───────┐                         ┌─────────┴─────────┐
│   Expert 0    │                         │     Expert N      │
│   (Architect) │                         │     (Backend)     │
│               │                         │                   │
│ Write to      │                         │ Write to          │
│ queue/        │                         │ queue/            │
└───────────────┘                         └───────────────────┘
```

### 2.2 Message Delivery Flow

```
1. Expert writes message to queue
   └─▶ .macot/messages/queue/msg-{timestamp}.yaml

2. Control Tower polls queue (1s interval)
   └─▶ Reads all messages sorted by created_at

3. For each message, Control Tower routes:

   [If recipient is ROLE]
   ├─▶ Find any idle expert with matching role
   │   ├─▶ Found: Send via tmux → Delete from queue
   │   └─▶ Not found: Skip (retry next poll)
   │
   [If recipient is EXPERT ID/NAME]
   ├─▶ Check if target expert is idle
   │   ├─▶ Idle: Send via tmux → Delete from queue
   │   └─▶ Busy: Skip (retry next poll)

4. Repeat polling
```

---

## 3. Data Models

### 3.1 Message Recipient

```rust
// src/models/message.rs

/// Target for message delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageRecipient {
    /// Send to specific expert by ID
    ExpertId { expert_id: u32 },
    /// Send to specific expert by name
    ExpertName { expert_name: String },
    /// Send to any idle expert with this role
    Role { role: String },
}

impl MessageRecipient {
    pub fn expert_id(id: u32) -> Self {
        Self::ExpertId { expert_id: id }
    }

    pub fn expert_name(name: impl Into<String>) -> Self {
        Self::ExpertName { expert_name: name.into() }
    }

    pub fn role(role: impl Into<String>) -> Self {
        Self::Role { role: role.into() }
    }
}
```

### 3.2 Message Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    #[default]
    Query,      // Request information
    Response,   // Reply to query
    Notify,     // Information only
    Delegate,   // Task handoff
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessagePriority {
    #[default]
    Normal,
    High,
}
```

### 3.3 Constants

```rust
// src/models/message.rs

/// Maximum retry attempts before removing message with unknown recipient
pub const MAX_DELIVERY_ATTEMPTS: u32 = 100;

/// Default message TTL in seconds (24 hours)
pub const DEFAULT_MESSAGE_TTL_SECS: u64 = 86400;
```

### 3.4 Message Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub message_id: String,
    pub from_expert_id: u32,  // Sending expert's ID (0-N)
    pub to: MessageRecipient,
    pub message_type: MessageType,
    pub priority: MessagePriority,
    pub created_at: DateTime<Utc>,
    pub content: MessageContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,  // Original message_id for responses
    #[serde(default)]
    pub delivery_attempts: u32,    // Incremented on each failed delivery
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,  // Message expiration time (TTL)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    pub subject: String,
    pub body: String,
}
```

### 3.5 Message YAML Schema

```yaml
# .macot/messages/queue/msg-20240115-103000123.yaml

# Example 1: Send to specific expert by ID
message_id: "msg-20240115-103000123"
from_expert_id: 0
to:
  expert_id: 2
message_type: delegate
priority: high
created_at: "2024-01-15T10:30:00.123Z"
content:
  subject: "Implement User API"
  body: |
    Please implement the user CRUD endpoints.
    See doc/api-design.md for details.
reply_to: null

---
# Example 2: Send to expert by name
message_id: "msg-20240115-103100456"
from_expert_id: 0
to:
  expert_name: "Backend"
message_type: query
priority: normal
created_at: "2024-01-15T10:31:00.456Z"
content:
  subject: "API Response Format"
  body: "What format should we use for dates?"
reply_to: null

---
# Example 3: Send to any expert with role
message_id: "msg-20240115-103200789"
from_expert_id: 0  # Architect expert
to:
  role: "backend"
message_type: delegate
priority: normal
created_at: "2024-01-15T10:32:00.789Z"
content:
  subject: "Need Backend Help"
  body: "Please implement the authentication middleware."
reply_to: null
```

---

## 4. File System Structure

```
.macot/
├── messages/
│   └── queue/                  # Single message queue
│       ├── msg-20240115-103000123.yaml
│       ├── msg-20240115-103100456.yaml
│       └── msg-20240115-103200789.yaml
│
├── tasks/
├── reports/
└── sessions/
```

---

## 5. Implementation

### 5.1 QueueManager

```rust
// src/queue/manager.rs

impl QueueManager {
    fn messages_path(&self) -> PathBuf {
        self.base_path.join("messages")
    }

    fn queue_path(&self) -> PathBuf {
        self.messages_path().join("queue")
    }

    /// Initialize queue directory
    pub async fn init_message_queue(&self) -> Result<()> {
        fs::create_dir_all(self.queue_path()).await?;
        Ok(())
    }

    /// Add message to queue
    pub async fn enqueue(&self, message: &Message) -> Result<()> {
        let path = self.queue_path()
            .join(format!("{}.yaml", message.message_id));
        let yaml = serde_yaml::to_string(message)?;
        fs::write(&path, yaml).await?;
        Ok(())
    }

    /// Read all queued messages (sorted by created_at)
    pub async fn read_queue(&self) -> Result<Vec<Message>> {
        let mut messages = Vec::new();
        let queue = self.queue_path();

        if !queue.exists() {
            return Ok(messages);
        }

        let mut entries = fs::read_dir(&queue).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "yaml") {
                if let Ok(content) = fs::read_to_string(&path).await {
                    if let Ok(msg) = serde_yaml::from_str::<Message>(&content) {
                        messages.push(msg);
                    }
                }
            }
        }

        // Sort by created_at (oldest first)
        messages.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(messages)
    }

    /// Remove message from queue
    pub async fn dequeue(&self, message_id: &str) -> Result<()> {
        let path = self.queue_path().join(format!("{}.yaml", message_id));
        if path.exists() {
            fs::remove_file(path).await?;
        }
        Ok(())
    }

    /// Count messages in queue
    pub async fn queue_len(&self) -> Result<usize> {
        Ok(self.read_queue().await?.len())
    }

    /// Update delivery attempts counter for a message
    pub async fn update_delivery_attempts(&self, message_id: &str, attempts: u32) -> Result<()> {
        let path = self.queue_path().join(format!("{}.yaml", message_id));
        if !path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&path).await?;
        let mut message: Message = serde_yaml::from_str(&content)?;
        message.delivery_attempts = attempts;

        let yaml = serde_yaml::to_string(&message)?;
        fs::write(&path, yaml).await?;
        Ok(())
    }
}
```

### 5.2 TowerApp Message Router

```rust
// src/tower/app.rs

impl TowerApp {
    /// Poll and route messages (called from main loop)
    pub async fn poll_messages(&mut self) -> Result<()> {
        const POLL_INTERVAL: Duration = Duration::from_millis(1000);

        if self.last_message_poll.elapsed() < POLL_INTERVAL {
            return Ok(());
        }
        self.last_message_poll = Instant::now();

        let messages = self.queue.read_queue().await?;

        for message in messages {
            if self.try_deliver(&message).await? {
                self.queue.dequeue(&message.message_id).await?;
            }
            // If not delivered, message stays in queue for next poll
        }

        Ok(())
    }

    /// Try to deliver message. Returns true if delivered.
    async fn try_deliver(&mut self, message: &Message) -> Result<bool> {
        match &message.to {
            MessageRecipient::ExpertId { expert_id } => {
                self.try_deliver_to_expert(*expert_id, message).await
            }
            MessageRecipient::ExpertName { expert_name } => {
                if let Some(expert_id) = self.find_expert_by_name(expert_name) {
                    self.try_deliver_to_expert(expert_id, message).await
                } else {
                    // Unknown expert name - increment attempts and check threshold
                    tracing::warn!("Unknown expert name: {}", expert_name);
                    self.handle_undeliverable(message).await
                }
            }
            MessageRecipient::Role { role } => {
                self.try_deliver_to_role(role, message).await
            }
        }
    }

    /// Try to deliver to specific expert. Returns true if delivered.
    ///
    /// Note: There is a minor race condition between status check and send.
    /// If expert becomes busy between check and send, message may be delivered
    /// to a non-idle expert. This is acceptable as the message will still be
    /// visible to the expert when they return to idle state.
    async fn try_deliver_to_expert(
        &mut self,
        expert_id: u32,
        message: &Message,
    ) -> Result<bool> {
        let status = self.capture.get_expert_status(expert_id);

        if status != ExpertStatus::Idle {
            // Expert is busy - skip, retry next poll
            return Ok(false);
        }

        self.send_to_expert(expert_id, message).await?;
        Ok(true)
    }

    /// Try to deliver to any idle expert with matching role. Returns true if delivered.
    async fn try_deliver_to_role(
        &mut self,
        role: &str,
        message: &Message,
    ) -> Result<bool> {
        // Find first idle expert with matching role
        for expert_id in 0..self.config.num_experts() {
            let expert_role = self.config.get_expert_role(expert_id);
            let status = self.capture.get_expert_status(expert_id);

            if expert_role.eq_ignore_ascii_case(role) && status == ExpertStatus::Idle {
                self.send_to_expert(expert_id, message).await?;
                return Ok(true);
            }
        }

        // No idle expert with this role - skip, retry next poll
        Ok(false)
    }

    /// Send message to expert via tmux
    async fn send_to_expert(&mut self, expert_id: u32, message: &Message) -> Result<()> {
        let sender_name = self.get_expert_name(message.from_expert_id);
        let notification = format!(
            "New message from {} (Expert {}).\n\
             Type: {:?} | Priority: {:?}\n\
             Subject: {}\n\n\
             {}",
            sender_name,
            message.from_expert_id,
            message.message_type,
            message.priority,
            message.content.subject,
            message.content.body
        );

        self.claude.send_keys_with_enter(expert_id, &notification).await?;
        Ok(())
    }

    /// Find expert ID by name
    fn find_expert_by_name(&self, name: &str) -> Option<u32> {
        for expert_id in 0..self.config.num_experts() {
            let expert_name = self.config.get_expert_name(expert_id);
            if expert_name.eq_ignore_ascii_case(name) {
                return Some(expert_id);
            }
        }
        None
    }

    /// Handle undeliverable message (unknown recipient or expired)
    /// Returns Ok(true) to remove message, Ok(false) to keep in queue
    async fn handle_undeliverable(&mut self, message: &Message) -> Result<bool> {
        // Check TTL expiration
        if let Some(expires_at) = message.expires_at {
            if Utc::now() > expires_at {
                tracing::info!("Message {} expired, removing from queue", message.message_id);
                return Ok(true);  // Remove expired message
            }
        }

        // Increment delivery attempts (requires mutable message or queue update)
        let attempts = message.delivery_attempts + 1;
        if attempts >= MAX_DELIVERY_ATTEMPTS {
            tracing::warn!(
                "Message {} exceeded max delivery attempts ({}), removing from queue",
                message.message_id,
                MAX_DELIVERY_ATTEMPTS
            );
            return Ok(true);  // Remove after max attempts
        }

        // Update attempts count in queue file
        self.queue.update_delivery_attempts(&message.message_id, attempts).await?;
        Ok(false)  // Keep in queue for retry
    }

}
```

---

## 6. UI Components

### 6.1 Message Panel (Display Only)

The Control Tower displays queued messages for monitoring purposes only. It does not send messages.

```
┌─────────────────────────────────────────────────────────────────┐
│ Messages [2 queued]                                              │
├─────────────────────────────────────────────────────────────────┤
│ ? [0→2] API Question                                2m ago  High │
│ D [0→role:backend] Need Backend Help                5m ago       │
└─────────────────────────────────────────────────────────────────┘
```

**Legend:**
- `?` Query | `!` Notify | `R` Response | `D` Delegate
- `[0→2]` From Expert 0 to Expert 2
- `[0→role:backend]` From Expert 0 to any expert with role "backend"

### 6.2 Keyboard Shortcuts

| Context | Key | Action |
|---------|-----|--------|
| Message Panel | `j` / `↓` | Next message |
| Message Panel | `k` / `↑` | Previous message |
| Message Panel | `d` | Delete message from queue |

### 6.3 Separation of Concerns

| Channel | Purpose | Used By |
|---------|---------|---------|
| Task System (`assign_task()`) | Formal task assignment with tracking | Control Tower → Expert |
| Message Queue | Expert coordination and communication | Expert ↔ Expert |

**Design Rationale:**
- Control Tower assigns tasks via the existing task system with effort levels and reporting
- Experts communicate with each other via messages for coordination
- This separation keeps task tracking formal while allowing flexible expert communication

---

## 7. Expert Instructions

Add to `instructions/core.md`:

```markdown
## Inter-Expert Communication

### Overview
MACOT enables message-based communication between experts via the Control Tower.
Messages are delivered automatically when the recipient is idle.

### Message Queue Location
- `.macot/messages/queue/` - All outgoing messages

### Sending Messages
Write a YAML file to the queue directory:

```yaml
message_id: "msg-{YYYYMMDD-HHMMSSmmm}"
from_expert_id: {your_id}
to:
  # Option 1: Send to specific expert by ID
  expert_id: 2

  # Option 2: Send to specific expert by name
  # expert_name: "Backend"

  # Option 3: Send to any idle expert with role
  # role: "backend"

message_type: query  # query|response|notify|delegate
priority: normal     # normal|high
created_at: "{ISO8601_timestamp}"
content:
  subject: "Brief subject"
  body: |
    Detailed message content.
reply_to: null  # Set to original message_id for responses
```

The Control Tower will route your message automatically.

### Message Types

| Type | Purpose | Expected Action |
|------|---------|-----------------|
| Query | Request information | Respond with Response type |
| Response | Reply to query | Process the information |
| Notify | Information only | Acknowledge if needed |
| Delegate | Task handoff | Accept and begin work |

### Response Protocol
When responding to a Query:
1. Note the original `message_id`
2. Create Response with `reply_to` set to original ID
3. Write to queue directory
```

---

## 8. Implementation Phases

### Phase 1: Core Infrastructure
- [ ] Create `src/models/message.rs` module
- [ ] Define constants: `MAX_DELIVERY_ATTEMPTS`, `DEFAULT_MESSAGE_TTL_SECS`
- [ ] Implement `MessageRecipient` enum with three variants
- [ ] Implement `Message` struct with TTL and delivery_attempts fields
- [ ] Add message module to `src/models/mod.rs`
- [ ] Implement `QueueManager` message queue methods (enqueue, read_queue, dequeue, update_delivery_attempts)
- [ ] Add `messages_path()` helper to QueueManager
- [ ] Initialize queue directory in `init()`
- [ ] Write unit tests for queue operations and TTL handling

### Phase 2: Message Router
- [ ] Add `last_message_poll: Instant` field to TowerApp
- [ ] Implement `poll_messages()` in TowerApp
- [ ] Implement `try_deliver()` routing logic
- [ ] Implement `try_deliver_to_expert()` with race condition documentation
- [ ] Implement `try_deliver_to_role()`
- [ ] Implement `handle_undeliverable()` for TTL and max attempts handling
- [ ] Implement `send_to_expert()` via tmux
- [ ] Implement `find_expert_by_name()`
- [ ] Add polling to main event loop

### Phase 3: UI Components
- [ ] Create `MessagePanel` widget (display-only, for monitoring)
- [ ] Add keyboard handlers for message list navigation
- [ ] Update UI layout to include message panel

### Phase 4: Expert Integration
- [ ] Update `core.md` instructions
- [ ] Test expert message sending
- [ ] Test message receiving

---

## 9. Verification Plan

### 9.1 Unit Tests
```bash
cargo test
```
- Queue enqueue/dequeue operations
- Message serialization/deserialization
- Recipient type parsing
- TTL expiration detection
- Delivery attempts counter increment

### 9.2 Manual Testing Checklist

| Test | Expected Result |
|------|-----------------|
| Start session | Queue directory created |
| Expert sends to idle expert (by ID) | Message delivered immediately |
| Expert sends to busy expert (by ID) | Message stays in queue |
| Busy expert becomes idle | Message delivered on next poll |
| Expert sends to expert (by name) | Message delivered to named expert |
| Expert sends to unknown expert name | Message removed after MAX_DELIVERY_ATTEMPTS |
| Expert sends to role (idle exists) | Message delivered to one idle expert |
| Expert sends to role (all busy) | Message stays in queue |
| Message TTL expires | Message removed from queue |
| Tower displays queued messages | Message panel shows pending messages |

### 9.3 Integration Test Scenario

```
1. Start session: macot start
2. Open tower: macot tower
3. Expert 0 (Architect) is Idle, Expert 2 (Backend) is Busy
4. Expert 0 writes message to queue targeting role:backend
5. Expert 2 is busy: message stays in queue, visible in tower's message panel
6. Expert 2 becomes Idle
7. Next poll: message delivered to Expert 2
8. Expert 2 receives message and can respond
```

---

## 10. Future Enhancements

- Message priority queue (High priority first)
- Message expiration (TTL)
- Delivery confirmation
- Message history/archive
- Reply threading
