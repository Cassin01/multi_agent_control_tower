use anyhow::{Context, Result};
use std::path::PathBuf;
use thiserror::Error;
use tokio::fs;

use crate::models::{Message, MessageId, QueuedMessage, Report};

/// Comprehensive error types for message queue operations
///
/// These error types provide detailed information for debugging and
/// support error isolation to prevent cascading failures.
#[derive(Debug, Error)]
pub enum QueueError {
    /// Error reading or writing to the file system
    #[error("I/O error: {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Error parsing YAML content
    #[error("YAML parsing error for {file_type} at {path}: {details}")]
    YamlParsing {
        file_type: &'static str,
        path: String,
        details: String,
    },

    /// Error serializing data to YAML
    #[error("YAML serialization error for {file_type}: {details}")]
    YamlSerialization {
        file_type: &'static str,
        details: String,
    },

    /// Message validation error
    #[error("Message validation error: {field}: {reason}")]
    Validation {
        field: &'static str,
        reason: String,
    },

    /// Message not found in queue
    #[error("Message not found: {message_id}")]
    MessageNotFound { message_id: String },

    /// Queue directory not accessible
    #[error("Queue directory error: {path}: {reason}")]
    QueueDirectory {
        path: String,
        reason: String,
    },

    /// Atomic write operation failed
    #[error("Atomic write failed for {path}: {reason}")]
    AtomicWrite {
        path: String,
        reason: String,
    },
}

impl QueueError {
    /// Create an I/O error with context
    pub fn io(operation: &'static str, path: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }

    /// Create a YAML parsing error
    pub fn yaml_parsing(file_type: &'static str, path: impl Into<String>, details: impl Into<String>) -> Self {
        Self::YamlParsing {
            file_type,
            path: path.into(),
            details: details.into(),
        }
    }

    /// Create a validation error
    pub fn validation(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Validation {
            field,
            reason: reason.into(),
        }
    }
}

/// Result type alias for queue operations
pub type QueueResult<T> = std::result::Result<T, QueueError>;

pub struct QueueManager {
    base_path: PathBuf,
}

impl QueueManager {
    pub fn new(queue_path: PathBuf) -> Self {
        Self {
            base_path: queue_path,
        }
    }

    fn reports_path(&self) -> PathBuf {
        self.base_path.join("reports")
    }

    fn messages_path(&self) -> PathBuf {
        self.base_path.join("messages")
    }

    fn queue_path(&self) -> PathBuf {
        self.messages_path().join("queue")
    }

    fn outbox_path(&self) -> PathBuf {
        self.messages_path().join("outbox")
    }

    #[allow(dead_code)]
    fn report_file(&self, expert_id: u32) -> PathBuf {
        self.reports_path()
            .join(format!("expert{}_report.yaml", expert_id))
    }

    fn message_file(&self, message_id: &str) -> PathBuf {
        self.queue_path().join(format!("{}.yaml", message_id))
    }

    pub async fn init(&self) -> Result<()> {
        fs::create_dir_all(self.reports_path()).await?;
        self.init_message_queue().await?;
        Ok(())
    }

    /// Initialize message queue directory
    pub async fn init_message_queue(&self) -> Result<()> {
        fs::create_dir_all(self.queue_path()).await?;
        fs::create_dir_all(self.outbox_path()).await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn write_report(&self, report: &Report) -> Result<()> {
        let path = self.report_file(report.expert_id);
        let content = serde_yaml::to_string(report)?;
        fs::write(&path, content)
            .await
            .context("Failed to write report file")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn read_report(&self, expert_id: u32) -> Result<Option<Report>> {
        let path = self.report_file(expert_id);

        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .await
            .context("Failed to read report file")?;
        let report: Report = serde_yaml::from_str(&content)?;
        Ok(Some(report))
    }

    #[allow(dead_code)]
    pub async fn clear_report(&self, expert_id: u32) -> Result<()> {
        let path = self.report_file(expert_id);
        if path.exists() {
            fs::remove_file(&path).await?;
        }
        Ok(())
    }

    pub async fn list_reports(&self) -> Result<Vec<Report>> {
        let mut reports = Vec::new();
        let reports_path = self.reports_path();

        if !reports_path.exists() {
            return Ok(reports);
        }

        let mut entries = fs::read_dir(&reports_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "yaml") {
                match fs::read_to_string(&path).await {
                    Ok(content) => match serde_yaml::from_str::<Report>(&content) {
                        Ok(report) => {
                            if let Err(validation_errors) = report.validate() {
                                tracing::warn!(
                                    "Report {} has validation warnings: {:?}",
                                    path.display(),
                                    validation_errors
                                );
                            }
                            reports.push(report);
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to parse report file {}: {}",
                                path.display(),
                                e
                            );
                        }
                    },
                    Err(e) => {
                        tracing::error!(
                            "Failed to read report file {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }

        reports.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        Ok(reports)
    }

    /// Add message to queue
    pub async fn enqueue(&self, message: &Message) -> Result<()> {
        let queued_message = QueuedMessage::new(message.clone());
        let path = self.message_file(&message.message_id);
        let yaml = serde_yaml::to_string(&queued_message)
            .context("Failed to serialize message to YAML")?;
        
        // Atomic write: write to temp file first, then rename
        let temp_path = path.with_extension("yaml.tmp");
        fs::write(&temp_path, yaml)
            .await
            .context("Failed to write message to temp file")?;
        fs::rename(&temp_path, &path)
            .await
            .context("Failed to atomically move message file")?;
        
        tracing::debug!("Enqueued message {} to queue", message.message_id);
        Ok(())
    }

    /// Read all queued messages (sorted by created_at, then by priority)
    pub async fn read_queue(&self) -> Result<Vec<QueuedMessage>> {
        let mut messages = Vec::new();
        let queue = self.queue_path();

        if !queue.exists() {
            return Ok(messages);
        }

        let mut entries = fs::read_dir(&queue).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "yaml") {
                match fs::read_to_string(&path).await {
                    Ok(content) => match serde_yaml::from_str::<QueuedMessage>(&content) {
                        Ok(mut queued_msg) => {
                            // Check if message is expired and mark it
                            if queued_msg.message.is_expired() {
                                queued_msg.mark_expired();
                            }
                            messages.push(queued_msg);
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to parse message file {}: {}",
                                path.display(),
                                e
                            );
                        }
                    },
                    Err(e) => {
                        tracing::error!(
                            "Failed to read message file {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }

        // Sort by priority (high first), then by created_at (oldest first)
        messages.sort_by(|a, b| {
            b.message.priority.cmp(&a.message.priority)
                .then_with(|| a.message.created_at.cmp(&b.message.created_at))
        });
        
        Ok(messages)
    }

    /// Remove message from queue
    pub async fn dequeue(&self, message_id: &str) -> Result<()> {
        let path = self.message_file(message_id);
        if path.exists() {
            fs::remove_file(&path)
                .await
                .context("Failed to remove message file")?;
            tracing::debug!("Dequeued message {} from queue", message_id);
        }
        Ok(())
    }

    /// Count messages in queue
    pub async fn queue_len(&self) -> Result<usize> {
        Ok(self.read_queue().await?.len())
    }

    /// Update delivery attempts counter for a message
    pub async fn update_delivery_attempts(&self, message_id: &str, attempts: u32) -> Result<()> {
        let path = self.message_file(message_id);
        if !path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&path)
            .await
            .context("Failed to read message file for update")?;
        let mut queued_message: QueuedMessage = serde_yaml::from_str(&content)
            .context("Failed to parse message file for update")?;
        
        queued_message.attempts = attempts;
        queued_message.message.delivery_attempts = attempts;

        let yaml = serde_yaml::to_string(&queued_message)
            .context("Failed to serialize updated message")?;
        
        // Atomic write
        let temp_path = path.with_extension("yaml.tmp");
        fs::write(&temp_path, yaml)
            .await
            .context("Failed to write updated message to temp file")?;
        fs::rename(&temp_path, &path)
            .await
            .context("Failed to atomically update message file")?;
        
        tracing::debug!("Updated delivery attempts for message {} to {}", message_id, attempts);
        Ok(())
    }

    /// Update message status
    pub async fn update_message_status(&self, message_id: &str, queued_message: &QueuedMessage) -> Result<()> {
        let path = self.message_file(message_id);
        if !path.exists() {
            return Ok(());
        }

        let yaml = serde_yaml::to_string(queued_message)
            .context("Failed to serialize message status update")?;
        
        // Atomic write
        let temp_path = path.with_extension("yaml.tmp");
        fs::write(&temp_path, yaml)
            .await
            .context("Failed to write message status to temp file")?;
        fs::rename(&temp_path, &path)
            .await
            .context("Failed to atomically update message status")?;
        
        tracing::debug!("Updated status for message {}", message_id);
        Ok(())
    }

    /// Clean up expired messages and messages that exceeded max attempts
    pub async fn cleanup_expired_messages(&self) -> Result<Vec<MessageId>> {
        let messages = self.read_queue().await?;
        let mut removed_messages = Vec::new();

        for queued_msg in messages {
            let should_remove = if queued_msg.message.is_expired() {
                tracing::info!("Removing expired message: {}", queued_msg.message.message_id);
                true
            } else if queued_msg.message.has_exceeded_max_attempts() {
                tracing::warn!(
                    "Removing message {} after {} delivery attempts",
                    queued_msg.message.message_id,
                    queued_msg.message.delivery_attempts
                );
                true
            } else {
                false
            };

            if should_remove {
                self.dequeue(&queued_msg.message.message_id).await?;
                removed_messages.push(queued_msg.message.message_id);
            }
        }

        if !removed_messages.is_empty() {
            tracing::info!("Cleaned up {} expired/failed messages", removed_messages.len());
        }

        Ok(removed_messages)
    }

    /// Get pending messages (not expired, not exceeded max attempts)
    pub async fn get_pending_messages(&self) -> Result<Vec<QueuedMessage>> {
        let messages = self.read_queue().await?;
        Ok(messages.into_iter()
            .filter(|msg| msg.should_retry())
            .collect())
    }

    /// Process outbox directory and move valid messages to queue
    pub async fn process_outbox(&self) -> Result<Vec<MessageId>> {
        let mut processed_messages = Vec::new();
        let outbox = self.outbox_path();

        if !outbox.exists() {
            return Ok(processed_messages);
        }

        let mut entries = fs::read_dir(&outbox).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "yaml") {
                match self.process_outbox_file(&path).await {
                    Ok(message_id) => {
                        processed_messages.push(message_id);
                        // Remove the processed file from outbox
                        if let Err(e) = fs::remove_file(&path).await {
                            tracing::warn!("Failed to remove processed outbox file {}: {}", path.display(), e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to process outbox file {}: {}", path.display(), e);
                    }
                }
            }
        }

        if !processed_messages.is_empty() {
            tracing::info!("Processed {} messages from outbox", processed_messages.len());
        }

        Ok(processed_messages)
    }

    /// Process a single outbox file
    async fn process_outbox_file(&self, file_path: &std::path::Path) -> Result<MessageId> {
        let content = fs::read_to_string(file_path)
            .await
            .context("Failed to read outbox file")?;
        
        let message: Message = serde_yaml::from_str(&content)
            .context("Failed to parse message YAML from outbox")?;
        
        // Validate required fields are present
        self.validate_message(&message)?;
        
        // Enqueue the message
        self.enqueue(&message).await?;
        
        tracing::debug!("Processed outbox message: {}", message.message_id);
        Ok(message.message_id)
    }

    /// Validate that a message has all required fields
    fn validate_message(&self, message: &Message) -> Result<()> {
        if message.message_id.is_empty() {
            return Err(anyhow::anyhow!("Message ID is required"));
        }
        
        if message.content.subject.is_empty() {
            return Err(anyhow::anyhow!("Message subject is required"));
        }
        
        if message.content.body.is_empty() {
            return Err(anyhow::anyhow!("Message body is required"));
        }
        
        // Additional validation can be added here
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn cleanup(&self) -> Result<()> {
        if self.reports_path().exists() {
            fs::remove_dir_all(self.reports_path()).await?;
        }
        if self.messages_path().exists() {
            fs::remove_dir_all(self.messages_path()).await?;
        }
        self.init().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TaskStatus;
    use tempfile::TempDir;

    async fn create_test_manager() -> (QueueManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let manager = QueueManager::new(temp_dir.path().to_path_buf());
        manager.init().await.unwrap();
        (manager, temp_dir)
    }

    #[tokio::test]
    async fn queue_manager_init_creates_directories() {
        let (manager, _temp) = create_test_manager().await;
        assert!(manager.reports_path().exists());
    }

    #[tokio::test]
    async fn queue_manager_write_and_read_report() {
        let (manager, _temp) = create_test_manager().await;

        let report = Report::new("task-001".to_string(), 0, "architect".to_string())
            .complete("Done".to_string());
        manager.write_report(&report).await.unwrap();

        let loaded = manager.read_report(0).await.unwrap();
        assert!(loaded.is_some());

        let loaded = loaded.unwrap();
        assert_eq!(loaded.task_id, "task-001");
        assert_eq!(loaded.status, TaskStatus::Done);
    }

    #[tokio::test]
    async fn queue_manager_list_reports_returns_all() {
        let (manager, _temp) = create_test_manager().await;

        let report1 = Report::new("task-001".to_string(), 0, "architect".to_string());
        let report2 = Report::new("task-002".to_string(), 1, "frontend".to_string());

        manager.write_report(&report1).await.unwrap();
        manager.write_report(&report2).await.unwrap();

        let reports = manager.list_reports().await.unwrap();
        assert_eq!(reports.len(), 2);
    }

    #[tokio::test]
    async fn queue_manager_cleanup_removes_all() {
        let (manager, _temp) = create_test_manager().await;

        let report = Report::new("task-001".to_string(), 0, "architect".to_string());
        manager.write_report(&report).await.unwrap();

        manager.cleanup().await.unwrap();

        let reports = manager.list_reports().await.unwrap();
        assert!(reports.is_empty());
    }

    // Message queue tests
    use crate::models::{MessageContent, MessageRecipient, MessageType, MessagePriority};

    fn create_test_message() -> Message {
        let content = MessageContent {
            subject: "Test Subject".to_string(),
            body: "Test Body".to_string(),
        };
        let recipient = MessageRecipient::expert_id(1);
        Message::new(0, recipient, MessageType::Query, content)
    }

    #[tokio::test]
    async fn queue_manager_init_creates_message_directories() {
        let (manager, _temp) = create_test_manager().await;
        assert!(manager.messages_path().exists());
        assert!(manager.queue_path().exists());
        assert!(manager.outbox_path().exists());
    }

    #[tokio::test]
    async fn queue_manager_enqueue_and_read_message() {
        let (manager, _temp) = create_test_manager().await;

        let message = create_test_message();
        manager.enqueue(&message).await.unwrap();

        let messages = manager.read_queue().await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message.message_id, message.message_id);
        assert_eq!(messages[0].message.content.subject, "Test Subject");
    }

    #[tokio::test]
    async fn queue_manager_dequeue_removes_message() {
        let (manager, _temp) = create_test_manager().await;

        let message = create_test_message();
        manager.enqueue(&message).await.unwrap();
        assert_eq!(manager.queue_len().await.unwrap(), 1);

        manager.dequeue(&message.message_id).await.unwrap();
        assert_eq!(manager.queue_len().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn queue_manager_priority_ordering() {
        let (manager, _temp) = create_test_manager().await;

        // Create messages with different priorities and ensure unique IDs by adding delays
        let content1 = MessageContent {
            subject: "Low Priority".to_string(),
            body: "Low priority message".to_string(),
        };
        let low_msg = Message::new(0, MessageRecipient::expert_id(1), MessageType::Query, content1)
            .with_priority(MessagePriority::Low);
        
        // Small delay to ensure different timestamps
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        
        let content2 = MessageContent {
            subject: "High Priority".to_string(),
            body: "High priority message".to_string(),
        };
        let high_msg = Message::new(0, MessageRecipient::expert_id(2), MessageType::Query, content2)
            .with_priority(MessagePriority::High);
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        
        let content3 = MessageContent {
            subject: "Normal Priority".to_string(),
            body: "Normal priority message".to_string(),
        };
        let normal_msg = Message::new(0, MessageRecipient::expert_id(3), MessageType::Query, content3)
            .with_priority(MessagePriority::Normal);

        // Enqueue in random order
        manager.enqueue(&low_msg).await.unwrap();
        manager.enqueue(&high_msg).await.unwrap();
        manager.enqueue(&normal_msg).await.unwrap();

        let messages = manager.read_queue().await.unwrap();
        assert_eq!(messages.len(), 3);
        
        // Should be ordered: High, Normal, Low
        assert_eq!(messages[0].message.priority, MessagePriority::High);
        assert_eq!(messages[1].message.priority, MessagePriority::Normal);
        assert_eq!(messages[2].message.priority, MessagePriority::Low);
    }

    #[tokio::test]
    async fn queue_manager_update_delivery_attempts() {
        let (manager, _temp) = create_test_manager().await;

        let message = create_test_message();
        manager.enqueue(&message).await.unwrap();

        manager.update_delivery_attempts(&message.message_id, 5).await.unwrap();

        let messages = manager.read_queue().await.unwrap();
        assert_eq!(messages[0].attempts, 5);
        assert_eq!(messages[0].message.delivery_attempts, 5);
    }

    #[tokio::test]
    async fn queue_manager_cleanup_expired_messages() {
        let (manager, _temp) = create_test_manager().await;

        // Create messages with unique IDs
        let expired_content = MessageContent {
            subject: "Expired Message".to_string(),
            body: "This message will expire".to_string(),
        };
        let expired_msg = Message::new(0, MessageRecipient::expert_id(1), MessageType::Query, expired_content)
            .with_ttl_seconds(0);

        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

        let normal_content = MessageContent {
            subject: "Normal Message".to_string(),
            body: "This message is normal".to_string(),
        };
        let normal_msg = Message::new(0, MessageRecipient::expert_id(2), MessageType::Query, normal_content);

        manager.enqueue(&expired_msg).await.unwrap();
        manager.enqueue(&normal_msg).await.unwrap();

        // Small delay to ensure expiration
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let removed = manager.cleanup_expired_messages().await.unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0], expired_msg.message_id);

        let remaining = manager.read_queue().await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].message.message_id, normal_msg.message_id);
    }

    #[tokio::test]
    async fn queue_manager_get_pending_messages_filters_correctly() {
        let (manager, _temp) = create_test_manager().await;

        let pending_content = MessageContent {
            subject: "Pending Message".to_string(),
            body: "This message is pending".to_string(),
        };
        let pending_msg = Message::new(0, MessageRecipient::expert_id(1), MessageType::Query, pending_content);

        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

        let expired_content = MessageContent {
            subject: "Expired Message".to_string(),
            body: "This message will expire".to_string(),
        };
        let expired_msg = Message::new(0, MessageRecipient::expert_id(2), MessageType::Query, expired_content)
            .with_ttl_seconds(0);

        manager.enqueue(&pending_msg).await.unwrap();
        manager.enqueue(&expired_msg).await.unwrap();

        // Small delay to ensure expiration
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let pending = manager.get_pending_messages().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].message.message_id, pending_msg.message_id);
    }

    #[tokio::test]
    async fn queue_manager_process_outbox_valid_message() {
        let (manager, _temp) = create_test_manager().await;

        let message = create_test_message();
        let outbox_path = manager.outbox_path();
        let message_file = outbox_path.join(format!("{}.yaml", message.message_id));
        
        // Write message to outbox
        let yaml_content = serde_yaml::to_string(&message).unwrap();
        fs::write(&message_file, yaml_content).await.unwrap();

        // Process outbox
        let processed = manager.process_outbox().await.unwrap();
        assert_eq!(processed.len(), 1);
        assert_eq!(processed[0], message.message_id);

        // Message should be in queue
        let messages = manager.read_queue().await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message.message_id, message.message_id);

        // Outbox file should be removed
        assert!(!message_file.exists());
    }

    #[tokio::test]
    async fn queue_manager_process_outbox_invalid_message() {
        let (manager, _temp) = create_test_manager().await;

        let outbox_path = manager.outbox_path();
        let invalid_file = outbox_path.join("invalid.yaml");
        
        // Write invalid YAML to outbox
        fs::write(&invalid_file, "invalid: yaml: content: [").await.unwrap();

        // Process outbox should not crash and return empty result
        let processed = manager.process_outbox().await.unwrap();
        assert_eq!(processed.len(), 0);

        // Queue should be empty
        let messages = manager.read_queue().await.unwrap();
        assert_eq!(messages.len(), 0);

        // Invalid file should still exist (not removed on error)
        assert!(invalid_file.exists());
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use crate::models::{MessageContent, MessageRecipient, MessageType, MessagePriority};

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

    fn arbitrary_valid_message() -> impl Strategy<Value = Message> {
        (
            0u32..100,
            arbitrary_message_recipient(),
            arbitrary_message_type(),
            arbitrary_message_content(),
            arbitrary_message_priority(),
            1u64..86400,
        ).prop_map(|(from_expert_id, to, message_type, content, priority, ttl_seconds)| {
            Message::new(from_expert_id, to, message_type, content)
                .with_priority(priority)
                .with_ttl_seconds(ttl_seconds)
        })
    }

    async fn create_test_manager_for_props() -> (QueueManager, tempfile::TempDir) {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let manager = QueueManager::new(temp_dir.path().to_path_buf());
        manager.init().await.unwrap();
        (manager, temp_dir)
    }

    // Feature: inter-expert-messaging, Property 2: Message Validation Consistency
    // **Validates: Requirements 1.2, 1.3**
    proptest! {
        #[test]
        fn message_validation_consistency(
            message in arbitrary_valid_message()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;
                
                // Valid messages should always be accepted
                let result = manager.enqueue(&message).await;
                assert!(result.is_ok(), "Valid message should be accepted: {:?}", result);
                
                // Message should be retrievable from queue
                let messages = manager.read_queue().await.unwrap();
                assert!(!messages.is_empty(), "Enqueued message should be in queue");
                
                let found_message = messages.iter()
                    .find(|m| m.message.message_id == message.message_id);
                assert!(found_message.is_some(), "Enqueued message should be findable in queue");
                
                // Message content should be preserved
                let found = found_message.unwrap();
                assert_eq!(found.message.content.subject, message.content.subject);
                assert_eq!(found.message.content.body, message.content.body);
                assert_eq!(found.message.priority, message.priority);
                assert_eq!(found.message.message_type, message.message_type);
            });
        }

        // Feature: inter-expert-messaging, Property 1: Message Queue Acceptance
        // **Validates: Requirements 1.1, 1.4, 1.5**
        #[test]
        fn message_queue_accepts_valid_outbox_messages(
            message in arbitrary_valid_message()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;
                
                // Write message to outbox as YAML file
                let outbox_path = manager.outbox_path();
                let message_file = outbox_path.join(format!("{}.yaml", message.message_id));
                let yaml_content = serde_yaml::to_string(&message).unwrap();
                fs::write(&message_file, yaml_content).await.unwrap();
                
                // Process outbox
                let processed = manager.process_outbox().await.unwrap();
                
                // Message should be processed successfully
                assert_eq!(processed.len(), 1, "One message should be processed from outbox");
                assert_eq!(processed[0], message.message_id, "Processed message ID should match");
                
                // Message should be in the queue with unique ID and timestamp
                let queued_messages = manager.read_queue().await.unwrap();
                assert_eq!(queued_messages.len(), 1, "One message should be in queue");
                
                let queued = &queued_messages[0];
                assert_eq!(queued.message.message_id, message.message_id, "Message ID should be preserved");
                assert!(!queued.message.message_id.is_empty(), "Message should have unique ID");
                assert!(queued.message.created_at <= chrono::Utc::now(), "Message should have valid timestamp");
                
                // Message content should be preserved
                assert_eq!(queued.message.content.subject, message.content.subject);
                assert_eq!(queued.message.content.body, message.content.body);
                assert_eq!(queued.message.priority, message.priority);
                assert_eq!(queued.message.message_type, message.message_type);
                assert_eq!(queued.message.to, message.to);
                assert_eq!(queued.message.from_expert_id, message.from_expert_id);
                
                // Outbox file should be removed after processing
                assert!(!message_file.exists(), "Outbox file should be removed after processing");
            });
        }

        #[test]
        fn message_queue_operations_are_consistent(
            messages in prop::collection::vec(arbitrary_valid_message(), 1..10)
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;
                
                // Ensure unique message IDs by adding delays and modifying IDs
                let mut unique_messages = Vec::new();
                for (i, mut message) in messages.into_iter().enumerate() {
                    // Ensure unique ID by appending index
                    message.message_id = format!("{}-{}", message.message_id, i);
                    unique_messages.push(message);
                    
                    // Small delay to ensure different timestamps for next message
                    if i < 9 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    }
                }
                
                // Enqueue all messages
                for message in &unique_messages {
                    manager.enqueue(message).await.unwrap();
                }
                
                // Queue length should match number of enqueued messages
                let queue_len = manager.queue_len().await.unwrap();
                assert_eq!(queue_len, unique_messages.len());
                
                // All messages should be retrievable
                let queued_messages = manager.read_queue().await.unwrap();
                assert_eq!(queued_messages.len(), unique_messages.len());
                
                // Each original message should have a corresponding queued message
                for original in &unique_messages {
                    let found = queued_messages.iter()
                        .find(|q| q.message.message_id == original.message_id);
                    assert!(found.is_some(), "Message {} should be in queue", original.message_id);
                }
            });
        }

        #[test]
        fn message_dequeue_removes_correctly(
            message in arbitrary_valid_message()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;
                
                // Enqueue message
                manager.enqueue(&message).await.unwrap();
                assert_eq!(manager.queue_len().await.unwrap(), 1);
                
                // Dequeue message
                manager.dequeue(&message.message_id).await.unwrap();
                assert_eq!(manager.queue_len().await.unwrap(), 0);
                
                // Message should no longer be in queue
                let messages = manager.read_queue().await.unwrap();
                assert!(messages.is_empty());
            });
        }

        #[test]
        fn message_priority_ordering_is_consistent(
            messages in prop::collection::vec(arbitrary_valid_message(), 2..5)
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;
                
                // Ensure messages have unique IDs and different timestamps
                let mut unique_messages = Vec::new();
                for (i, mut message) in messages.into_iter().enumerate() {
                    // Ensure unique ID
                    message.message_id = format!("{}-{}", message.message_id, i);
                    unique_messages.push(message);
                    
                    // Small delay to ensure different timestamps
                    if i < 4 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    }
                }
                
                // Enqueue all messages
                for message in &unique_messages {
                    manager.enqueue(message).await.unwrap();
                }
                
                // Read queue and verify priority ordering
                let queued_messages = manager.read_queue().await.unwrap();
                assert_eq!(queued_messages.len(), unique_messages.len());
                
                // Messages should be sorted by priority (High > Normal > Low), then by timestamp
                for i in 1..queued_messages.len() {
                    let prev = &queued_messages[i-1];
                    let curr = &queued_messages[i];

                    // Either previous has higher priority, or same priority with earlier timestamp
                    assert!(
                        prev.message.priority > curr.message.priority ||
                        (prev.message.priority == curr.message.priority &&
                         prev.message.created_at <= curr.message.created_at),
                        "Messages should be ordered by priority then timestamp. Prev: {:?} {:?}, Curr: {:?} {:?}",
                        prev.message.priority, prev.message.created_at,
                        curr.message.priority, curr.message.created_at
                    );
                }
            });
        }

        // Feature: inter-expert-messaging, Property 7: Message Lifecycle Management
        // **Validates: Requirements 5.1, 5.2, 5.3, 5.5**
        #[test]
        fn message_lifecycle_ttl_expiration(
            message in arbitrary_valid_message()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                // Create message with immediate expiration (TTL = 0)
                let mut expired_message = message.clone();
                expired_message.message_id = format!("{}-expired", expired_message.message_id);
                expired_message.expires_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));

                // Enqueue both expired and valid message
                manager.enqueue(&expired_message).await.unwrap();

                // Small delay to ensure valid message has different timestamp
                tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;

                let mut valid_message = message;
                valid_message.message_id = format!("{}-valid", valid_message.message_id);
                valid_message.expires_at = Some(chrono::Utc::now() + chrono::Duration::hours(24));
                manager.enqueue(&valid_message).await.unwrap();

                // Small delay to ensure expiration is detected
                tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

                // Cleanup expired messages
                let removed = manager.cleanup_expired_messages().await.unwrap();

                // Expired message should be removed
                assert!(
                    removed.contains(&expired_message.message_id),
                    "Expired message should be removed during cleanup"
                );
                assert!(
                    !removed.contains(&valid_message.message_id),
                    "Valid message should not be removed during cleanup"
                );

                // Queue should only contain the valid message
                let remaining = manager.read_queue().await.unwrap();
                assert_eq!(remaining.len(), 1, "Only valid message should remain");
                assert_eq!(
                    remaining[0].message.message_id,
                    valid_message.message_id,
                    "Remaining message should be the valid one"
                );
            });
        }

        #[test]
        fn message_lifecycle_max_attempts_cleanup(
            message in arbitrary_valid_message()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                // Create message that has exceeded max delivery attempts
                let mut max_attempts_message = message.clone();
                max_attempts_message.message_id = format!("{}-maxattempts", max_attempts_message.message_id);
                max_attempts_message.delivery_attempts = crate::models::MAX_DELIVERY_ATTEMPTS;

                // Create a valid message with no delivery attempts
                let mut valid_message = message;
                valid_message.message_id = format!("{}-valid", valid_message.message_id);
                valid_message.delivery_attempts = 0;

                // Enqueue both messages
                manager.enqueue(&max_attempts_message).await.unwrap();

                tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
                manager.enqueue(&valid_message).await.unwrap();

                // Cleanup should remove the max attempts message
                let removed = manager.cleanup_expired_messages().await.unwrap();

                // Max attempts message should be removed
                assert!(
                    removed.contains(&max_attempts_message.message_id),
                    "Message with max delivery attempts should be removed"
                );
                assert!(
                    !removed.contains(&valid_message.message_id),
                    "Valid message should not be removed"
                );

                // Queue should only contain the valid message
                let remaining = manager.read_queue().await.unwrap();
                assert_eq!(remaining.len(), 1, "Only valid message should remain");
                assert_eq!(
                    remaining[0].message.message_id,
                    valid_message.message_id,
                    "Remaining message should be the valid one"
                );
            });
        }

        #[test]
        fn message_lifecycle_get_pending_filters_correctly(
            messages in prop::collection::vec(arbitrary_valid_message(), 2..5)
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                let mut pending_count = 0;
                let mut expired_count = 0;
                let mut max_attempts_count = 0;

                // Enqueue messages with varying states
                for (i, mut message) in messages.into_iter().enumerate() {
                    message.message_id = format!("{}-{}", message.message_id, i);

                    match i % 3 {
                        0 => {
                            // Valid pending message
                            message.expires_at = Some(chrono::Utc::now() + chrono::Duration::hours(24));
                            message.delivery_attempts = 0;
                            pending_count += 1;
                        },
                        1 => {
                            // Expired message
                            message.expires_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
                            expired_count += 1;
                        },
                        _ => {
                            // Max attempts message
                            message.delivery_attempts = crate::models::MAX_DELIVERY_ATTEMPTS;
                            max_attempts_count += 1;
                        }
                    }

                    manager.enqueue(&message).await.unwrap();
                    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
                }

                // Small delay to ensure expiration is detected
                tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

                // Get pending messages (filters out expired and max attempts)
                let pending = manager.get_pending_messages().await.unwrap();

                // Only pending messages should be returned
                assert_eq!(
                    pending.len(),
                    pending_count,
                    "Only pending messages should be returned. Expected {}, got {}. Expired: {}, MaxAttempts: {}",
                    pending_count, pending.len(), expired_count, max_attempts_count
                );

                // Verify all returned messages are valid (not expired, not max attempts)
                for queued_msg in &pending {
                    assert!(
                        !queued_msg.message.is_expired(),
                        "Pending messages should not be expired"
                    );
                    assert!(
                        !queued_msg.message.has_exceeded_max_attempts(),
                        "Pending messages should not have exceeded max attempts"
                    );
                    assert!(
                        queued_msg.should_retry(),
                        "Pending messages should be retryable"
                    );
                }
            });
        }

        // Feature: inter-expert-messaging, Property 8: Delivery Attempt Tracking
        // **Validates: Requirements 2.5, 5.3**
        #[test]
        fn delivery_attempt_tracking_increments_correctly(
            message in arbitrary_valid_message(),
            attempts in 1u32..10
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                // Enqueue message
                manager.enqueue(&message).await.unwrap();

                // Update delivery attempts
                manager.update_delivery_attempts(&message.message_id, attempts).await.unwrap();

                // Read back and verify attempts are tracked
                let messages = manager.read_queue().await.unwrap();
                assert_eq!(messages.len(), 1);

                let queued_msg = &messages[0];
                assert_eq!(
                    queued_msg.attempts, attempts,
                    "QueuedMessage.attempts should be updated to {}", attempts
                );
                assert_eq!(
                    queued_msg.message.delivery_attempts, attempts,
                    "Message.delivery_attempts should be updated to {}", attempts
                );
            });
        }

        #[test]
        fn delivery_attempt_tracking_status_update(
            message in arbitrary_valid_message()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                // Enqueue message
                manager.enqueue(&message).await.unwrap();

                // Read message and update its status
                let mut messages = manager.read_queue().await.unwrap();
                assert_eq!(messages.len(), 1);

                let mut queued_msg = messages.remove(0);

                // Mark delivery attempt
                queued_msg.mark_delivery_attempt();
                assert_eq!(queued_msg.attempts, 1);
                assert!(queued_msg.is_delivering());
                assert!(queued_msg.last_attempt.is_some());

                // Update status in queue
                manager.update_message_status(&message.message_id, &queued_msg).await.unwrap();

                // Read back and verify status is persisted
                let updated_messages = manager.read_queue().await.unwrap();
                assert_eq!(updated_messages.len(), 1);

                let persisted = &updated_messages[0];
                assert_eq!(persisted.attempts, 1, "Attempts should be persisted");
                assert!(persisted.last_attempt.is_some(), "Last attempt time should be persisted");
            });
        }

        #[test]
        fn delivery_attempt_tracking_failure_reason(
            message in arbitrary_valid_message(),
            failure_reason in "[a-zA-Z0-9 ]{1,100}"
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                // Enqueue message
                manager.enqueue(&message).await.unwrap();

                // Read and mark as failed
                let mut messages = manager.read_queue().await.unwrap();
                let mut queued_msg = messages.remove(0);

                queued_msg.mark_failed(failure_reason.clone());
                assert!(queued_msg.is_failed());
                assert_eq!(queued_msg.get_failure_reason(), Some(failure_reason.as_str()));

                // Update status
                manager.update_message_status(&message.message_id, &queued_msg).await.unwrap();

                // Read back and verify failure is persisted
                let updated_messages = manager.read_queue().await.unwrap();
                assert_eq!(updated_messages.len(), 1);

                let persisted = &updated_messages[0];
                assert!(persisted.is_failed(), "Failed status should be persisted");
                assert_eq!(
                    persisted.get_failure_reason(),
                    Some(failure_reason.as_str()),
                    "Failure reason should be persisted"
                );
            });
        }

        #[test]
        fn delivery_attempt_tracking_retry_logic(
            message in arbitrary_valid_message()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                // Create message with valid TTL
                let mut test_message = message;
                test_message.expires_at = Some(chrono::Utc::now() + chrono::Duration::hours(24));
                test_message.delivery_attempts = 0;

                // Enqueue message
                manager.enqueue(&test_message).await.unwrap();

                // Read and verify should_retry is true initially
                let messages = manager.read_queue().await.unwrap();
                let queued_msg = &messages[0];
                assert!(queued_msg.should_retry(), "Fresh message should be retryable");

                // Simulate failed delivery and reset to pending
                let mut updated_msg = queued_msg.clone();
                updated_msg.mark_delivery_attempt();
                updated_msg.mark_failed("Temporary error".to_string());
                updated_msg.reset_to_pending();

                assert!(updated_msg.is_pending(), "Reset message should be pending");
                assert!(updated_msg.should_retry(), "Reset message should be retryable");

                // Update in queue
                manager.update_message_status(&test_message.message_id, &updated_msg).await.unwrap();

                // Verify get_pending_messages returns the retryable message
                let pending = manager.get_pending_messages().await.unwrap();
                assert_eq!(pending.len(), 1, "Retryable message should appear in pending");
                assert!(pending[0].should_retry(), "Pending message should be retryable");
            });
        }

        // Feature: inter-expert-messaging, Property 12: Comprehensive Error Logging
        // **Validates: Requirements 10.1, 10.2, 10.3, 10.4, 10.5**

        #[test]
        fn error_logging_invalid_yaml_parsing(
            invalid_content in "[^{}]*\\{[^{}]*"  // Content that will fail YAML parsing
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                // Write invalid YAML content directly to the queue directory
                let queue_path = manager.queue_path();
                let invalid_file = queue_path.join("invalid-msg.yaml");
                fs::write(&invalid_file, &invalid_content).await.unwrap();

                // Requirement 10.2: Detailed logging for parsing failures
                // The queue should gracefully handle parsing errors and continue
                let messages = manager.read_queue().await.unwrap();

                // Invalid message should be skipped, not crash the system
                // Requirement 10.4: Error isolation to prevent system-wide failures
                assert!(
                    messages.is_empty() || messages.iter().all(|m| !m.message.message_id.contains("invalid")),
                    "Invalid messages should be isolated and not included in queue"
                );

                // Clean up
                let _ = fs::remove_file(&invalid_file).await;
            });
        }

        #[test]
        fn error_logging_message_validation_failures(
            from_expert_id in 0u32..100,
            recipient in arbitrary_message_recipient()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                // Test with empty subject (should fail validation)
                let invalid_content1 = MessageContent {
                    subject: "".to_string(),  // Empty subject
                    body: "Valid body content".to_string(),
                };
                let message1 = Message::new(from_expert_id, recipient.clone(), MessageType::Query, invalid_content1);

                // Requirement 10.1: Error types for queue operations
                // Validation should catch invalid messages
                let validation_result = manager.validate_message(&message1);
                assert!(
                    validation_result.is_err(),
                    "Empty subject should fail validation"
                );

                // Test with empty body
                let invalid_content2 = MessageContent {
                    subject: "Valid subject".to_string(),
                    body: "".to_string(),  // Empty body
                };
                let message2 = Message::new(from_expert_id, recipient.clone(), MessageType::Query, invalid_content2);

                let validation_result2 = manager.validate_message(&message2);
                assert!(
                    validation_result2.is_err(),
                    "Empty body should fail validation"
                );

                // Test with empty message ID
                let valid_content = MessageContent {
                    subject: "Valid subject".to_string(),
                    body: "Valid body".to_string(),
                };
                let mut message3 = Message::new(from_expert_id, recipient, MessageType::Query, valid_content);
                message3.message_id = "".to_string();  // Empty message ID

                let validation_result3 = manager.validate_message(&message3);
                assert!(
                    validation_result3.is_err(),
                    "Empty message ID should fail validation"
                );
            });
        }

        #[test]
        fn error_logging_file_system_isolation(
            message in arbitrary_valid_message()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                // Enqueue a valid message first
                manager.enqueue(&message).await.unwrap();

                // Create a non-YAML file in the queue directory
                let queue_path = manager.queue_path();
                let non_yaml_file = queue_path.join("not-a-yaml.txt");
                fs::write(&non_yaml_file, "This is not YAML").await.unwrap();

                // Create a directory that shouldn't be processed
                let dir_in_queue = queue_path.join("subdir");
                fs::create_dir_all(&dir_in_queue).await.unwrap();

                // Requirement 10.4: Error isolation
                // Reading the queue should only return valid YAML messages
                let messages = manager.read_queue().await.unwrap();

                assert_eq!(
                    messages.len(), 1,
                    "Only valid YAML message files should be read"
                );
                assert_eq!(
                    messages[0].message.message_id, message.message_id,
                    "The valid message should be returned"
                );

                // Clean up
                let _ = fs::remove_file(&non_yaml_file).await;
                let _ = fs::remove_dir(&dir_in_queue).await;
            });
        }

        #[test]
        fn error_logging_outbox_error_isolation(
            valid_message in arbitrary_valid_message()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                let outbox_path = manager.outbox_path();

                // Create valid message file
                let valid_file = outbox_path.join(format!("{}.yaml", valid_message.message_id));
                let valid_yaml = serde_yaml::to_string(&valid_message).unwrap();
                fs::write(&valid_file, valid_yaml).await.unwrap();

                // Small delay to ensure different timestamps
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

                // Create invalid message file (malformed YAML)
                let invalid_file = outbox_path.join("invalid-outbox.yaml");
                fs::write(&invalid_file, "invalid: yaml: [\ncontent").await.unwrap();

                // Create file with valid YAML but missing required fields
                let incomplete_file = outbox_path.join("incomplete-outbox.yaml");
                fs::write(&incomplete_file, "message_id: incomplete\n").await.unwrap();

                // Requirement 10.4: Error isolation
                // Process outbox - should handle errors gracefully
                let processed = manager.process_outbox().await.unwrap();

                // Only the valid message should be processed
                assert_eq!(
                    processed.len(), 1,
                    "Only valid messages should be processed from outbox"
                );
                assert_eq!(
                    processed[0], valid_message.message_id,
                    "The valid message should be processed"
                );

                // The queue should contain only the valid message
                let queued = manager.read_queue().await.unwrap();
                assert_eq!(queued.len(), 1);
                assert_eq!(queued[0].message.message_id, valid_message.message_id);

                // Requirement 10.3: Detailed logging for delivery errors
                // Invalid files should still exist (not deleted on error)
                assert!(invalid_file.exists(), "Invalid file should not be deleted on error");
                assert!(incomplete_file.exists(), "Incomplete file should not be deleted on error");

                // Clean up
                let _ = fs::remove_file(&invalid_file).await;
                let _ = fs::remove_file(&incomplete_file).await;
            });
        }

        #[test]
        fn error_logging_delivery_lifecycle_events(
            message in arbitrary_valid_message()
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                // Requirement 10.5: Appropriate log levels for different event types
                // Test message lifecycle events that should trigger logging

                // 1. Enqueue event (should log at debug level)
                manager.enqueue(&message).await.unwrap();

                // 2. Read event (should log at debug level on parse errors)
                let messages = manager.read_queue().await.unwrap();
                assert_eq!(messages.len(), 1);

                // 3. Update delivery attempts (should log at debug level)
                manager.update_delivery_attempts(&message.message_id, 1).await.unwrap();

                // 4. Status update (should log at debug level)
                let mut queued_msg = messages[0].clone();
                queued_msg.mark_delivery_attempt();
                manager.update_message_status(&message.message_id, &queued_msg).await.unwrap();

                // 5. Dequeue event (should log at debug level)
                manager.dequeue(&message.message_id).await.unwrap();

                // Verify message was removed
                let remaining = manager.read_queue().await.unwrap();
                assert!(remaining.is_empty());
            });
        }

        #[test]
        fn error_logging_cleanup_events(
            messages in prop::collection::vec(arbitrary_valid_message(), 2..5)
        ) {
            tokio_test::block_on(async {
                let (manager, _temp) = create_test_manager_for_props().await;

                let mut expired_count = 0;
                let mut max_attempts_count = 0;
                let mut valid_count = 0;

                // Create messages with different error conditions
                for (i, mut message) in messages.into_iter().enumerate() {
                    message.message_id = format!("{}-{}", message.message_id, i);

                    match i % 3 {
                        0 => {
                            // Expired message - should trigger info log
                            message.expires_at = Some(chrono::Utc::now() - chrono::Duration::seconds(1));
                            expired_count += 1;
                        },
                        1 => {
                            // Max attempts message - should trigger warn log
                            message.delivery_attempts = crate::models::MAX_DELIVERY_ATTEMPTS;
                            max_attempts_count += 1;
                        },
                        _ => {
                            // Valid message - no special logging
                            message.expires_at = Some(chrono::Utc::now() + chrono::Duration::hours(24));
                            message.delivery_attempts = 0;
                            valid_count += 1;
                        }
                    }

                    manager.enqueue(&message).await.unwrap();
                    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
                }

                // Small delay to ensure expiration
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

                // Cleanup should log removal events
                let removed = manager.cleanup_expired_messages().await.unwrap();

                // Verify the expected number of messages were removed
                let expected_removed = expired_count + max_attempts_count;
                assert_eq!(
                    removed.len(), expected_removed,
                    "Should remove {} expired/max-attempts messages, removed {}",
                    expected_removed, removed.len()
                );

                // Verify only valid messages remain
                let remaining = manager.read_queue().await.unwrap();
                assert_eq!(
                    remaining.len(), valid_count,
                    "Should have {} valid messages remaining",
                    valid_count
                );
            });
        }

        #[test]
        fn error_type_coverage(
            path in "[a-zA-Z0-9/]{1,50}",
            message_id in "[a-zA-Z0-9-]{1,30}",
            _field in "[a-zA-Z]{1,20}",
            reason in "[a-zA-Z0-9 ]{1,100}"
        ) {
            // Requirement 10.1: Error types for queue operations
            // Test that all error types can be constructed and provide useful information

            // I/O error
            let io_error = QueueError::io(
                "read",
                path.clone(),
                std::io::Error::new(std::io::ErrorKind::NotFound, "test error")
            );
            let io_msg = io_error.to_string();
            assert!(io_msg.contains("I/O error"));
            assert!(io_msg.contains(&path));

            // YAML parsing error
            let yaml_error = QueueError::yaml_parsing(
                "message",
                path.clone(),
                "invalid syntax"
            );
            let yaml_msg = yaml_error.to_string();
            assert!(yaml_msg.contains("YAML parsing error"));
            assert!(yaml_msg.contains(&path));

            // Validation error
            let validation_error = QueueError::validation(
                "subject",
                reason.clone()
            );
            let validation_msg = validation_error.to_string();
            assert!(validation_msg.contains("validation error"));
            assert!(validation_msg.contains(&reason));

            // Message not found error
            let not_found = QueueError::MessageNotFound {
                message_id: message_id.clone()
            };
            let not_found_msg = not_found.to_string();
            assert!(not_found_msg.contains("not found"));
            assert!(not_found_msg.contains(&message_id));

            // Queue directory error
            let dir_error = QueueError::QueueDirectory {
                path: path.clone(),
                reason: reason.clone()
            };
            let dir_msg = dir_error.to_string();
            assert!(dir_msg.contains("Queue directory error"));
            assert!(dir_msg.contains(&path));

            // Atomic write error
            let atomic_error = QueueError::AtomicWrite {
                path: path.clone(),
                reason: reason.clone()
            };
            let atomic_msg = atomic_error.to_string();
            assert!(atomic_msg.contains("Atomic write failed"));
            assert!(atomic_msg.contains(&path));
        }
    }
}
