use anyhow::{Context, Result};
use thiserror::Error;
use tracing::{debug, error, info, warn};

use crate::experts::ExpertRegistry;
use crate::models::{
    ExpertId, Message, MessageId, MessageRecipient, QueuedMessage, MAX_DELIVERY_ATTEMPTS,
};
use crate::session::TmuxSender;

use super::QueueManager;

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("Queue error: {0}")]
    Queue(#[from] anyhow::Error),

    #[error("Tmux error: {0}")]
    Tmux(String),

    #[error("Expert not found: {0}")]
    ExpertNotFound(String),

    #[allow(dead_code)]
    #[error("Delivery failed: {0}")]
    DeliveryFailed(String),

    #[allow(dead_code)]
    #[error("No idle experts available for role: {0}")]
    NoIdleExpertsForRole(String),

    #[error("Registry error: {0}")]
    Registry(#[from] crate::experts::RegistryError),
}

#[derive(Debug, Clone)]
pub struct DeliveryResult {
    pub success: bool,
    pub message_id: MessageId,
    pub expert_id: Option<ExpertId>,
    pub error: Option<String>,
}

impl DeliveryResult {
    pub fn success(message_id: MessageId, expert_id: ExpertId) -> Self {
        Self {
            success: true,
            message_id,
            expert_id: Some(expert_id),
            error: None,
        }
    }

    pub fn failed(message_id: MessageId, error: String) -> Self {
        Self {
            success: false,
            message_id,
            expert_id: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProcessingStats {
    pub messages_processed: usize,
    pub messages_delivered: usize,
    pub messages_failed: usize,
    pub messages_expired: usize,
    pub messages_skipped: usize,
    pub delivered_expert_ids: Vec<u32>,
}

/// MessageRouter handles message routing logic and delivery coordination
///
/// The MessageRouter is responsible for:
/// - Processing queued messages in priority order
/// - Finding appropriate recipients using targeting strategies (ID, name, role)
/// - Checking expert availability (idle state) for non-blocking delivery
/// - Delivering messages via tmux integration
/// - Managing delivery attempts and retry logic
/// - Cleaning up expired messages and failed deliveries
pub struct MessageRouter<T: TmuxSender = crate::session::TmuxManager> {
    queue_manager: QueueManager,
    expert_registry: ExpertRegistry,
    tmux_sender: T,
}

impl<T: TmuxSender> MessageRouter<T> {
    /// Create a new MessageRouter with the provided dependencies
    pub fn new(
        queue_manager: QueueManager,
        expert_registry: ExpertRegistry,
        tmux_sender: T,
    ) -> Self {
        Self {
            queue_manager,
            expert_registry,
            tmux_sender,
        }
    }

    /// Process the message queue, attempting delivery for all pending messages
    ///
    /// This method:
    /// 1. Cleans up expired messages
    /// 2. Retrieves pending messages in priority order
    /// 3. Attempts delivery for each message
    /// 4. Updates message status and delivery attempts
    /// 5. Returns processing statistics
    pub async fn process_queue(&mut self) -> Result<ProcessingStats, RouterError> {
        let mut stats = ProcessingStats::default();

        // First, clean up expired messages
        let expired_messages = self.queue_manager.cleanup_expired_messages().await?;
        stats.messages_expired = expired_messages.len();

        // Get pending messages (already sorted by priority and timestamp)
        let pending_messages = self.queue_manager.get_pending_messages().await?;
        stats.messages_processed = pending_messages.len();

        debug!(
            "Processing {} pending messages, cleaned up {} expired messages",
            pending_messages.len(),
            expired_messages.len()
        );

        // Process each message
        for queued_message in pending_messages {
            match self.attempt_delivery(&queued_message).await {
                Ok(result) => {
                    if result.success {
                        stats.messages_delivered += 1;
                        if let Some(eid) = result.expert_id {
                            stats.delivered_expert_ids.push(eid);
                        }
                        // Remove successfully delivered message from queue
                        self.queue_manager
                            .dequeue(&result.message_id)
                            .await
                            .context("Failed to dequeue delivered message")?;

                        info!(
                            "Successfully delivered message {} to expert {:?}",
                            result.message_id, result.expert_id
                        );
                    } else {
                        stats.messages_failed += 1;
                        // Update delivery attempts and status
                        let mut updated_message = queued_message.clone();
                        updated_message.mark_delivery_attempt();

                        if let Some(error) = &result.error {
                            updated_message.mark_failed(error.clone());
                        }

                        // Check if message should be removed due to max attempts
                        if updated_message.message.has_exceeded_max_attempts() {
                            warn!(
                                "Removing message {} after {} failed delivery attempts",
                                result.message_id, updated_message.attempts
                            );
                            self.queue_manager.dequeue(&result.message_id).await?;
                        } else {
                            // Update message status in queue
                            updated_message.reset_to_pending(); // Reset for retry
                            self.queue_manager
                                .update_message_status(&result.message_id, &updated_message)
                                .await?;
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "Error processing message {}: {}",
                        queued_message.message.message_id, e
                    );
                    stats.messages_failed += 1;
                }
            }
        }

        debug!(
            "Queue processing complete. Delivered: {}, Failed: {}, Expired: {}, Skipped: {}",
            stats.messages_delivered,
            stats.messages_failed,
            stats.messages_expired,
            stats.messages_skipped
        );

        Ok(stats)
    }

    /// Attempt delivery of a single message
    ///
    /// This method:
    /// 1. Finds the appropriate recipient using targeting logic
    /// 2. Checks if the recipient is idle (non-blocking delivery)
    /// 3. Delivers the message via tmux if recipient is available
    /// 4. Returns delivery result with success/failure information
    pub async fn attempt_delivery(
        &mut self,
        queued_message: &QueuedMessage,
    ) -> Result<DeliveryResult, RouterError> {
        let message = &queued_message.message;

        debug!(
            "Attempting delivery of message {} (attempt {}/{})",
            message.message_id,
            queued_message.attempts + 1,
            MAX_DELIVERY_ATTEMPTS
        );

        // Find recipient expert (worktree-aware)
        let sender_id = message.from_expert_id;
        let expert_id = match self.find_recipient(&message.to, sender_id).await? {
            Some(id) => id,
            None => {
                let error = format!("No recipient found for targeting: {:?}", message.to);
                warn!("{}", error);
                return Ok(DeliveryResult::failed(message.message_id.clone(), error));
            }
        };

        // Check if expert is idle (non-blocking delivery requirement)
        if !self.is_expert_idle(expert_id).await? {
            debug!(
                "Expert {} is not idle, skipping delivery of message {}",
                expert_id, message.message_id
            );
            return Ok(DeliveryResult::failed(
                message.message_id.clone(),
                format!("Expert {expert_id} is not idle"),
            ));
        }

        // Attempt tmux delivery
        match self.deliver_via_tmux(expert_id, message).await {
            Ok(()) => {
                debug!(
                    "Successfully delivered message {} to expert {}",
                    message.message_id, expert_id
                );
                Ok(DeliveryResult::success(
                    message.message_id.clone(),
                    expert_id,
                ))
            }
            Err(e) => {
                let error = format!("Tmux delivery failed: {e}");
                warn!("{}", error);
                Ok(DeliveryResult::failed(message.message_id.clone(), error))
            }
        }
    }

    /// Check if sender and recipient share the same worktree context
    fn worktree_matches(&self, sender_id: ExpertId, recipient_id: ExpertId) -> bool {
        let sender = match self.expert_registry.get_expert(sender_id) {
            Some(s) => s,
            None => {
                warn!(
                    "worktree_matches: sender expert {} not found in registry",
                    sender_id
                );
                return false;
            }
        };
        let recipient = match self.expert_registry.get_expert(recipient_id) {
            Some(r) => r,
            None => {
                warn!(
                    "worktree_matches: recipient expert {} not found in registry",
                    recipient_id
                );
                return false;
            }
        };
        sender.same_worktree(recipient)
    }

    /// Find the appropriate recipient expert based on targeting strategy
    ///
    /// Supports three targeting strategies:
    /// 1. ExpertId: Direct targeting by expert ID
    /// 2. ExpertName: Targeting by expert name (case-insensitive)
    /// 3. Role: Targeting by role (finds first idle expert with matching role)
    ///
    /// All strategies enforce worktree affinity: the recipient must share the
    /// same worktree context as the sender.
    pub async fn find_recipient(
        &self,
        recipient: &MessageRecipient,
        sender_id: ExpertId,
    ) -> Result<Option<ExpertId>, RouterError> {
        match recipient {
            MessageRecipient::ExpertId { expert_id } => {
                // Direct targeting by ID
                if self.expert_registry.get_expert(*expert_id).is_some() {
                    if self.worktree_matches(sender_id, *expert_id) {
                        Ok(Some(*expert_id))
                    } else {
                        warn!(
                            "Expert {} is in a different worktree than sender {}",
                            expert_id, sender_id
                        );
                        Ok(None)
                    }
                } else {
                    warn!("Expert with ID {} not found in registry", expert_id);
                    Ok(None)
                }
            }
            MessageRecipient::ExpertName { expert_name } => {
                // Targeting by name (case-insensitive)
                match self.expert_registry.find_by_name(expert_name) {
                    Some(expert_id) => {
                        if self.worktree_matches(sender_id, expert_id) {
                            Ok(Some(expert_id))
                        } else {
                            warn!(
                                "Expert {} is in a different worktree than sender {}",
                                expert_id, sender_id
                            );
                            Ok(None)
                        }
                    }
                    None => {
                        warn!("Expert with name '{}' not found in registry", expert_name);
                        Ok(None)
                    }
                }
            }
            MessageRecipient::Role { role } => {
                // Role-based targeting - find first idle expert with matching role and worktree
                let sender_worktree = self
                    .expert_registry
                    .get_expert(sender_id)
                    .map(|e| e.worktree_path.clone());
                let idle_experts = match sender_worktree {
                    Some(ref wt) => self
                        .expert_registry
                        .get_idle_experts_by_role_str_in_worktree(role, wt),
                    None => {
                        // Sender not found in registry; fall back to empty
                        vec![]
                    }
                };

                if idle_experts.is_empty() {
                    debug!(
                        "No idle experts found for role '{}' in sender's worktree",
                        role
                    );
                    Ok(None)
                } else {
                    let expert_id = idle_experts[0];
                    debug!("Found idle expert {} for role '{}'", expert_id, role);
                    Ok(Some(expert_id))
                }
            }
        }
    }

    /// Check if a specific expert is idle and available for message delivery
    pub async fn is_expert_idle(&self, expert_id: ExpertId) -> Result<bool, RouterError> {
        match self.expert_registry.is_expert_idle(expert_id) {
            Some(is_idle) => Ok(is_idle),
            None => {
                warn!("Expert {} not found when checking idle status", expert_id);
                Ok(false)
            }
        }
    }

    /// Deliver a message to an expert via tmux
    ///
    /// This method formats the message for delivery and sends it to the expert's
    /// tmux pane using the standardized message format.
    pub async fn deliver_via_tmux(
        &self,
        expert_id: ExpertId,
        message: &Message,
    ) -> Result<(), RouterError> {
        // Get expert info for tmux pane details
        let expert_info = self
            .expert_registry
            .get_expert(expert_id)
            .ok_or_else(|| RouterError::ExpertNotFound(expert_id.to_string()))?;

        // Parse window ID from tmux_window string
        let window_id: u32 = expert_info.tmux_window.parse().map_err(|e| {
            RouterError::Tmux(format!(
                "Invalid window ID '{}': {}",
                expert_info.tmux_window, e
            ))
        })?;

        // Format message for delivery
        let formatted_message =
            self.format_message_for_delivery(message, expert_info.name.as_str());

        // Send message via tmux
        self.tmux_sender
            .send_keys_with_enter(window_id, &formatted_message)
            .await
            .map_err(|e| {
                RouterError::Tmux(format!("Failed to send message to window {window_id}: {e}"))
            })?;

        debug!(
            "Delivered message {} to expert {} (window {})",
            message.message_id, expert_id, window_id
        );

        Ok(())
    }

    /// Format a message for standardized delivery to experts
    ///
    /// Creates a consistent message format that includes all required information
    /// for the receiving expert to understand and process the message.
    fn format_message_for_delivery(&self, message: &Message, recipient_name: &str) -> String {
        let message_type = match message.message_type {
            crate::models::MessageType::Query => "QUERY",
            crate::models::MessageType::Response => "RESPONSE",
            crate::models::MessageType::Notify => "NOTIFICATION",
            crate::models::MessageType::Delegate => "TASK_DELEGATION",
        };

        let priority = match message.priority {
            crate::models::MessagePriority::High => "HIGH",
            crate::models::MessagePriority::Normal => "NORMAL",
            crate::models::MessagePriority::Low => "LOW",
        };

        let sender_info = self
            .expert_registry
            .get_expert(message.from_expert_id)
            .map(|expert| expert.name.as_str())
            .unwrap_or("Unknown");

        // Create standardized message format
        format!(
            "📨 INCOMING MESSAGE [{}] 📨\n\
            From: {} (Expert {})\n\
            To: {}\n\
            Type: {}\n\
            Priority: {}\n\
            Subject: {}\n\
            \n\
            {}\n\
            \n\
            Message ID: {}\n\
            Timestamp: {}\n\
            {}",
            priority,
            sender_info,
            message.from_expert_id,
            recipient_name,
            message_type,
            priority,
            message.content.subject,
            message.content.body,
            message.message_id,
            message.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
            if let Some(reply_to) = &message.reply_to {
                format!("Reply to: {reply_to}")
            } else {
                String::new()
            }
        )
    }

    /// Get access to the queue manager for external operations
    pub fn queue_manager(&self) -> &QueueManager {
        &self.queue_manager
    }

    /// Get mutable access to the queue manager for external operations
    #[allow(dead_code)]
    pub fn queue_manager_mut(&mut self) -> &mut QueueManager {
        &mut self.queue_manager
    }

    /// Get access to the expert registry for external operations
    #[allow(dead_code)]
    pub fn expert_registry(&self) -> &ExpertRegistry {
        &self.expert_registry
    }

    /// Get mutable access to the expert registry for external operations
    pub fn expert_registry_mut(&mut self) -> &mut ExpertRegistry {
        &mut self.expert_registry
    }

    /// Process outbox messages and add them to the queue
    ///
    /// This method processes messages from the outbox directory and moves
    /// valid messages to the main queue for delivery processing.
    pub async fn process_outbox(&mut self) -> Result<Vec<MessageId>, RouterError> {
        let processed = self.queue_manager.process_outbox().await?;

        if !processed.is_empty() {
            info!("Processed {} messages from outbox", processed.len());
        }

        Ok(processed)
    }

    /// Get current queue statistics
    #[allow(dead_code)]
    pub async fn get_queue_stats(&self) -> Result<QueueStats, RouterError> {
        let all_messages = self.queue_manager.read_queue().await?;
        let pending_messages = self.queue_manager.get_pending_messages().await?;

        let mut stats = QueueStats {
            total_messages: all_messages.len(),
            pending_messages: pending_messages.len(),
            ..Default::default()
        };

        // Count by priority
        for message in &all_messages {
            match message.message.priority {
                crate::models::MessagePriority::High => stats.high_priority += 1,
                crate::models::MessagePriority::Normal => stats.normal_priority += 1,
                crate::models::MessagePriority::Low => stats.low_priority += 1,
            }
        }

        // Count by status
        for message in &all_messages {
            if message.is_expired() {
                stats.expired_messages += 1;
            } else if message.is_failed() {
                stats.failed_messages += 1;
            }
        }

        Ok(stats)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct QueueStats {
    pub total_messages: usize,
    pub pending_messages: usize,
    pub high_priority: usize,
    pub normal_priority: usize,
    pub low_priority: usize,
    pub expired_messages: usize,
    pub failed_messages: usize,
}

#[cfg(test)]
mod mock_tmux {
    use crate::session::TmuxSender;

    #[derive(Clone)]
    pub struct MockTmuxSender;

    #[async_trait::async_trait]
    impl TmuxSender for MockTmuxSender {
        async fn send_keys(&self, _pane_id: u32, _keys: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn capture_pane(&self, _pane_id: u32) -> anyhow::Result<String> {
            Ok(String::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock_tmux::MockTmuxSender;
    use super::*;
    use crate::models::{
        ExpertInfo, ExpertState, MessageContent, MessagePriority, MessageType, Role,
    };
    use tempfile::TempDir;

    async fn create_test_router() -> (MessageRouter<MockTmuxSender>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let queue_manager = QueueManager::new(temp_dir.path().to_path_buf());
        queue_manager.init().await.unwrap();

        let mut expert_registry = ExpertRegistry::new();

        // Register test experts
        let expert1 = ExpertInfo::new(
            1,
            "backend-dev".to_string(),
            Role::Developer,
            "test-session".to_string(),
            "0".to_string(),
        );
        let expert2 = ExpertInfo::new(
            2,
            "frontend-dev".to_string(),
            Role::Developer,
            "test-session".to_string(),
            "1".to_string(),
        );

        expert_registry.register_expert(expert1).unwrap();
        expert_registry.register_expert(expert2).unwrap();

        let router = MessageRouter::new(queue_manager, expert_registry, MockTmuxSender);
        (router, temp_dir)
    }

    fn create_test_message() -> Message {
        let content = MessageContent {
            subject: "Test Subject".to_string(),
            body: "Test Body".to_string(),
        };
        let recipient = MessageRecipient::expert_id(1);
        Message::new(1, recipient, MessageType::Query, content)
    }

    #[tokio::test]
    async fn router_new_creates_with_dependencies() {
        let (router, _temp) = create_test_router().await;
        assert_eq!(router.expert_registry().len(), 2);
    }

    #[tokio::test]
    async fn find_recipient_by_expert_id() {
        let (router, _temp) = create_test_router().await;

        // sender_id=1 (same worktree=None as recipient)
        let recipient = MessageRecipient::expert_id(1);
        let result = router.find_recipient(&recipient, 2).await.unwrap();
        assert_eq!(result, Some(1));

        let recipient = MessageRecipient::expert_id(999);
        let result = router.find_recipient(&recipient, 1).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn find_recipient_by_expert_name() {
        let (router, _temp) = create_test_router().await;

        let recipient = MessageRecipient::expert_name("backend-dev".to_string());
        let result = router.find_recipient(&recipient, 2).await.unwrap();
        assert_eq!(result, Some(1));

        let recipient = MessageRecipient::expert_name("nonexistent".to_string());
        let result = router.find_recipient(&recipient, 1).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn find_recipient_by_role_returns_idle_expert() {
        let (mut router, _temp) = create_test_router().await;

        // Set expert 1 to idle
        router
            .expert_registry_mut()
            .update_expert_state(1, ExpertState::Idle)
            .unwrap();

        let recipient = MessageRecipient::role("developer".to_string());
        let result = router.find_recipient(&recipient, 2).await.unwrap();
        assert_eq!(result, Some(1));
    }

    #[tokio::test]
    async fn find_recipient_by_role_returns_none_when_no_idle_experts() {
        let (mut router, _temp) = create_test_router().await;

        // Set all experts to busy
        router
            .expert_registry_mut()
            .update_expert_state(1, ExpertState::Busy)
            .unwrap();
        router
            .expert_registry_mut()
            .update_expert_state(2, ExpertState::Busy)
            .unwrap();

        let recipient = MessageRecipient::role("developer".to_string());
        let result = router.find_recipient(&recipient, 1).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn is_expert_idle_returns_correct_status() {
        let (mut router, _temp) = create_test_router().await;

        // Initially idle
        assert!(router.is_expert_idle(1).await.unwrap());

        // Set to busy
        router
            .expert_registry_mut()
            .update_expert_state(1, ExpertState::Busy)
            .unwrap();
        assert!(!router.is_expert_idle(1).await.unwrap());

        // Nonexistent expert
        assert!(!router.is_expert_idle(999).await.unwrap());
    }

    #[tokio::test]
    async fn format_message_for_delivery_creates_standard_format() {
        let (router, _temp) = create_test_router().await;

        let message = create_test_message();
        let formatted = router.format_message_for_delivery(&message, "backend-dev");

        assert!(formatted.contains("📨 INCOMING MESSAGE"));
        assert!(formatted.contains("From: backend-dev (Expert 1)"));
        assert!(formatted.contains("To: backend-dev"));
        assert!(formatted.contains("Type: QUERY"));
        assert!(formatted.contains("Priority: NORMAL"));
        assert!(formatted.contains("Subject: Test Subject"));
        assert!(formatted.contains("Test Body"));
        assert!(formatted.contains(&message.message_id));
    }

    #[tokio::test]
    async fn process_queue_handles_empty_queue() {
        let (mut router, _temp) = create_test_router().await;

        let stats = router.process_queue().await.unwrap();
        assert_eq!(stats.messages_processed, 0);
        assert_eq!(stats.messages_delivered, 0);
        assert_eq!(stats.messages_failed, 0);
    }

    #[tokio::test]
    async fn get_queue_stats_returns_correct_counts() {
        let (mut router, _temp) = create_test_router().await;

        // Add some test messages with unique IDs
        let content1 = MessageContent {
            subject: "High Priority Message".to_string(),
            body: "This is a high priority message".to_string(),
        };
        let message1 = Message::new(
            0,
            MessageRecipient::expert_id(1),
            MessageType::Query,
            content1,
        )
        .with_priority(MessagePriority::High);

        // Small delay to ensure different timestamps and IDs
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

        let content2 = MessageContent {
            subject: "Low Priority Message".to_string(),
            body: "This is a low priority message".to_string(),
        };
        let message2 = Message::new(
            0,
            MessageRecipient::expert_id(2),
            MessageType::Query,
            content2,
        )
        .with_priority(MessagePriority::Low);

        router.queue_manager_mut().enqueue(&message1).await.unwrap();
        router.queue_manager_mut().enqueue(&message2).await.unwrap();

        let stats = router.get_queue_stats().await.unwrap();
        assert_eq!(stats.total_messages, 2);
        assert_eq!(stats.high_priority, 1);
        assert_eq!(stats.low_priority, 1);
        assert_eq!(stats.normal_priority, 0);
    }

    #[tokio::test]
    async fn process_outbox_delegates_to_queue_manager() {
        let (mut router, _temp) = create_test_router().await;

        // Process outbox (should be empty)
        let processed = router.process_outbox().await.unwrap();
        assert!(processed.is_empty());
    }
}

#[cfg(test)]
mod worktree_tests {
    use super::mock_tmux::MockTmuxSender;
    use super::*;
    use crate::models::{
        ExpertInfo, ExpertState, MessageContent, MessageType, QueuedMessage, Role,
    };
    use tempfile::TempDir;

    async fn create_worktree_router() -> (MessageRouter<MockTmuxSender>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let queue_manager = QueueManager::new(temp_dir.path().to_path_buf());
        queue_manager.init().await.unwrap();

        let mut expert_registry = ExpertRegistry::new();

        // Expert 0: main repo (architect)
        let expert0 = ExpertInfo::new(
            0,
            "architect".to_string(),
            Role::Analyst,
            "test-session".to_string(),
            "0".to_string(),
        );
        // Expert 1: worktree feature-auth (developer)
        let mut expert1 = ExpertInfo::new(
            1,
            "auth-dev".to_string(),
            Role::Developer,
            "test-session".to_string(),
            "1".to_string(),
        );
        expert1.set_worktree_path(Some("/wt/feature-auth".to_string()));

        // Expert 2: worktree feature-auth (reviewer)
        let mut expert2 = ExpertInfo::new(
            2,
            "auth-reviewer".to_string(),
            Role::Reviewer,
            "test-session".to_string(),
            "2".to_string(),
        );
        expert2.set_worktree_path(Some("/wt/feature-auth".to_string()));

        // Expert 3: worktree feature-payments (developer)
        let mut expert3 = ExpertInfo::new(
            3,
            "payments-dev".to_string(),
            Role::Developer,
            "test-session".to_string(),
            "3".to_string(),
        );
        expert3.set_worktree_path(Some("/wt/feature-payments".to_string()));

        // Expert 4: main repo (developer)
        let expert4 = ExpertInfo::new(
            4,
            "main-dev".to_string(),
            Role::Developer,
            "test-session".to_string(),
            "4".to_string(),
        );

        expert_registry.register_expert(expert0).unwrap();
        expert_registry.register_expert(expert1).unwrap();
        expert_registry.register_expert(expert2).unwrap();
        expert_registry.register_expert(expert3).unwrap();
        expert_registry.register_expert(expert4).unwrap();

        // Set all experts to idle
        for id in 0..5 {
            expert_registry
                .update_expert_state(id, ExpertState::Idle)
                .unwrap();
        }

        let router = MessageRouter::new(queue_manager, expert_registry, MockTmuxSender);
        (router, temp_dir)
    }

    // Property 1: worktree_matches returns correct results for all affinity matrix combinations
    #[tokio::test]
    async fn worktree_matches_none_none() {
        let (router, _temp) = create_worktree_router().await;
        // Expert 0 (None) and Expert 4 (None)
        assert!(
            router.worktree_matches(0, 4),
            "worktree_matches: (None, None) should be true"
        );
    }

    #[tokio::test]
    async fn worktree_matches_some_same() {
        let (router, _temp) = create_worktree_router().await;
        // Expert 1 and Expert 2 both in /wt/feature-auth
        assert!(
            router.worktree_matches(1, 2),
            "worktree_matches: (Some(X), Some(X)) should be true"
        );
    }

    #[tokio::test]
    async fn worktree_matches_none_some() {
        let (router, _temp) = create_worktree_router().await;
        // Expert 0 (None) vs Expert 1 (Some)
        assert!(
            !router.worktree_matches(0, 1),
            "worktree_matches: (None, Some(X)) should be false"
        );
    }

    #[tokio::test]
    async fn worktree_matches_some_none() {
        let (router, _temp) = create_worktree_router().await;
        // Expert 1 (Some) vs Expert 0 (None)
        assert!(
            !router.worktree_matches(1, 0),
            "worktree_matches: (Some(X), None) should be false"
        );
    }

    #[tokio::test]
    async fn worktree_matches_different_worktrees() {
        let (router, _temp) = create_worktree_router().await;
        // Expert 1 (/wt/feature-auth) vs Expert 3 (/wt/feature-payments)
        assert!(
            !router.worktree_matches(1, 3),
            "worktree_matches: (Some(X), Some(Y)) should be false"
        );
    }

    #[tokio::test]
    async fn worktree_matches_nonexistent_sender() {
        let (router, _temp) = create_worktree_router().await;
        assert!(
            !router.worktree_matches(999, 0),
            "worktree_matches: nonexistent sender should return false"
        );
    }

    #[tokio::test]
    async fn worktree_matches_nonexistent_recipient() {
        let (router, _temp) = create_worktree_router().await;
        assert!(
            !router.worktree_matches(0, 999),
            "worktree_matches: nonexistent recipient should return false"
        );
    }

    // Property 4: ID/Name Targeting Enforcement
    #[tokio::test]
    async fn find_recipient_by_id_worktree_mismatch_returns_none() {
        let (router, _temp) = create_worktree_router().await;
        // Expert 0 (main) tries to reach Expert 1 (feature-auth) by ID
        let recipient = MessageRecipient::expert_id(1);
        let result = router.find_recipient(&recipient, 0).await.unwrap();
        assert_eq!(
            result, None,
            "find_recipient: ID targeting across worktrees should return None"
        );
    }

    #[tokio::test]
    async fn find_recipient_by_name_worktree_mismatch_returns_none() {
        let (router, _temp) = create_worktree_router().await;
        // Expert 0 (main) tries to reach "auth-dev" (feature-auth) by name
        let recipient = MessageRecipient::expert_name("auth-dev".to_string());
        let result = router.find_recipient(&recipient, 0).await.unwrap();
        assert_eq!(
            result, None,
            "find_recipient: name targeting across worktrees should return None"
        );
    }

    // Property 3: Role Scoping
    #[tokio::test]
    async fn find_recipient_by_role_returns_only_same_worktree() {
        let (router, _temp) = create_worktree_router().await;
        // Expert 1 (feature-auth) targets role "reviewer" -> should find Expert 2 (feature-auth)
        let recipient = MessageRecipient::role("reviewer".to_string());
        let result = router.find_recipient(&recipient, 1).await.unwrap();
        assert_eq!(
            result,
            Some(2),
            "find_recipient: role targeting should find expert in same worktree"
        );
    }

    #[tokio::test]
    async fn find_recipient_by_role_different_worktree_returns_none() {
        let (router, _temp) = create_worktree_router().await;
        // Expert 3 (feature-payments) targets role "reviewer" -> no reviewer in feature-payments
        let recipient = MessageRecipient::role("reviewer".to_string());
        let result = router.find_recipient(&recipient, 3).await.unwrap();
        assert_eq!(
            result, None,
            "find_recipient: role targeting should not find expert in different worktree"
        );
    }

    #[tokio::test]
    async fn find_recipient_by_role_main_repo_only_finds_main_repo() {
        let (router, _temp) = create_worktree_router().await;
        // Expert 0 (main) targets role "developer" -> should find Expert 4 (main), not 1 or 3
        let recipient = MessageRecipient::role("developer".to_string());
        let result = router.find_recipient(&recipient, 0).await.unwrap();
        assert_eq!(
            result,
            Some(4),
            "find_recipient: main repo role targeting should only find main repo experts"
        );
    }

    // Property 2: Main Repo Affinity
    #[tokio::test]
    async fn main_repo_experts_can_find_each_other_by_id() {
        let (router, _temp) = create_worktree_router().await;
        let recipient = MessageRecipient::expert_id(4);
        let result = router.find_recipient(&recipient, 0).await.unwrap();
        assert_eq!(
            result,
            Some(4),
            "find_recipient: main repo experts should find each other by ID"
        );
    }

    #[tokio::test]
    async fn main_repo_experts_can_find_each_other_by_name() {
        let (router, _temp) = create_worktree_router().await;
        let recipient = MessageRecipient::expert_name("main-dev".to_string());
        let result = router.find_recipient(&recipient, 0).await.unwrap();
        assert_eq!(
            result,
            Some(4),
            "find_recipient: main repo experts should find each other by name"
        );
    }

    // Property 7: Retry Semantics Preserved
    #[tokio::test]
    async fn attempt_delivery_worktree_mismatch_produces_failed_result() {
        let (mut router, _temp) = create_worktree_router().await;

        // Expert 0 (main) sends to Expert 1 (feature-auth) by ID
        let content = MessageContent {
            subject: "Cross-worktree".to_string(),
            body: "Should fail".to_string(),
        };
        let message = Message::new(
            0,
            MessageRecipient::expert_id(1),
            MessageType::Query,
            content,
        );
        let queued = QueuedMessage::new(message);
        let result = router.attempt_delivery(&queued).await.unwrap();

        assert!(
            !result.success,
            "attempt_delivery: cross-worktree delivery should fail"
        );
        assert!(
            result.error.is_some(),
            "attempt_delivery: failed result should have error message"
        );
    }

    // Same-worktree delivery proceeds past recipient finding
    #[tokio::test]
    async fn attempt_delivery_same_worktree_finds_recipient() {
        let (mut router, _temp) = create_worktree_router().await;

        // Expert 1 (feature-auth) sends to Expert 2 (feature-auth) by ID
        let content = MessageContent {
            subject: "Same worktree".to_string(),
            body: "Should find recipient".to_string(),
        };
        let message = Message::new(
            1,
            MessageRecipient::expert_id(2),
            MessageType::Query,
            content,
        );
        let queued = QueuedMessage::new(message);
        let result = router.attempt_delivery(&queued).await.unwrap();

        // Delivery may fail at tmux level in tests, but should find the recipient
        // (not fail with "No recipient found")
        if !result.success {
            let err = result.error.as_deref().unwrap_or("");
            assert!(
                !err.contains("No recipient found"),
                "attempt_delivery: same-worktree delivery should find recipient"
            );
        }
    }

    // End-to-end: process_queue with worktree isolation
    #[tokio::test]
    async fn process_queue_respects_worktree_isolation() {
        let (mut router, _temp) = create_worktree_router().await;

        // Enqueue cross-worktree message: Expert 0 (main) -> Expert 1 (feature-auth)
        let content = MessageContent {
            subject: "Cross-worktree via queue".to_string(),
            body: "Should not deliver".to_string(),
        };
        let msg = Message::new(
            0,
            MessageRecipient::expert_id(1),
            MessageType::Notify,
            content,
        );
        router.queue_manager_mut().enqueue(&msg).await.unwrap();

        let stats = router.process_queue().await.unwrap();
        assert_eq!(
            stats.messages_delivered, 0,
            "process_queue: cross-worktree message should not be delivered"
        );
        assert!(
            stats.messages_failed > 0,
            "process_queue: cross-worktree message should be marked as failed"
        );
    }
}

#[cfg(test)]
mod property_tests {
    use super::mock_tmux::MockTmuxSender;
    use super::*;
    use crate::models::{
        ExpertInfo, ExpertState, MessageContent, MessagePriority, MessageType, QueuedMessage, Role,
    };
    use proptest::prelude::*;
    use tempfile::TempDir;

    // Generators for property-based testing
    fn arbitrary_role() -> impl Strategy<Value = Role> {
        prop_oneof![
            Just(Role::Analyst),
            Just(Role::Developer),
            Just(Role::Reviewer),
            Just(Role::Coordinator),
            "[a-zA-Z0-9-]{1,20}".prop_map(Role::specialist),
        ]
    }

    fn arbitrary_expert_state() -> impl Strategy<Value = ExpertState> {
        prop_oneof![Just(ExpertState::Idle), Just(ExpertState::Busy),]
    }

    fn arbitrary_expert_info() -> impl Strategy<Value = ExpertInfo> {
        (
            "[a-zA-Z0-9-]{1,30}",
            arbitrary_role(),
            "[a-zA-Z0-9-]{1,20}",
            "[0-9]{1,2}",
        )
            .prop_map(|(name, role, session, pane)| {
                ExpertInfo::new(crate::experts::AUTO_ASSIGN_ID, name, role, session, pane)
            })
    }

    fn arbitrary_message_recipient() -> impl Strategy<Value = MessageRecipient> {
        prop_oneof![
            (1u32..100).prop_map(MessageRecipient::expert_id),
            "[a-zA-Z0-9-]{1,50}".prop_map(MessageRecipient::expert_name),
            "[a-zA-Z0-9-]{1,50}".prop_map(MessageRecipient::role),
        ]
    }

    fn arbitrary_message_content() -> impl Strategy<Value = MessageContent> {
        ("[a-zA-Z0-9 ]{1,100}", "[a-zA-Z0-9 \n]{1,1000}")
            .prop_map(|(subject, body)| MessageContent { subject, body })
    }

    fn arbitrary_message() -> impl Strategy<Value = Message> {
        (
            0u32..100,
            arbitrary_message_recipient(),
            arbitrary_message_content(),
        )
            .prop_map(|(from_expert_id, to, content)| {
                Message::new(from_expert_id, to, MessageType::Query, content)
            })
    }

    #[allow(dead_code)]
    fn arbitrary_message_priority() -> impl Strategy<Value = MessagePriority> {
        prop_oneof![
            Just(MessagePriority::Low),
            Just(MessagePriority::Normal),
            Just(MessagePriority::High),
        ]
    }

    async fn create_test_router_with_experts(
        experts: Vec<ExpertInfo>,
    ) -> (MessageRouter<MockTmuxSender>, TempDir, Vec<ExpertId>) {
        let temp_dir = TempDir::new().unwrap();
        let queue_manager = QueueManager::new(temp_dir.path().to_path_buf());
        queue_manager.init().await.unwrap();

        let mut expert_registry = ExpertRegistry::new();

        let mut expert_ids = Vec::new();

        // Register experts, handling duplicate names by making them unique
        for (index, mut expert) in experts.into_iter().enumerate() {
            expert.name = format!("{}-{}", expert.name, index);
            let expert_id = expert_registry.register_expert(expert).unwrap();
            expert_ids.push(expert_id);
        }

        let router = MessageRouter::new(queue_manager, expert_registry, MockTmuxSender);
        (router, temp_dir, expert_ids)
    }

    // Feature: inter-expert-messaging, Property 3: Recipient Targeting Accuracy
    // **Validates: Requirements 2.1, 2.2, 2.3, 2.4**
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn recipient_targeting_accuracy(
            experts in prop::collection::vec(arbitrary_expert_info(), 1..20),
            messages in prop::collection::vec(arbitrary_message(), 1..50)
        ) {
            tokio_test::block_on(async {
                let (mut router, _temp, expert_ids) = create_test_router_with_experts(experts).await;

                // Set some experts to idle for role-based targeting
                for (i, &expert_id) in expert_ids.iter().enumerate() {
                    let state = if i % 3 == 0 { ExpertState::Idle } else { ExpertState::Busy };
                    router.expert_registry_mut().update_expert_state(expert_id, state).unwrap();
                }

                // Use first expert as sender (all have worktree_path=None)
                let sender_id = expert_ids[0];

                for message in messages {
                    let result = router.find_recipient(&message.to, sender_id).await.unwrap();

                    match &message.to {
                        MessageRecipient::ExpertId { expert_id } => {
                            // Requirement 2.1: Message should deliver only to specific expert by ID
                            if expert_ids.contains(expert_id) {
                                assert_eq!(result, Some(*expert_id));
                            } else {
                                assert_eq!(result, None);
                            }
                        },
                        MessageRecipient::ExpertName { expert_name } => {
                            // Requirement 2.2: Message should deliver to expert with exact name
                            if let Some(found_id) = result {
                                let expert_info = router.expert_registry().get_expert(found_id).unwrap();
                                // Name matching should be case-insensitive but exact
                                assert!(expert_info.name.to_lowercase().contains(&expert_name.to_lowercase()));
                            }
                        },
                        MessageRecipient::Role { role } => {
                            // Requirements 2.3, 2.4: Message should deliver to idle expert with matching role
                            if let Some(found_id) = result {
                                let expert_info = router.expert_registry().get_expert(found_id).unwrap();
                                // Expert must have matching role
                                assert!(expert_info.role.matches(role));
                                // Expert must be idle (non-blocking delivery)
                                assert!(expert_info.is_idle());
                            }

                            // If no expert found, verify no idle experts exist with that role
                            if result.is_none() {
                                let idle_experts_for_role = router.expert_registry().get_idle_experts_by_role_str(role);
                                assert!(idle_experts_for_role.is_empty());
                            }
                        }
                    }
                }
            });
        }

        #[test]
        fn recipient_targeting_exclusivity(
            experts in prop::collection::vec(arbitrary_expert_info(), 2..10),
            target_expert_index in 0usize..9,
            other_expert_indices in prop::collection::vec(0usize..9, 1..5)
        ) {
            tokio_test::block_on(async {
                let (mut router, _temp, expert_ids) = create_test_router_with_experts(experts).await;

                if target_expert_index >= expert_ids.len() {
                    return; // Skip if index out of bounds
                }

                let target_expert_id = expert_ids[target_expert_index];
                let target_expert = router.expert_registry().get_expert(target_expert_id).unwrap().clone();

                // Set target expert to idle
                router.expert_registry_mut().update_expert_state(target_expert_id, ExpertState::Idle).unwrap();

                // Set other experts to busy
                for &other_index in &other_expert_indices {
                    if other_index < expert_ids.len() && other_index != target_expert_index {
                        let other_id = expert_ids[other_index];
                        router.expert_registry_mut().update_expert_state(other_id, ExpertState::Busy).unwrap();
                    }
                }

                // Use a different expert as sender (all have worktree_path=None)
                let sender_id = expert_ids[(target_expert_index + 1) % expert_ids.len()];

                // Test targeting by ID - should find exactly the target expert
                let by_id = MessageRecipient::expert_id(target_expert_id);
                let result = router.find_recipient(&by_id, sender_id).await.unwrap();
                assert_eq!(result, Some(target_expert_id));

                // Test targeting by name - should find exactly the target expert
                let by_name = MessageRecipient::expert_name(target_expert.name.clone());
                let result = router.find_recipient(&by_name, sender_id).await.unwrap();
                assert_eq!(result, Some(target_expert_id));

                // Test targeting by role - should find the target expert if it's the only idle one with that role
                let by_role = MessageRecipient::role(target_expert.role.as_str().to_string());
                let result = router.find_recipient(&by_role, sender_id).await.unwrap();

                // Should either find the target expert or no expert (if others with same role are also idle)
                if let Some(found_id) = result {
                    let found_expert = router.expert_registry().get_expert(found_id).unwrap();
                    assert!(found_expert.role.matches(target_expert.role.as_str()));
                    assert!(found_expert.is_idle());
                }
            });
        }

        #[test]
        fn recipient_targeting_consistency(
            experts in prop::collection::vec(arbitrary_expert_info(), 1..15),
            recipient in arbitrary_message_recipient()
        ) {
            tokio_test::block_on(async {
                let (mut router, _temp, expert_ids) = create_test_router_with_experts(experts).await;

                // Set random states for experts
                for (i, &expert_id) in expert_ids.iter().enumerate() {
                    let state = match i % 2 {
                        0 => ExpertState::Idle,
                        _ => ExpertState::Busy,
                    };
                    router.expert_registry_mut().update_expert_state(expert_id, state).unwrap();
                }

                // Use first expert as sender (all have worktree_path=None)
                let sender_id = expert_ids[0];

                // Multiple calls to find_recipient should return consistent results
                let result1 = router.find_recipient(&recipient, sender_id).await.unwrap();
                let result2 = router.find_recipient(&recipient, sender_id).await.unwrap();
                let result3 = router.find_recipient(&recipient, sender_id).await.unwrap();

                assert_eq!(result1, result2);
                assert_eq!(result2, result3);

                // If a recipient is found, it should match the targeting criteria
                if let Some(found_id) = result1 {
                    let expert_info = router.expert_registry().get_expert(found_id).unwrap();

                    match &recipient {
                        MessageRecipient::ExpertId { expert_id } => {
                            assert_eq!(found_id, *expert_id);
                        },
                        MessageRecipient::ExpertName { expert_name } => {
                            assert!(expert_info.matches_name(expert_name));
                        },
                        MessageRecipient::Role { role } => {
                            assert!(expert_info.matches_role(role));
                            assert!(expert_info.is_idle()); // Role-based targeting requires idle state
                        }
                    }
                }
            });
        }

        // Feature: inter-expert-messaging, Property 4: Non-Blocking Delivery Enforcement
        // **Validates: Requirements 3.1, 3.2, 3.3**
        #[test]
        fn non_blocking_delivery_enforcement(
            experts in prop::collection::vec(arbitrary_expert_info(), 1..10),
            messages in prop::collection::vec(arbitrary_message(), 1..20),
            expert_states in prop::collection::vec(arbitrary_expert_state(), 1..10)
        ) {
            tokio_test::block_on(async {
                let (mut router, _temp, expert_ids) = create_test_router_with_experts(experts).await;

                // Apply random states to experts, ensuring we have a mix of idle and non-idle
                for (expert_id, state) in expert_ids.iter().zip(expert_states.iter()) {
                    router.expert_registry_mut().update_expert_state(*expert_id, state.clone()).unwrap();
                }

                for message in messages {
                    // Create a queued message for delivery attempt
                    let queued_message = QueuedMessage::new(message.clone());

                    // Attempt delivery
                    let delivery_result = router.attempt_delivery(&queued_message).await.unwrap();

                    // Find the target expert for this message (use same sender_id as attempt_delivery)
                    let target_expert_id = router.find_recipient(&message.to, message.from_expert_id).await.unwrap();

                    if let Some(expert_id) = target_expert_id {
                        let expert_info = router.expert_registry().get_expert(expert_id).unwrap();

                        // Requirement 3.1: Message should only be delivered when target expert is idle
                        // Requirement 3.2: Delivery should be skipped when expert is busy
                        if expert_info.is_idle() {
                            // Expert is idle - delivery should succeed (assuming no tmux errors)
                            // Note: In our test environment, tmux delivery will fail, but the logic should
                            // reach the tmux delivery attempt, not be blocked by idle state check
                            if !delivery_result.success {
                                // If delivery failed, it should be due to tmux error, not idle state
                                if let Some(error) = &delivery_result.error {
                                    assert!(error.contains("Tmux") || error.contains("tmux") ||
                                           error.contains("Failed to send message") ||
                                           error.contains("pane"));
                                }
                            }
                        } else {
                            // Expert is not idle (busy or offline) - delivery should be skipped
                            // Requirement 3.3: Delivery should be skipped with retry scheduled
                            assert!(!delivery_result.success);
                            if let Some(error) = &delivery_result.error {
                                assert!(error.contains("not idle") || error.contains("is not idle"));
                            }
                        }

                        // Verify that the delivery decision is consistent with expert state
                        let is_expert_idle = router.is_expert_idle(expert_id).await.unwrap();
                        assert_eq!(is_expert_idle, expert_info.is_idle());

                        // If expert is not idle, delivery should definitely fail
                        if !is_expert_idle {
                            assert!(!delivery_result.success);
                        }
                    } else {
                        // No target expert found - delivery should fail
                        assert!(!delivery_result.success);
                        if let Some(error) = &delivery_result.error {
                            assert!(error.contains("No recipient found") ||
                                   error.contains("not found"));
                        }
                    }
                }
            });
        }

        #[test]
        fn non_blocking_delivery_state_transitions(
            expert in arbitrary_expert_info(),
            message in arbitrary_message(),
            state_sequence in prop::collection::vec(arbitrary_expert_state(), 2..10)
        ) {
            tokio_test::block_on(async {
                let (mut router, _temp, expert_ids) = create_test_router_with_experts(vec![expert]).await;
                let expert_id = expert_ids[0];

                // Create a message targeting this specific expert (sender must be registered for worktree check)
                let mut test_message = message;
                test_message.from_expert_id = expert_id;
                test_message.to = MessageRecipient::expert_id(expert_id);
                let queued_message = QueuedMessage::new(test_message);

                // Test delivery behavior across different state transitions
                for state in state_sequence {
                    // Update expert state
                    router.expert_registry_mut().update_expert_state(expert_id, state.clone()).unwrap();

                    // Attempt delivery
                    let delivery_result = router.attempt_delivery(&queued_message).await.unwrap();

                    // Verify non-blocking delivery enforcement
                    match state {
                        ExpertState::Idle => {
                            // When idle, delivery should proceed to tmux attempt
                            // (may fail due to tmux in test environment, but should not be blocked by state)
                            if !delivery_result.success {
                                if let Some(error) = &delivery_result.error {
                                    // Error should be tmux-related, not state-related
                                    assert!(!error.contains("not idle"));
                                }
                            }
                        },
                        ExpertState::Busy => {
                            // When not idle, delivery should be blocked
                            assert!(!delivery_result.success);
                            if let Some(error) = &delivery_result.error {
                                assert!(error.contains("not idle"));
                            }
                        }
                    }

                    // Verify state consistency
                    let is_idle = router.is_expert_idle(expert_id).await.unwrap();
                    assert_eq!(is_idle, matches!(state, ExpertState::Idle));
                }
            });
        }

        #[test]
        fn non_blocking_delivery_retry_scheduling(
            experts in prop::collection::vec(arbitrary_expert_info(), 2..5),
            messages in prop::collection::vec(arbitrary_message(), 1..10)
        ) {
            tokio_test::block_on(async {
                let (mut router, _temp, expert_ids) = create_test_router_with_experts(experts).await;

                // Set all experts to busy initially
                for &expert_id in &expert_ids {
                    router.expert_registry_mut().update_expert_state(expert_id, ExpertState::Busy).unwrap();
                }

                for message in messages {
                    let queued_message = QueuedMessage::new(message.clone());

                    // First delivery attempt - should fail due to busy state
                    let first_result = router.attempt_delivery(&queued_message).await.unwrap();
                    assert!(!first_result.success);

                    // Find target expert and set to idle (use same sender_id as attempt_delivery)
                    if let Some(target_expert_id) = router.find_recipient(&message.to, message.from_expert_id).await.unwrap() {
                        router.expert_registry_mut().update_expert_state(target_expert_id, ExpertState::Idle).unwrap();

                        // Second delivery attempt - should now proceed (may fail at tmux level)
                        let second_result = router.attempt_delivery(&queued_message).await.unwrap();

                        // Should not be blocked by idle state anymore
                        if !second_result.success {
                            if let Some(error) = &second_result.error {
                                // Error should not be about idle state
                                assert!(!error.contains("not idle"));
                            }
                        }

                        // Set back to busy
                        router.expert_registry_mut().update_expert_state(target_expert_id, ExpertState::Busy).unwrap();

                        // Third delivery attempt - should fail again due to busy state
                        let third_result = router.attempt_delivery(&queued_message).await.unwrap();
                        assert!(!third_result.success);
                        if let Some(error) = &third_result.error {
                            assert!(error.contains("not idle"));
                        }
                    }
                }
            });
        }

        // Feature: inter-expert-messaging, Property 6: Priority-Based Ordering
        // **Validates: Requirements 6.1, 6.2, 6.3, 6.4**
        #[test]
        fn priority_based_ordering(
            messages in prop::collection::vec(arbitrary_message(), 2..20),
            recipient_expert_id in 1u32..10
        ) {
            tokio_test::block_on(async {
                let (mut router, _temp, _expert_ids) = create_test_router_with_experts(vec![
                    ExpertInfo::new(recipient_expert_id, "test-expert".to_string(), Role::Developer, "test-session".to_string(), "0".to_string())
                ]).await;

                // Set the expert to idle so messages can be delivered
                router.expert_registry_mut().update_expert_state(recipient_expert_id, ExpertState::Idle).unwrap();

                // Create messages with different priorities targeting the same recipient
                let mut test_messages = Vec::new();
                let base_time = chrono::Utc::now();
                for (i, mut message) in messages.into_iter().enumerate() {
                    message.to = MessageRecipient::expert_id(recipient_expert_id);

                    let priority = match i % 3 {
                        0 => MessagePriority::High,
                        1 => MessagePriority::Normal,
                        _ => MessagePriority::Low,
                    };

                    let content = message.content.clone();
                    let mut new_message = Message::new(message.from_expert_id, message.to.clone(), message.message_type, content)
                        .with_priority(priority);
                    // Ensure unique IDs and monotonic timestamps without sleeping
                    new_message.message_id = format!("msg-test-{:04}", i);
                    new_message.created_at = base_time + chrono::Duration::milliseconds(i as i64);

                    test_messages.push(new_message);
                }

                // Enqueue all messages
                for message in &test_messages {
                    router.queue_manager_mut().enqueue(message).await.unwrap();
                }

                // Get pending messages - should be sorted by priority then FIFO
                let pending_messages = router.queue_manager().get_pending_messages().await.unwrap();

                // Skip test if no messages were enqueued (edge case)
                if pending_messages.is_empty() {
                    return;
                }

                // Verify priority ordering: High > Normal > Low
                // Within same priority: FIFO (earlier created_at first)
                for i in 1..pending_messages.len() {
                    let prev = &pending_messages[i - 1];
                    let curr = &pending_messages[i];

                    // Requirement 6.2: Higher priority messages should be delivered before lower priority
                    if prev.message.priority != curr.message.priority {
                        assert!(
                            prev.message.priority > curr.message.priority,
                            "Messages should be ordered by priority (High > Normal > Low). \
                            Previous: {:?}, Current: {:?}",
                            prev.message.priority, curr.message.priority
                        );
                    } else {
                        // Requirement 6.3: Messages with same priority should be delivered in FIFO order
                        assert!(
                            prev.message.created_at <= curr.message.created_at,
                            "Messages with same priority should be in FIFO order. \
                            Previous: {:?} at {:?}, Current: {:?} at {:?}",
                            prev.message.priority, prev.message.created_at,
                            curr.message.priority, curr.message.created_at
                        );
                    }
                }

                // Verify that all high priority messages come before normal priority messages
                let high_priority_count = pending_messages.iter()
                    .filter(|msg| msg.message.priority == MessagePriority::High)
                    .count();
                let normal_priority_count = pending_messages.iter()
                    .filter(|msg| msg.message.priority == MessagePriority::Normal)
                    .count();
                let low_priority_count = pending_messages.iter()
                    .filter(|msg| msg.message.priority == MessagePriority::Low)
                    .count();

                // Verify priority grouping
                if high_priority_count > 0 {
                    for msg in pending_messages.iter().take(high_priority_count) {
                        assert_eq!(
                            msg.message.priority,
                            MessagePriority::High,
                            "High priority messages should come first"
                        );
                    }
                }

                if normal_priority_count > 0 {
                    for msg in pending_messages.iter().skip(high_priority_count).take(normal_priority_count) {
                        assert_eq!(
                            msg.message.priority,
                            MessagePriority::Normal,
                            "Normal priority messages should come after high priority"
                        );
                    }
                }

                if low_priority_count > 0 {
                    for msg in pending_messages.iter().skip(high_priority_count + normal_priority_count) {
                        assert_eq!(
                            msg.message.priority,
                            MessagePriority::Low,
                            "Low priority messages should come last"
                        );
                    }
                }

                // Test delivery order by processing the queue
                let _processing_stats = router.process_queue().await.unwrap();

                // Since tmux delivery will fail in test environment, we need to check the order
                // in which delivery was attempted by examining the queue processing
                // The queue manager should have attempted delivery in priority order

                // Verify that the queue processing attempted messages in correct order
                // by checking that higher priority messages were processed first
                let remaining_messages = router.queue_manager().get_pending_messages().await.unwrap();

                // In a real scenario, high priority messages would be delivered first
                // In our test environment, all deliveries fail due to tmux, but the order should be maintained
                // We can verify this by checking that the queue still maintains priority order
                for i in 1..remaining_messages.len() {
                    let prev = &remaining_messages[i - 1];
                    let curr = &remaining_messages[i];

                    if prev.message.priority != curr.message.priority {
                        assert!(
                            prev.message.priority > curr.message.priority,
                            "Queue should maintain priority ordering even after processing attempts"
                        );
                    } else {
                        assert!(
                            prev.message.created_at <= curr.message.created_at,
                            "Queue should maintain FIFO ordering within same priority"
                        );
                    }
                }
            });
        }

        // Feature: inter-expert-messaging, Property 10: Message Format Standardization
        // **Validates: Requirements 8.2, 8.4**
        #[test]
        fn message_format_standardization(
            message in arbitrary_message(),
            recipient_name in "[a-zA-Z0-9-]{1,30}"
        ) {
            tokio_test::block_on(async {
                let (router, _temp, _expert_ids) = create_test_router_with_experts(vec![]).await;

                // Format message for delivery
                let formatted = router.format_message_for_delivery(&message, &recipient_name);

                // Requirement 8.2: Message format must contain all required fields
                // Verify message header is present
                assert!(
                    formatted.contains("INCOMING MESSAGE"),
                    "Formatted message should contain header"
                );

                // Verify priority indicator is present
                let priority_str = match message.priority {
                    MessagePriority::High => "HIGH",
                    MessagePriority::Normal => "NORMAL",
                    MessagePriority::Low => "LOW",
                };
                assert!(
                    formatted.contains(priority_str),
                    "Formatted message should contain priority: {}", priority_str
                );

                // Verify message type indicator is present
                let type_str = match message.message_type {
                    MessageType::Query => "QUERY",
                    MessageType::Response => "RESPONSE",
                    MessageType::Notify => "NOTIFICATION",
                    MessageType::Delegate => "TASK_DELEGATION",
                };
                assert!(
                    formatted.contains(type_str),
                    "Formatted message should contain message type: {}", type_str
                );

                // Verify sender info is present
                assert!(
                    formatted.contains(&format!("Expert {}", message.from_expert_id)),
                    "Formatted message should contain sender expert ID"
                );

                // Verify recipient name is present
                assert!(
                    formatted.contains(&recipient_name),
                    "Formatted message should contain recipient name"
                );

                // Verify subject is present
                assert!(
                    formatted.contains(&message.content.subject),
                    "Formatted message should contain subject"
                );

                // Verify body is present
                assert!(
                    formatted.contains(&message.content.body),
                    "Formatted message should contain body"
                );

                // Verify message ID is present
                assert!(
                    formatted.contains(&message.message_id),
                    "Formatted message should contain message ID"
                );

                // Verify timestamp is present (format: YYYY-MM-DD HH:MM:SS UTC)
                assert!(
                    formatted.contains("UTC"),
                    "Formatted message should contain timestamp with UTC"
                );

                // Requirement 8.4: Reply-to field should be included when present
                if let Some(reply_to) = &message.reply_to {
                    assert!(
                        formatted.contains(reply_to),
                        "Formatted message should contain reply_to when present"
                    );
                    assert!(
                        formatted.contains("Reply to:"),
                        "Formatted message should have 'Reply to:' label when reply_to is set"
                    );
                }
            });
        }

        #[test]
        fn message_format_consistency_across_types(
            from_expert_id in 0u32..10,
            recipient_name in "[a-zA-Z0-9-]{1,20}",
            subject in "[a-zA-Z0-9 ]{1,50}",
            body in "[a-zA-Z0-9 \n]{1,200}"
        ) {
            tokio_test::block_on(async {
                let (router, _temp, _expert_ids) = create_test_router_with_experts(vec![]).await;

                let content = MessageContent {
                    subject: subject.clone(),
                    body: body.clone(),
                };

                // Test all message types
                let message_types = [
                    MessageType::Query,
                    MessageType::Response,
                    MessageType::Notify,
                    MessageType::Delegate,
                ];

                for msg_type in message_types {
                    let message = Message::new(
                        from_expert_id,
                        MessageRecipient::expert_id(1),
                        msg_type,
                        content.clone(),
                    );

                    let formatted = router.format_message_for_delivery(&message, &recipient_name);

                    // All message types should have consistent structure
                    assert!(formatted.contains("INCOMING MESSAGE"), "Header should be present for {:?}", msg_type);
                    assert!(formatted.contains("From:"), "From field should be present for {:?}", msg_type);
                    assert!(formatted.contains("To:"), "To field should be present for {:?}", msg_type);
                    assert!(formatted.contains("Type:"), "Type field should be present for {:?}", msg_type);
                    assert!(formatted.contains("Priority:"), "Priority field should be present for {:?}", msg_type);
                    assert!(formatted.contains("Subject:"), "Subject field should be present for {:?}", msg_type);
                    assert!(formatted.contains("Message ID:"), "Message ID field should be present for {:?}", msg_type);
                    assert!(formatted.contains("Timestamp:"), "Timestamp field should be present for {:?}", msg_type);
                }
            });
        }

        #[test]
        fn message_format_preserves_special_characters(
            from_expert_id in 0u32..10,
            recipient_name in "[a-zA-Z0-9-]{1,20}"
        ) {
            tokio_test::block_on(async {
                let (router, _temp, _expert_ids) = create_test_router_with_experts(vec![]).await;

                // Test with special characters in content
                let special_content = MessageContent {
                    subject: "Test: Special Characters [Test]".to_string(),
                    body: "Body with special chars: <>&\"'\\n\ttab".to_string(),
                };

                let message = Message::new(
                    from_expert_id,
                    MessageRecipient::expert_id(1),
                    MessageType::Query,
                    special_content.clone(),
                );

                let formatted = router.format_message_for_delivery(&message, &recipient_name);

                // Special characters should be preserved
                assert!(
                    formatted.contains(&special_content.subject),
                    "Subject with special characters should be preserved"
                );
                assert!(
                    formatted.contains(&special_content.body),
                    "Body with special characters should be preserved"
                );
            });
        }
    }
}

/// Integration tests for end-to-end message flow
/// Feature: inter-expert-messaging, Task 13.2
/// Validates: Requirements 1.1, 3.4, 4.2, 7.2
#[cfg(test)]
mod integration_tests {
    use super::mock_tmux::MockTmuxSender;
    use super::*;
    use crate::models::{
        ExpertInfo, ExpertState, MessageContent, MessagePriority, MessageType, Role,
    };
    use tempfile::TempDir;
    use tokio::fs;

    async fn create_integration_test_router(
        num_experts: usize,
    ) -> (MessageRouter<MockTmuxSender>, TempDir, Vec<ExpertId>) {
        let temp_dir = TempDir::new().unwrap();
        let queue_manager = QueueManager::new(temp_dir.path().to_path_buf());
        queue_manager.init().await.unwrap();

        let mut expert_registry = ExpertRegistry::new();

        let mut expert_ids = Vec::new();
        for i in 0..num_experts {
            let expert = ExpertInfo::new(
                (i + 1) as u32,
                format!("expert-{}", i),
                Role::specialist(format!("role-{}", i % 3)),
                "test-session".to_string(),
                i.to_string(),
            );
            let expert_id = expert_registry.register_expert(expert).unwrap();
            expert_ids.push(expert_id);
        }

        let router = MessageRouter::new(queue_manager, expert_registry, MockTmuxSender);
        (router, temp_dir, expert_ids)
    }

    /// Test complete message flow from expert outbox to queue processing
    /// Validates: Requirements 1.1, 4.2
    #[tokio::test]
    async fn integration_complete_message_flow_outbox_to_queue() {
        let (mut router, temp_dir, expert_ids) = create_integration_test_router(3).await;

        // Set expert 1 to idle for message delivery
        router
            .expert_registry_mut()
            .update_expert_state(expert_ids[1], ExpertState::Idle)
            .unwrap();

        // Create message file in outbox (simulating expert writing a message)
        let outbox_path = temp_dir.path().join("messages").join("outbox");
        let content = MessageContent {
            subject: "Integration Test".to_string(),
            body: "Testing end-to-end message flow".to_string(),
        };
        let message = Message::new(
            expert_ids[0],
            MessageRecipient::expert_id(expert_ids[1]),
            MessageType::Query,
            content,
        );

        let message_file = outbox_path.join(format!("{}.yaml", message.message_id));
        let yaml_content = serde_yaml::to_string(&message).unwrap();
        fs::write(&message_file, yaml_content).await.unwrap();

        // Process outbox - message should be moved to queue
        let processed = router.process_outbox().await.unwrap();
        assert_eq!(
            processed.len(),
            1,
            "One message should be processed from outbox"
        );
        assert_eq!(processed[0], message.message_id);

        // Message should now be in the queue
        let queue_stats = router.get_queue_stats().await.unwrap();
        assert_eq!(queue_stats.total_messages, 1, "Message should be in queue");
        assert_eq!(queue_stats.pending_messages, 1, "Message should be pending");

        // Outbox file should be removed
        assert!(
            !message_file.exists(),
            "Outbox file should be removed after processing"
        );
    }

    /// Test message persistence across simulated restart
    /// Validates: Requirements 4.2, 7.2
    #[tokio::test]
    async fn integration_message_persistence_across_restart() {
        let temp_dir = TempDir::new().unwrap();
        let queue_path = temp_dir.path().to_path_buf();

        // Phase 1: Create router and enqueue message
        {
            let queue_manager = QueueManager::new(queue_path.clone());
            queue_manager.init().await.unwrap();

            let mut expert_registry = ExpertRegistry::new();

            let expert = ExpertInfo::new(
                1,
                "test-expert".to_string(),
                Role::Developer,
                "test-session".to_string(),
                "0".to_string(),
            );
            expert_registry.register_expert(expert).unwrap();

            let mut router = MessageRouter::new(queue_manager, expert_registry, MockTmuxSender);

            // Enqueue a message
            let content = MessageContent {
                subject: "Persistence Test".to_string(),
                body: "This message should persist".to_string(),
            };
            let message = Message::new(
                1,
                MessageRecipient::expert_id(1),
                MessageType::Notify,
                content,
            );

            router.queue_manager_mut().enqueue(&message).await.unwrap();

            // Verify message is in queue
            let stats = router.get_queue_stats().await.unwrap();
            assert_eq!(stats.total_messages, 1);
        }
        // Router is dropped here, simulating shutdown

        // Phase 2: Create new router and verify message persists
        {
            let queue_manager = QueueManager::new(queue_path);
            // Don't call init() to simulate restart with existing data

            let mut expert_registry = ExpertRegistry::new();

            let expert = ExpertInfo::new(
                1,
                "test-expert".to_string(),
                Role::Developer,
                "test-session".to_string(),
                "0".to_string(),
            );
            expert_registry.register_expert(expert).unwrap();

            let router = MessageRouter::new(queue_manager, expert_registry, MockTmuxSender);

            // Message should still be in queue
            let stats = router.get_queue_stats().await.unwrap();
            assert_eq!(
                stats.total_messages, 1,
                "Message should persist after restart"
            );

            // Verify message content is intact
            let messages = router.queue_manager().get_pending_messages().await.unwrap();
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].message.content.subject, "Persistence Test");
            assert_eq!(
                messages[0].message.content.body,
                "This message should persist"
            );
        }
    }

    /// Test concurrent messaging with multiple experts
    /// Validates: Requirements 1.1, 3.4
    #[tokio::test]
    async fn integration_concurrent_messaging_multiple_experts() {
        let (mut router, _temp_dir, expert_ids) = create_integration_test_router(5).await;

        // Set all experts to idle
        for &expert_id in &expert_ids {
            router
                .expert_registry_mut()
                .update_expert_state(expert_id, ExpertState::Idle)
                .unwrap();
        }

        // Create messages from different experts to different recipients
        let mut message_ids = Vec::new();
        for i in 0..expert_ids.len() {
            let from_expert = expert_ids[i];
            let to_expert = expert_ids[(i + 1) % expert_ids.len()];

            let content = MessageContent {
                subject: format!("Message from expert {}", i),
                body: format!("Concurrent test message #{}", i),
            };
            let message = Message::new(
                from_expert,
                MessageRecipient::expert_id(to_expert),
                MessageType::Notify,
                content,
            );

            message_ids.push(message.message_id.clone());
            router.queue_manager_mut().enqueue(&message).await.unwrap();

            // Small delay to ensure unique timestamps
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        }

        // Verify all messages are in queue
        let stats = router.get_queue_stats().await.unwrap();
        assert_eq!(
            stats.total_messages,
            expert_ids.len(),
            "All messages should be in queue"
        );

        // Process the queue (delivery will fail due to tmux in test, but logic should work)
        let processing_stats = router.process_queue().await.unwrap();
        assert_eq!(
            processing_stats.messages_processed,
            expert_ids.len(),
            "All messages should be processed"
        );
    }

    /// Test message delivery attempt with idle/busy state transitions
    /// Validates: Requirements 3.4, 7.2
    #[tokio::test]
    async fn integration_delivery_with_state_transitions() {
        let (mut router, _temp_dir, expert_ids) = create_integration_test_router(2).await;

        let sender_id = expert_ids[0];
        let recipient_id = expert_ids[1];

        // Create a message
        let content = MessageContent {
            subject: "State Transition Test".to_string(),
            body: "Testing delivery with state changes".to_string(),
        };
        let message = Message::new(
            sender_id,
            MessageRecipient::expert_id(recipient_id),
            MessageType::Query,
            content,
        );

        // Set recipient to busy first
        router
            .expert_registry_mut()
            .update_expert_state(recipient_id, ExpertState::Busy)
            .unwrap();

        router.queue_manager_mut().enqueue(&message).await.unwrap();

        // First attempt: recipient is busy
        let stats1 = router.process_queue().await.unwrap();
        assert_eq!(
            stats1.messages_delivered, 0,
            "Should not deliver to busy expert"
        );

        // Message should still be in queue
        let queue_stats = router.get_queue_stats().await.unwrap();
        assert!(queue_stats.total_messages >= 1 || stats1.messages_failed >= 1);

        // Set recipient to idle
        router
            .expert_registry_mut()
            .update_expert_state(recipient_id, ExpertState::Idle)
            .unwrap();

        // Recipient is idle (delivery will fail due to tmux in test)
        // But the state check should pass
        let is_idle = router.expert_registry().is_expert_idle(recipient_id);
        assert_eq!(is_idle, Some(true), "Recipient should be idle");

        // Set recipient to busy
        router
            .expert_registry_mut()
            .update_expert_state(recipient_id, ExpertState::Busy)
            .unwrap();

        // Recipient is busy again
        let is_idle_now = router.expert_registry().is_expert_idle(recipient_id);
        assert_eq!(is_idle_now, Some(false), "Recipient should be busy");
    }

    /// Test priority-based message ordering in concurrent scenario
    /// Validates: Requirements 6.1, 6.2, 6.3
    #[tokio::test]
    async fn integration_priority_ordering_in_concurrent_messages() {
        let (mut router, _temp_dir, expert_ids) = create_integration_test_router(2).await;

        let sender_id = expert_ids[0];
        let recipient_id = expert_ids[1];

        // Create messages with different priorities
        let priorities = [
            (MessagePriority::Low, "Low Priority"),
            (MessagePriority::Normal, "Normal Priority"),
            (MessagePriority::High, "High Priority"),
        ];

        for (priority, subject) in priorities.iter() {
            let content = MessageContent {
                subject: subject.to_string(),
                body: format!("Message with {} priority", subject),
            };
            let message = Message::new(
                sender_id,
                MessageRecipient::expert_id(recipient_id),
                MessageType::Notify,
                content,
            )
            .with_priority(*priority);

            router.queue_manager_mut().enqueue(&message).await.unwrap();
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        }

        // Get pending messages - should be ordered by priority
        let pending = router.queue_manager().get_pending_messages().await.unwrap();
        assert_eq!(pending.len(), 3);

        // High priority should be first
        assert_eq!(pending[0].message.priority, MessagePriority::High);
        assert_eq!(pending[1].message.priority, MessagePriority::Normal);
        assert_eq!(pending[2].message.priority, MessagePriority::Low);
    }

    /// Test message cleanup and expiration in end-to-end flow
    /// Validates: Requirements 5.1, 5.2
    #[tokio::test]
    async fn integration_message_cleanup_end_to_end() {
        let (mut router, _temp_dir, expert_ids) = create_integration_test_router(2).await;

        let sender_id = expert_ids[0];
        let recipient_id = expert_ids[1];

        // Create expired message with TTL = 0 (same as unit test approach)
        let expired_content = MessageContent {
            subject: "Expired Message".to_string(),
            body: "This should be cleaned up".to_string(),
        };
        let expired_message = Message::new(
            sender_id,
            MessageRecipient::expert_id(recipient_id),
            MessageType::Notify,
            expired_content,
        )
        .with_ttl_seconds(0);

        // Small delay after creating expired message
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;

        // Create valid message
        let valid_content = MessageContent {
            subject: "Valid Message".to_string(),
            body: "This should remain".to_string(),
        };
        let valid_message = Message::new(
            sender_id,
            MessageRecipient::expert_id(recipient_id),
            MessageType::Notify,
            valid_content,
        );

        router
            .queue_manager_mut()
            .enqueue(&expired_message)
            .await
            .unwrap();
        router
            .queue_manager_mut()
            .enqueue(&valid_message)
            .await
            .unwrap();

        // Delay to ensure expiration is detected (same as unit test)
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Process queue - should clean up expired messages as part of processing
        let stats = router.process_queue().await.unwrap();
        assert!(
            stats.messages_expired >= 1,
            "At least one message should expire. Got stats: {:?}",
            stats
        );

        // Only valid message should remain (or be processed)
        let remaining = router.queue_manager().read_queue().await.unwrap();
        assert!(
            remaining.is_empty()
                || remaining
                    .iter()
                    .all(|m| m.message.content.subject == "Valid Message"),
            "Only valid messages should remain"
        );
    }

    /// Test role-based message routing with multiple experts
    /// Validates: Requirements 2.3, 2.4
    #[tokio::test]
    async fn integration_role_based_routing() {
        let temp_dir = TempDir::new().unwrap();
        let queue_manager = QueueManager::new(temp_dir.path().to_path_buf());
        queue_manager.init().await.unwrap();

        let mut expert_registry = ExpertRegistry::new();

        // Create experts with specific roles
        let backend_expert1 = ExpertInfo::new(
            1,
            "backend-1".to_string(),
            Role::specialist("backend"),
            "test-session".to_string(),
            "0".to_string(),
        );
        let backend_expert2 = ExpertInfo::new(
            2,
            "backend-2".to_string(),
            Role::specialist("backend"),
            "test-session".to_string(),
            "1".to_string(),
        );
        let frontend_expert = ExpertInfo::new(
            3,
            "frontend-1".to_string(),
            Role::specialist("frontend"),
            "test-session".to_string(),
            "2".to_string(),
        );

        let backend1_id = expert_registry.register_expert(backend_expert1).unwrap();
        let backend2_id = expert_registry.register_expert(backend_expert2).unwrap();
        let frontend_id = expert_registry.register_expert(frontend_expert).unwrap();

        let mut router = MessageRouter::new(queue_manager, expert_registry, MockTmuxSender);

        // Set backend1 to busy, leave backend2 idle (default)
        router
            .expert_registry_mut()
            .update_expert_state(backend1_id, ExpertState::Busy)
            .unwrap();

        // Send message to "backend" role
        let content = MessageContent {
            subject: "Backend Task".to_string(),
            body: "Need help with backend work".to_string(),
        };
        let message = Message::new(
            frontend_id,
            MessageRecipient::role("backend".to_string()),
            MessageType::Delegate,
            content,
        );

        // Find recipient should return the idle backend expert (sender=frontend_id, same worktree=None)
        let recipient = router
            .find_recipient(&message.to, frontend_id)
            .await
            .unwrap();
        assert_eq!(
            recipient,
            Some(backend2_id),
            "Should route to idle backend expert (backend-2)"
        );

        // Verify backend1 is not selected (not idle)
        let is_backend1_idle = router.expert_registry().is_expert_idle(backend1_id);
        assert_eq!(is_backend1_idle, Some(false));
    }
}
