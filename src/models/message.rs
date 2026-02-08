use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Maximum retry attempts before removing message with unknown recipient
pub const MAX_DELIVERY_ATTEMPTS: u32 = 100;

/// Default message TTL in seconds (24 hours)
#[allow(dead_code)]
pub const DEFAULT_MESSAGE_TTL_SECS: u64 = 86400;

/// Unique identifier for messages
pub type MessageId = String;

/// Unique identifier for experts
pub type ExpertId = u32;

/// Target for message delivery
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum MessageRecipient {
    /// Send to specific expert by ID
    ExpertId { expert_id: u32 },
    /// Send to specific expert by name
    ExpertName { expert_name: String },
    /// Send to any idle expert with this role
    Role { role: String },
}

#[allow(dead_code)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    #[default]
    Query,      // Request information
    Response,   // Reply to query
    Notify,     // Information only
    Delegate,   // Task handoff
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MessagePriority {
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub message_id: MessageId,
    pub from_expert_id: ExpertId,
    pub to: MessageRecipient,
    pub message_type: MessageType,
    pub priority: MessagePriority,
    pub created_at: DateTime<Utc>,
    pub content: MessageContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<MessageId>,
    #[serde(default)]
    pub delivery_attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[allow(dead_code)]
impl Message {
    pub fn new(
        from_expert_id: ExpertId,
        to: MessageRecipient,
        message_type: MessageType,
        content: MessageContent,
    ) -> Self {
        let now = Utc::now();
        let message_id = format!("msg-{}", now.format("%Y%m%d-%H%M%S%3f"));
        
        Self {
            message_id,
            from_expert_id,
            to,
            message_type,
            priority: MessagePriority::default(),
            created_at: now,
            content,
            reply_to: None,
            delivery_attempts: 0,
            expires_at: Some(now + chrono::Duration::seconds(DEFAULT_MESSAGE_TTL_SECS as i64)),
            metadata: HashMap::new(),
        }
    }

    pub fn with_priority(mut self, priority: MessagePriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_reply_to(mut self, reply_to: MessageId) -> Self {
        self.reply_to = Some(reply_to);
        self
    }

    pub fn with_ttl_seconds(mut self, ttl_seconds: u64) -> Self {
        self.expires_at = Some(self.created_at + chrono::Duration::seconds(ttl_seconds as i64));
        self
    }

    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }

    pub fn increment_delivery_attempts(&mut self) {
        self.delivery_attempts += 1;
    }

    pub fn has_exceeded_max_attempts(&self) -> bool {
        self.delivery_attempts >= MAX_DELIVERY_ATTEMPTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_new_creates_with_defaults() {
        let content = MessageContent {
            subject: "Test Subject".to_string(),
            body: "Test Body".to_string(),
        };
        let recipient = MessageRecipient::expert_id(1);
        
        let message = Message::new(0, recipient, MessageType::Query, content);

        assert_eq!(message.from_expert_id, 0);
        assert_eq!(message.message_type, MessageType::Query);
        assert_eq!(message.priority, MessagePriority::Normal);
        assert_eq!(message.delivery_attempts, 0);
        assert!(message.message_id.starts_with("msg-"));
        assert!(message.expires_at.is_some());
        assert!(message.reply_to.is_none());
    }

    #[test]
    fn message_recipient_constructors() {
        let by_id = MessageRecipient::expert_id(42);
        let by_name = MessageRecipient::expert_name("backend");
        let by_role = MessageRecipient::role("developer");

        match by_id {
            MessageRecipient::ExpertId { expert_id } => assert_eq!(expert_id, 42),
            _ => panic!("Expected ExpertId variant"),
        }

        match by_name {
            MessageRecipient::ExpertName { expert_name } => assert_eq!(expert_name, "backend"),
            _ => panic!("Expected ExpertName variant"),
        }

        match by_role {
            MessageRecipient::Role { role } => assert_eq!(role, "developer"),
            _ => panic!("Expected Role variant"),
        }
    }

    #[test]
    fn message_builder_methods() {
        let content = MessageContent {
            subject: "Test".to_string(),
            body: "Body".to_string(),
        };
        let recipient = MessageRecipient::expert_id(1);
        
        let message = Message::new(0, recipient, MessageType::Query, content)
            .with_priority(MessagePriority::High)
            .with_reply_to("original-msg-id".to_string())
            .with_ttl_seconds(3600)
            .with_metadata("urgency".to_string(), "high".to_string());

        assert_eq!(message.priority, MessagePriority::High);
        assert_eq!(message.reply_to, Some("original-msg-id".to_string()));
        assert_eq!(message.metadata.get("urgency"), Some(&"high".to_string()));
    }

    #[test]
    fn message_expiration_logic() {
        let content = MessageContent {
            subject: "Test".to_string(),
            body: "Body".to_string(),
        };
        let recipient = MessageRecipient::expert_id(1);
        
        // Message with very short TTL (already expired)
        let expired_message = Message::new(0, recipient.clone(), MessageType::Query, content.clone())
            .with_ttl_seconds(0);
        
        // Message with no expiration
        let mut no_expiry_message = Message::new(0, recipient, MessageType::Query, content);
        no_expiry_message.expires_at = None;

        assert!(expired_message.is_expired());
        assert!(!no_expiry_message.is_expired());
    }

    #[test]
    fn message_delivery_attempts() {
        let content = MessageContent {
            subject: "Test".to_string(),
            body: "Body".to_string(),
        };
        let recipient = MessageRecipient::expert_id(1);
        
        let mut message = Message::new(0, recipient, MessageType::Query, content);
        
        assert_eq!(message.delivery_attempts, 0);
        assert!(!message.has_exceeded_max_attempts());

        message.increment_delivery_attempts();
        assert_eq!(message.delivery_attempts, 1);

        // Set to max attempts
        message.delivery_attempts = MAX_DELIVERY_ATTEMPTS;
        assert!(message.has_exceeded_max_attempts());
    }

    #[test]
    fn message_serializes_to_yaml() {
        let content = MessageContent {
            subject: "API Question".to_string(),
            body: "What format should we use for dates?".to_string(),
        };
        let recipient = MessageRecipient::expert_name("Backend".to_string());
        
        let message = Message::new(0, recipient, MessageType::Query, content)
            .with_priority(MessagePriority::High);

        let yaml = serde_yaml::to_string(&message).unwrap();
        assert!(yaml.contains("message_type: query"));
        assert!(yaml.contains("priority: high"));
        assert!(yaml.contains("from_expert_id: 0"));
        assert!(yaml.contains("expert_name: Backend"));
    }

    #[test]
    fn message_deserializes_from_yaml() {
        let yaml = r#"
message_id: "msg-20240115-103000123"
from_expert_id: 0
to:
  expert_id: 2
message_type: delegate
priority: high
created_at: "2024-01-15T10:30:00.123Z"
content:
  subject: "Implement User API"
  body: "Please implement the user CRUD endpoints."
reply_to: null
delivery_attempts: 0
expires_at: "2024-01-16T10:30:00.123Z"
metadata: {}
"#;

        let message: Message = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(message.message_id, "msg-20240115-103000123");
        assert_eq!(message.from_expert_id, 0);
        assert_eq!(message.message_type, MessageType::Delegate);
        assert_eq!(message.priority, MessagePriority::High);
        assert_eq!(message.content.subject, "Implement User API");
    }

    #[test]
    fn message_priority_ordering() {
        assert!(MessagePriority::High > MessagePriority::Normal);
        assert!(MessagePriority::Normal > MessagePriority::Low);
        assert!(MessagePriority::High > MessagePriority::Low);
    }

    #[test]
    fn message_type_default_is_query() {
        assert_eq!(MessageType::default(), MessageType::Query);
    }

    #[test]
    fn message_priority_default_is_normal() {
        assert_eq!(MessagePriority::default(), MessagePriority::Normal);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // Generators for property-based testing
    fn arbitrary_message_content() -> impl Strategy<Value = MessageContent> {
        (
            "[a-zA-Z0-9 ]{1,100}",
            "[a-zA-Z0-9 \n]{1,1000}",
        ).prop_map(|(subject, body)| MessageContent { subject, body })
    }

    fn arbitrary_message_recipient() -> impl Strategy<Value = MessageRecipient> {
        prop_oneof![
            (0u32..100).prop_map(MessageRecipient::expert_id),
            "[a-zA-Z0-9-]{1,50}".prop_map(MessageRecipient::expert_name),
            "[a-zA-Z0-9-]{1,50}".prop_map(MessageRecipient::role),
        ]
    }

    fn arbitrary_message_type() -> impl Strategy<Value = MessageType> {
        prop_oneof![
            Just(MessageType::Query),
            Just(MessageType::Response),
            Just(MessageType::Notify),
            Just(MessageType::Delegate),
        ]
    }

    fn arbitrary_message_priority() -> impl Strategy<Value = MessagePriority> {
        prop_oneof![
            Just(MessagePriority::Low),
            Just(MessagePriority::Normal),
            Just(MessagePriority::High),
        ]
    }

    fn arbitrary_message() -> impl Strategy<Value = Message> {
        (
            0u32..100,
            arbitrary_message_recipient(),
            arbitrary_message_type(),
            arbitrary_message_content(),
            arbitrary_message_priority(),
            0u64..86400,
        ).prop_map(|(from_expert_id, to, message_type, content, priority, ttl_seconds)| {
            Message::new(from_expert_id, to, message_type, content)
                .with_priority(priority)
                .with_ttl_seconds(ttl_seconds)
        })
    }

    // Feature: inter-expert-messaging, Property 5: Message Persistence Round-Trip
    // **Validates: Requirements 4.1, 4.2, 4.3, 4.4**
    proptest! {
        #[test]
        fn message_persistence_round_trip(
            message in arbitrary_message()
        ) {
            // Serialize message to YAML
            let yaml = serde_yaml::to_string(&message).unwrap();
            
            // Deserialize back from YAML
            let deserialized: Message = serde_yaml::from_str(&yaml).unwrap();
            
            // Verify all fields are preserved
            assert_eq!(message.message_id, deserialized.message_id);
            assert_eq!(message.from_expert_id, deserialized.from_expert_id);
            assert_eq!(message.to, deserialized.to);
            assert_eq!(message.message_type, deserialized.message_type);
            assert_eq!(message.priority, deserialized.priority);
            assert_eq!(message.content.subject, deserialized.content.subject);
            assert_eq!(message.content.body, deserialized.content.body);
            assert_eq!(message.delivery_attempts, deserialized.delivery_attempts);
            assert_eq!(message.metadata, deserialized.metadata);
            
            // Timestamps should be preserved (within reasonable precision)
            assert_eq!(message.created_at.timestamp(), deserialized.created_at.timestamp());
            
            // TTL should be preserved if present
            if let (Some(orig), Some(deser)) = (message.expires_at, deserialized.expires_at) {
                assert_eq!(orig.timestamp(), deser.timestamp());
            } else {
                assert_eq!(message.expires_at.is_some(), deserialized.expires_at.is_some());
            }
        }

        #[test]
        fn message_recipient_serialization_round_trip(
            recipient in arbitrary_message_recipient()
        ) {
            let yaml = serde_yaml::to_string(&recipient).unwrap();
            let deserialized: MessageRecipient = serde_yaml::from_str(&yaml).unwrap();
            assert_eq!(recipient, deserialized);
        }

        #[test]
        fn message_content_serialization_round_trip(
            content in arbitrary_message_content()
        ) {
            let yaml = serde_yaml::to_string(&content).unwrap();
            let deserialized: MessageContent = serde_yaml::from_str(&yaml).unwrap();
            assert_eq!(content.subject, deserialized.subject);
            assert_eq!(content.body, deserialized.body);
        }
    }
}