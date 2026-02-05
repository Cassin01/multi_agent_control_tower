# Inter-Expert Messaging System

This document provides instructions for using the MACOT inter-expert messaging system, enabling communication between experts (Claude instances) through the Control Tower.

## Overview

The messaging system allows experts to:
- Query other experts for information
- Respond to queries from other experts
- Send notifications about important events
- Delegate tasks to other experts

The Control Tower acts as a message router, delivering messages only when recipients are idle (non-blocking delivery).

## Message Queue Location

All outgoing messages should be written to:
```
.macot/messages/outbox/
```

The Control Tower monitors this directory and processes messages automatically.

## Message Format

Messages must be written as YAML files with the following schema:

```yaml
message_id: "msg-YYYYMMDD-HHMMSSmmm"  # Unique identifier
from_expert_id: 0                      # Your expert ID (0-N)
to:                                    # Recipient targeting (see below)
  expert_id: 2
message_type: query                    # query | response | notify | delegate
priority: normal                       # low | normal | high
created_at: "2024-01-15T10:30:00Z"    # ISO 8601 timestamp
content:
  subject: "Brief subject line"
  body: |
    Detailed message content.
    Can span multiple lines.
reply_to: null                         # Set to original message_id for responses
```

## Recipient Targeting Methods

### 1. By Expert ID (Direct)

Send to a specific expert using their numeric ID:

```yaml
to:
  expert_id: 2
```

### 2. By Expert Name

Send to an expert by their configured name (case-insensitive):

```yaml
to:
  expert_name: "Backend"
```

### 3. By Role (Any Idle Expert)

Send to any idle expert with a matching role:

```yaml
to:
  role: "backend"
```

The Control Tower will deliver to the first idle expert with that role.

## Message Types

### Query

Request information from another expert:

```yaml
message_id: "msg-20240115-103000001"
from_expert_id: 0
to:
  expert_name: "Backend"
message_type: query
priority: normal
created_at: "2024-01-15T10:30:00Z"
content:
  subject: "API Response Format"
  body: |
    What format should we use for date fields in the API responses?
    Options: ISO 8601 string or Unix timestamp?
reply_to: null
```

### Response

Reply to a previous query (include `reply_to`):

```yaml
message_id: "msg-20240115-103500002"
from_expert_id: 2
to:
  expert_id: 0
message_type: response
priority: normal
created_at: "2024-01-15T10:35:00Z"
content:
  subject: "RE: API Response Format"
  body: |
    I recommend ISO 8601 format (e.g., "2024-01-15T10:30:00Z") for the following reasons:
    1. Human readable
    2. Timezone aware
    3. Standard across languages
reply_to: "msg-20240115-103000001"
```

### Notify

Send information without expecting a response:

```yaml
message_id: "msg-20240115-110000003"
from_expert_id: 1
to:
  role: "backend"
message_type: notify
priority: high
created_at: "2024-01-15T11:00:00Z"
content:
  subject: "API Schema Updated"
  body: |
    I've updated the API schema in doc/api-design.md.
    Key changes:
    - Added pagination parameters
    - New rate limiting headers
    Please review when convenient.
reply_to: null
```

### Delegate

Hand off a task to another expert:

```yaml
message_id: "msg-20240115-113000004"
from_expert_id: 0
to:
  role: "backend"
message_type: delegate
priority: high
created_at: "2024-01-15T11:30:00Z"
content:
  subject: "Implement User API Endpoints"
  body: |
    Please implement the user CRUD endpoints as specified in doc/api-design.md.

    Requirements:
    - GET /users - List users with pagination
    - GET /users/:id - Get single user
    - POST /users - Create user
    - PUT /users/:id - Update user
    - DELETE /users/:id - Delete user

    All endpoints should use the JSON format discussed earlier.
reply_to: null
```

## Priority Levels

| Priority | Usage | Delivery Order |
|----------|-------|----------------|
| `high` | Urgent matters, blockers | Delivered first |
| `normal` | Standard communication | Default priority |
| `low` | Non-urgent information | Delivered last |

Messages are processed in order: high priority first, then by creation timestamp (FIFO).

## Message Lifecycle

1. **Pending**: Message is in the queue waiting for delivery
2. **Delivering**: Message delivery is in progress
3. **Delivered**: Message successfully sent to recipient (removed from queue)
4. **Failed**: Delivery failed (will retry)
5. **Expired**: Message exceeded TTL (removed from queue)

## TTL (Time-to-Live)

Messages have a default TTL of 24 hours. After expiration, undelivered messages are removed from the queue. This prevents stale messages from being delivered.

## Best Practices

### Writing Messages

1. **Clear Subject Lines**: Use descriptive subjects that summarize the message intent
2. **Structured Body**: Break down complex information into bullet points or numbered lists
3. **Appropriate Priority**: Only use `high` priority for truly urgent matters
4. **Relevant Targeting**: Use role-based targeting when any expert can help

### Responding to Messages

1. **Include reply_to**: Always set `reply_to` when responding to a query
2. **Timely Responses**: Address queries promptly to avoid blocking other experts
3. **Complete Information**: Provide all necessary context in your response

### File Naming

Name your message files using the message ID:
```
.macot/messages/outbox/msg-20240115-103000001.yaml
```

## Troubleshooting

### Message Not Delivered

**Symptoms**: Message stays in queue for extended period

**Possible Causes**:
1. Recipient expert is busy (not idle)
2. Invalid recipient targeting (expert doesn't exist)
3. YAML syntax error in message file

**Solutions**:
1. Wait for recipient to become idle
2. Verify expert ID/name/role exists in the system
3. Check YAML syntax with a validator

### Response Not Received

**Symptoms**: Sent a query but no response received

**Possible Causes**:
1. Recipient hasn't seen the message yet
2. Query was unclear
3. Recipient is working on other tasks

**Solutions**:
1. Check if your message was delivered (not in queue)
2. Resend with clearer subject/body
3. Consider using `high` priority if urgent

### Message Parsing Error

**Symptoms**: Error message about parsing failure

**Possible Causes**:
1. Invalid YAML syntax
2. Missing required fields
3. Incorrect data types

**Solutions**:
1. Validate YAML syntax
2. Ensure all required fields are present:
   - `message_id`
   - `from_expert_id`
   - `to`
   - `message_type`
   - `created_at`
   - `content.subject`
   - `content.body`
3. Use correct types (numbers for IDs, strings for text)

### Duplicate Message IDs

**Symptoms**: Only one of multiple messages appears

**Possible Causes**:
1. Same message ID used for different messages

**Solutions**:
1. Always use unique message IDs
2. Include timestamp with milliseconds in the ID
3. Format: `msg-YYYYMMDD-HHMMSSmmm`

## Message Delivery vs Task Assignment

| Channel | Purpose | Direction |
|---------|---------|-----------|
| Message Queue | Expert coordination | Expert <-> Expert |
| Task System | Formal task assignment | Control Tower -> Expert |

Use messages for informal communication and coordination between experts. Formal tasks with effort levels and tracking come from the Control Tower through the task system.
