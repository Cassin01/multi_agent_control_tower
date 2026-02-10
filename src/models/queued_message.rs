use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::message::Message;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    #[default]
    Pending,
    Delivering,
    Failed {
        reason: String,
    },
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub message: Message,
    pub attempts: u32,
    pub last_attempt: Option<DateTime<Utc>>,
    pub status: MessageStatus,
}

impl QueuedMessage {
    pub fn new(message: Message) -> Self {
        Self {
            message,
            attempts: 0,
            last_attempt: None,
            status: MessageStatus::default(),
        }
    }

    pub fn mark_delivery_attempt(&mut self) {
        self.attempts += 1;
        self.last_attempt = Some(Utc::now());
        self.status = MessageStatus::Delivering;
    }

    pub fn mark_failed(&mut self, reason: String) {
        self.status = MessageStatus::Failed { reason };
    }

    pub fn mark_expired(&mut self) {
        self.status = MessageStatus::Expired;
    }

    pub fn reset_to_pending(&mut self) {
        self.status = MessageStatus::Pending;
    }

    pub fn is_pending(&self) -> bool {
        matches!(self.status, MessageStatus::Pending)
    }

    #[allow(dead_code)]
    pub fn is_delivering(&self) -> bool {
        matches!(self.status, MessageStatus::Delivering)
    }

    #[allow(dead_code)]
    pub fn is_failed(&self) -> bool {
        matches!(self.status, MessageStatus::Failed { .. })
    }

    #[allow(dead_code)]
    pub fn is_expired(&self) -> bool {
        matches!(self.status, MessageStatus::Expired)
    }

    pub fn should_retry(&self) -> bool {
        self.is_pending() && !self.message.is_expired() && !self.message.has_exceeded_max_attempts()
    }

    #[allow(dead_code)]
    pub fn get_failure_reason(&self) -> Option<&str> {
        match &self.status {
            MessageStatus::Failed { reason } => Some(reason),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::message::{MessageContent, MessageRecipient, MessageType};

    fn create_test_message() -> Message {
        let content = MessageContent {
            subject: "Test Subject".to_string(),
            body: "Test Body".to_string(),
        };
        let recipient = MessageRecipient::expert_id(1);
        Message::new(0, recipient, MessageType::Query, content)
    }

    #[test]
    fn queued_message_new_creates_with_defaults() {
        let message = create_test_message();
        let queued = QueuedMessage::new(message.clone());

        assert_eq!(queued.message.message_id, message.message_id);
        assert_eq!(queued.attempts, 0);
        assert!(queued.last_attempt.is_none());
        assert_eq!(queued.status, MessageStatus::Pending);
    }

    #[test]
    fn queued_message_delivery_attempt_tracking() {
        let message = create_test_message();
        let mut queued = QueuedMessage::new(message);

        assert_eq!(queued.attempts, 0);
        assert!(queued.is_pending());

        queued.mark_delivery_attempt();
        assert_eq!(queued.attempts, 1);
        assert!(queued.last_attempt.is_some());
        assert!(queued.is_delivering());
    }

    #[test]
    fn queued_message_status_transitions() {
        let message = create_test_message();
        let mut queued = QueuedMessage::new(message);

        // Start as pending
        assert!(queued.is_pending());
        assert!(!queued.is_delivering());
        assert!(!queued.is_failed());
        assert!(!queued.is_expired());

        // Mark as delivering
        queued.mark_delivery_attempt();
        assert!(queued.is_delivering());

        // Mark as failed
        queued.mark_failed("Network error".to_string());
        assert!(queued.is_failed());
        assert_eq!(queued.get_failure_reason(), Some("Network error"));

        // Reset to pending
        queued.reset_to_pending();
        assert!(queued.is_pending());
        assert_eq!(queued.get_failure_reason(), None);

        // Mark as expired
        queued.mark_expired();
        assert!(queued.is_expired());
    }

    #[test]
    fn queued_message_should_retry_logic() {
        let message = create_test_message();
        let mut queued = QueuedMessage::new(message);

        // Should retry when pending and not expired
        assert!(queued.should_retry());

        // Should not retry when delivering
        queued.mark_delivery_attempt();
        assert!(!queued.should_retry());

        // Should retry when reset to pending
        queued.reset_to_pending();
        assert!(queued.should_retry());

        // Should not retry when failed
        queued.mark_failed("Error".to_string());
        assert!(!queued.should_retry());

        // Should not retry when expired
        queued.reset_to_pending();
        queued.mark_expired();
        assert!(!queued.should_retry());
    }

    #[test]
    fn queued_message_serializes_to_yaml() {
        let message = create_test_message();
        let mut queued = QueuedMessage::new(message);
        queued.mark_failed("Connection timeout".to_string());

        let yaml = serde_yaml::to_string(&queued).unwrap();
        assert!(yaml.contains("attempts: 0"));
        assert!(yaml.contains("status:"));
        assert!(yaml.contains("reason: Connection timeout"));
    }

    #[test]
    fn queued_message_deserializes_from_yaml() {
        let yaml = r#"
message:
  message_id: "msg-test-001"
  from_expert_id: 0
  to:
    expert_id: 1
  message_type: query
  priority: normal
  created_at: "2024-01-15T10:30:00Z"
  content:
    subject: "Test"
    body: "Test body"
  delivery_attempts: 0
  metadata: {}
attempts: 2
last_attempt: "2024-01-15T10:35:00Z"
status: pending
"#;

        let queued: QueuedMessage = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(queued.message.message_id, "msg-test-001");
        assert_eq!(queued.attempts, 2);
        assert!(queued.is_pending());
    }

    #[test]
    fn message_status_default_is_pending() {
        assert_eq!(MessageStatus::default(), MessageStatus::Pending);
    }
}
