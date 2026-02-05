use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::models::{MessagePriority, MessageType, QueuedMessage};
use crate::utils::truncate_str;

/// Filter options for message display
#[derive(Debug, Clone, Default)]
pub struct MessageFilter {
    pub message_type: Option<MessageType>,
    pub priority: Option<MessagePriority>,
    pub recipient_filter: Option<String>,
}

/// Display widget for messaging queue monitoring
///
/// This is a display-only interface for monitoring queued messages.
/// It does not allow message manipulation - messages are managed by
/// the MessageRouter automatically.
pub struct MessagingDisplay {
    messages: Vec<QueuedMessage>,
    filtered_indices: Vec<usize>,
    state: ListState,
    focused: bool,
    filter: MessageFilter,
}

impl MessagingDisplay {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            filtered_indices: Vec::new(),
            state: ListState::default(),
            focused: false,
            filter: MessageFilter::default(),
        }
    }

    /// Set the messages to display
    pub fn set_messages(&mut self, messages: Vec<QueuedMessage>) {
        self.messages = messages;
        self.apply_filter();
    }

    /// Get all messages currently in the display
    pub fn messages(&self) -> &[QueuedMessage] {
        &self.messages
    }

    /// Get the number of visible (filtered) messages
    pub fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    /// Get the total number of messages
    pub fn total_count(&self) -> usize {
        self.messages.len()
    }

    /// Set the filter for message display
    pub fn set_filter(&mut self, filter: MessageFilter) {
        self.filter = filter;
        self.apply_filter();
    }

    /// Clear all filters
    pub fn clear_filter(&mut self) {
        self.filter = MessageFilter::default();
        self.apply_filter();
    }

    /// Get current filter
    pub fn filter(&self) -> &MessageFilter {
        &self.filter
    }

    fn apply_filter(&mut self) {
        self.filtered_indices = self
            .messages
            .iter()
            .enumerate()
            .filter(|(_, msg)| {
                // Filter by message type if set
                if let Some(ref filter_type) = self.filter.message_type {
                    if &msg.message.message_type != filter_type {
                        return false;
                    }
                }

                // Filter by priority if set
                if let Some(ref filter_priority) = self.filter.priority {
                    if &msg.message.priority != filter_priority {
                        return false;
                    }
                }

                // Filter by recipient if set
                if let Some(ref recipient_filter) = self.filter.recipient_filter {
                    let recipient_str = format!("{:?}", msg.message.to);
                    if !recipient_str.to_lowercase().contains(&recipient_filter.to_lowercase()) {
                        return false;
                    }
                }

                true
            })
            .map(|(i, _)| i)
            .collect();

        // Reset selection if it's out of bounds
        if let Some(selected) = self.state.selected() {
            if selected >= self.filtered_indices.len() {
                if self.filtered_indices.is_empty() {
                    self.state.select(None);
                } else {
                    self.state.select(Some(0));
                }
            }
        }
    }

    /// Set focused state
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Get focused state
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Navigate to next message
    pub fn next(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.filtered_indices.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    /// Navigate to previous message
    pub fn prev(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.filtered_indices.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    /// Get the currently selected message
    pub fn selected_message(&self) -> Option<&QueuedMessage> {
        self.state
            .selected()
            .and_then(|i| self.filtered_indices.get(i))
            .and_then(|&idx| self.messages.get(idx))
    }

    /// Get symbol for message type
    fn type_symbol(message_type: &MessageType) -> (&'static str, Color) {
        match message_type {
            MessageType::Query => ("?", Color::Cyan),
            MessageType::Response => ("R", Color::Green),
            MessageType::Notify => ("!", Color::Yellow),
            MessageType::Delegate => ("D", Color::Magenta),
        }
    }

    /// Get symbol for priority
    fn priority_symbol(priority: &MessagePriority) -> (&'static str, Color) {
        match priority {
            MessagePriority::High => ("⬆", Color::Red),
            MessagePriority::Normal => (" ", Color::White),
            MessagePriority::Low => ("⬇", Color::Gray),
        }
    }

    /// Get recipient display string
    fn recipient_display(recipient: &crate::models::MessageRecipient) -> String {
        match recipient {
            crate::models::MessageRecipient::ExpertId { expert_id } => format!("→{}", expert_id),
            crate::models::MessageRecipient::ExpertName { expert_name } => {
                format!("→{}", truncate_str(expert_name, 8))
            }
            crate::models::MessageRecipient::Role { role } => {
                format!("→@{}", truncate_str(role, 7))
            }
        }
    }

    /// Render the messaging display widget
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .filtered_indices
            .iter()
            .map(|&idx| {
                let msg = &self.messages[idx];
                let (type_symbol, type_color) = Self::type_symbol(&msg.message.message_type);
                let (priority_symbol, priority_color) = Self::priority_symbol(&msg.message.priority);

                let recipient = Self::recipient_display(&msg.message.to);
                let subject = truncate_str(&msg.message.content.subject, 25);

                // Calculate time ago
                let time_ago = {
                    let duration = chrono::Utc::now()
                        .signed_duration_since(msg.message.created_at);
                    if duration.num_hours() > 0 {
                        format!("{}h", duration.num_hours())
                    } else if duration.num_minutes() > 0 {
                        format!("{}m", duration.num_minutes())
                    } else {
                        format!("{}s", duration.num_seconds().max(0))
                    }
                };

                // Status indicator
                let status_indicator = if msg.is_failed() {
                    ("✗", Color::Red)
                } else if msg.is_expired() {
                    ("⌛", Color::DarkGray)
                } else if msg.attempts > 0 {
                    ("↻", Color::Yellow)
                } else {
                    ("○", Color::White)
                };

                let spans = vec![
                    Span::styled(type_symbol, Style::default().fg(type_color).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                    Span::styled(
                        format!("[{}{}]", msg.message.from_expert_id, recipient),
                        Style::default().add_modifier(Modifier::DIM),
                    ),
                    Span::raw(" "),
                    Span::styled(subject, Style::default()),
                    Span::raw(" "),
                    Span::styled(
                        format!("{:>4}", time_ago),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(" "),
                    Span::styled(priority_symbol, Style::default().fg(priority_color)),
                    Span::styled(status_indicator.0, Style::default().fg(status_indicator.1)),
                ];

                ListItem::new(Line::from(spans))
            })
            .collect();

        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Gray)
        };

        let title = if self.filtered_indices.len() == self.messages.len() {
            format!("Messages [{}]", self.messages.len())
        } else {
            format!("Messages [{}/{}]", self.filtered_indices.len(), self.messages.len())
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(title),
            )
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.state);
    }
}

impl Default for MessagingDisplay {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Message, MessageContent, MessageRecipient};

    fn create_test_queued_message(
        from: u32,
        to: MessageRecipient,
        msg_type: MessageType,
        priority: MessagePriority,
        subject: &str,
    ) -> QueuedMessage {
        let content = MessageContent {
            subject: subject.to_string(),
            body: "Test body".to_string(),
        };
        let message = Message::new(from, to, msg_type, content).with_priority(priority);
        QueuedMessage::new(message)
    }

    #[test]
    fn messaging_display_new_creates_empty() {
        let display = MessagingDisplay::new();
        assert!(display.messages.is_empty());
        assert!(display.filtered_indices.is_empty());
        assert!(!display.focused);
    }

    #[test]
    fn messaging_display_set_messages_updates_and_filters() {
        let mut display = MessagingDisplay::new();
        let messages = vec![
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(1),
                MessageType::Query,
                MessagePriority::High,
                "Test 1",
            ),
            create_test_queued_message(
                1,
                MessageRecipient::role("backend".to_string()),
                MessageType::Delegate,
                MessagePriority::Normal,
                "Test 2",
            ),
        ];

        display.set_messages(messages);
        assert_eq!(display.total_count(), 2);
        assert_eq!(display.visible_count(), 2);
    }

    #[test]
    fn messaging_display_filter_by_type() {
        let mut display = MessagingDisplay::new();
        let messages = vec![
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(1),
                MessageType::Query,
                MessagePriority::Normal,
                "Query message",
            ),
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(2),
                MessageType::Delegate,
                MessagePriority::Normal,
                "Delegate message",
            ),
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(3),
                MessageType::Query,
                MessagePriority::Normal,
                "Another query",
            ),
        ];

        display.set_messages(messages);
        assert_eq!(display.visible_count(), 3);

        display.set_filter(MessageFilter {
            message_type: Some(MessageType::Query),
            ..Default::default()
        });
        assert_eq!(display.visible_count(), 2);

        display.clear_filter();
        assert_eq!(display.visible_count(), 3);
    }

    #[test]
    fn messaging_display_filter_by_priority() {
        let mut display = MessagingDisplay::new();
        let messages = vec![
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(1),
                MessageType::Query,
                MessagePriority::High,
                "High priority",
            ),
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(2),
                MessageType::Query,
                MessagePriority::Normal,
                "Normal priority",
            ),
        ];

        display.set_messages(messages);
        display.set_filter(MessageFilter {
            priority: Some(MessagePriority::High),
            ..Default::default()
        });
        assert_eq!(display.visible_count(), 1);
    }

    #[test]
    fn messaging_display_navigation() {
        let mut display = MessagingDisplay::new();
        let messages = vec![
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(1),
                MessageType::Query,
                MessagePriority::Normal,
                "Message 1",
            ),
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(2),
                MessageType::Query,
                MessagePriority::Normal,
                "Message 2",
            ),
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(3),
                MessageType::Query,
                MessagePriority::Normal,
                "Message 3",
            ),
        ];

        display.set_messages(messages);

        // Initially no selection
        assert!(display.selected_message().is_none());

        // Navigate forward
        display.next();
        assert!(display.selected_message().is_some());

        display.next();
        display.next();
        display.next(); // Should wrap to beginning
        assert!(display.selected_message().is_some());
    }

    #[test]
    fn messaging_display_prev_navigation() {
        let mut display = MessagingDisplay::new();
        let messages = vec![
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(1),
                MessageType::Query,
                MessagePriority::Normal,
                "Message 1",
            ),
            create_test_queued_message(
                0,
                MessageRecipient::expert_id(2),
                MessageType::Query,
                MessagePriority::Normal,
                "Message 2",
            ),
        ];

        display.set_messages(messages);

        display.prev();
        assert!(display.selected_message().is_some());

        display.prev(); // Should wrap to end
        assert!(display.selected_message().is_some());
    }

    #[test]
    fn messaging_display_focus_state() {
        let mut display = MessagingDisplay::new();
        assert!(!display.is_focused());

        display.set_focused(true);
        assert!(display.is_focused());

        display.set_focused(false);
        assert!(!display.is_focused());
    }

    #[test]
    fn messaging_display_type_symbol_returns_correct_values() {
        assert_eq!(MessagingDisplay::type_symbol(&MessageType::Query).0, "?");
        assert_eq!(MessagingDisplay::type_symbol(&MessageType::Response).0, "R");
        assert_eq!(MessagingDisplay::type_symbol(&MessageType::Notify).0, "!");
        assert_eq!(MessagingDisplay::type_symbol(&MessageType::Delegate).0, "D");
    }

    #[test]
    fn messaging_display_priority_symbol_returns_correct_values() {
        assert_eq!(MessagingDisplay::priority_symbol(&MessagePriority::High).0, "⬆");
        assert_eq!(MessagingDisplay::priority_symbol(&MessagePriority::Normal).0, " ");
        assert_eq!(MessagingDisplay::priority_symbol(&MessagePriority::Low).0, "⬇");
    }

    #[test]
    fn messaging_display_recipient_display_formats_correctly() {
        assert!(MessagingDisplay::recipient_display(&MessageRecipient::expert_id(5)).contains("5"));
        assert!(MessagingDisplay::recipient_display(&MessageRecipient::expert_name("test".to_string())).contains("test"));
        assert!(MessagingDisplay::recipient_display(&MessageRecipient::role("backend".to_string())).contains("@"));
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use crate::models::{Message, MessageContent, MessageRecipient, MessageType, MessagePriority, QueuedMessage};

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
            Just(MessagePriority::High),
            Just(MessagePriority::Normal),
            Just(MessagePriority::Low),
        ]
    }

    fn arbitrary_message_recipient() -> impl Strategy<Value = MessageRecipient> {
        prop_oneof![
            (1u32..100).prop_map(MessageRecipient::expert_id),
            "[a-zA-Z0-9]{1,20}".prop_map(MessageRecipient::expert_name),
            "[a-zA-Z0-9]{1,20}".prop_map(MessageRecipient::role),
        ]
    }

    fn arbitrary_queued_message() -> impl Strategy<Value = QueuedMessage> {
        (
            0u32..10,
            arbitrary_message_recipient(),
            arbitrary_message_type(),
            arbitrary_message_priority(),
            "[a-zA-Z0-9 ]{1,50}",
            "[a-zA-Z0-9 ]{1,200}",
        ).prop_map(|(from_id, to, msg_type, priority, subject, body)| {
            let content = MessageContent {
                subject,
                body,
            };
            let message = Message::new(from_id, to, msg_type, content)
                .with_priority(priority);
            QueuedMessage::new(message)
        })
    }

    // Feature: inter-expert-messaging, Property 11: UI Display Completeness
    // **Validates: Requirements 9.1, 9.2, 9.4, 9.5**
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn ui_display_completeness(
            messages in prop::collection::vec(arbitrary_queued_message(), 0..20)
        ) {
            let mut display = MessagingDisplay::new();
            display.set_messages(messages.clone());

            // Requirement 9.1: Display should show all messages
            assert_eq!(
                display.total_count(),
                messages.len(),
                "Display should show all queued messages"
            );

            // Requirement 9.2: Display should support filtering
            assert_eq!(
                display.visible_count(),
                messages.len(),
                "Without filter, all messages should be visible"
            );

            // Verify messages() accessor returns the same messages
            let displayed_messages = display.messages();
            assert_eq!(
                displayed_messages.len(),
                messages.len(),
                "messages() should return all set messages"
            );

            // Requirement 9.4: Display is read-only (no manipulation methods)
            // Verified by the absence of delete/modify methods in the API

            // Requirement 9.5: Navigation should work correctly
            if !messages.is_empty() {
                display.next();
                assert!(
                    display.selected_message().is_some(),
                    "Should be able to select a message"
                );
            }
        }

        #[test]
        fn ui_display_filter_by_type(
            messages in prop::collection::vec(arbitrary_queued_message(), 1..20),
            filter_type in arbitrary_message_type()
        ) {
            let mut display = MessagingDisplay::new();
            display.set_messages(messages.clone());

            // Apply type filter
            display.set_filter(MessageFilter {
                message_type: Some(filter_type),
                ..Default::default()
            });

            // Count expected matches
            let expected_count = messages.iter()
                .filter(|m| m.message.message_type == filter_type)
                .count();

            assert_eq!(
                display.visible_count(),
                expected_count,
                "Filter should show only messages of type {:?}",
                filter_type
            );

            // Clear filter should restore all messages
            display.clear_filter();
            assert_eq!(
                display.visible_count(),
                messages.len(),
                "Clear filter should restore all messages"
            );
        }

        #[test]
        fn ui_display_filter_by_priority(
            messages in prop::collection::vec(arbitrary_queued_message(), 1..20),
            filter_priority in arbitrary_message_priority()
        ) {
            let mut display = MessagingDisplay::new();
            display.set_messages(messages.clone());

            // Apply priority filter
            display.set_filter(MessageFilter {
                priority: Some(filter_priority),
                ..Default::default()
            });

            // Count expected matches
            let expected_count = messages.iter()
                .filter(|m| m.message.priority == filter_priority)
                .count();

            assert_eq!(
                display.visible_count(),
                expected_count,
                "Filter should show only messages with priority {:?}",
                filter_priority
            );
        }

        #[test]
        fn ui_display_navigation_consistency(
            messages in prop::collection::vec(arbitrary_queued_message(), 2..10)
        ) {
            let mut display = MessagingDisplay::new();
            display.set_messages(messages.clone());

            // Navigation should be consistent
            let num_messages = messages.len();

            // Navigate to first message
            display.next();
            let first_index = display.state.selected();
            assert_eq!(first_index, Some(0), "First next() should select index 0");

            // Navigate through remaining messages
            for expected_index in 1..num_messages {
                display.next();
                let current_index = display.state.selected();
                assert_eq!(
                    current_index,
                    Some(expected_index),
                    "Navigation should proceed through indices sequentially"
                );
            }

            // After navigating through all, we should be at the last message
            // Next should wrap to the first
            display.next();
            let wrapped_index = display.state.selected();
            assert_eq!(
                wrapped_index,
                Some(0),
                "Navigation should wrap around to index 0"
            );

            // Navigate forward and then back
            display.next();
            let second_index = display.state.selected();
            assert_eq!(second_index, Some(1), "next() should go to index 1");

            display.prev();
            let back_to_first_index = display.state.selected();
            assert_eq!(
                back_to_first_index,
                Some(0),
                "prev() should reverse next() and go back to index 0"
            );
        }

        #[test]
        fn ui_display_message_fields_accessible(
            messages in prop::collection::vec(arbitrary_queued_message(), 1..10)
        ) {
            let mut display = MessagingDisplay::new();
            display.set_messages(messages.clone());

            // Navigate to first message
            display.next();

            if let Some(selected) = display.selected_message() {
                // Verify all required fields are accessible
                let _ = &selected.message.message_id;
                let _ = &selected.message.from_expert_id;
                let _ = &selected.message.to;
                let _ = &selected.message.message_type;
                let _ = &selected.message.priority;
                let _ = &selected.message.created_at;
                let _ = &selected.message.content.subject;
                let _ = &selected.message.content.body;
                let _ = &selected.attempts;
                let _ = selected.is_failed();
                let _ = selected.is_expired();

                // Verify the selected message matches one from our input
                let msg_id = &selected.message.message_id;
                assert!(
                    messages.iter().any(|m| &m.message.message_id == msg_id),
                    "Selected message should be from the input list"
                );
            }
        }

        #[test]
        fn ui_display_type_and_priority_symbols(
            msg_type in arbitrary_message_type(),
            priority in arbitrary_message_priority()
        ) {
            // Verify type symbols are distinct and meaningful
            let (type_symbol, type_color) = MessagingDisplay::type_symbol(&msg_type);
            assert!(
                !type_symbol.is_empty(),
                "Type symbol should not be empty for {:?}",
                msg_type
            );
            let _ = type_color; // Color should be defined

            // Verify priority symbols are distinct and meaningful
            let (priority_symbol, priority_color) = MessagingDisplay::priority_symbol(&priority);
            // Priority symbols can be empty for Normal priority
            let _ = priority_symbol;
            let _ = priority_color;

            // Verify all type symbols are unique
            let all_types = [
                MessageType::Query,
                MessageType::Response,
                MessageType::Notify,
                MessageType::Delegate,
            ];
            let symbols: Vec<_> = all_types.iter()
                .map(|t| MessagingDisplay::type_symbol(t).0)
                .collect();
            for (i, s1) in symbols.iter().enumerate() {
                for (j, s2) in symbols.iter().enumerate() {
                    if i != j {
                        assert_ne!(
                            s1, s2,
                            "Type symbols should be unique: {:?} vs {:?}",
                            all_types[i], all_types[j]
                        );
                    }
                }
            }
        }

        #[test]
        fn ui_display_recipient_formatting(
            recipient in arbitrary_message_recipient()
        ) {
            let formatted = MessagingDisplay::recipient_display(&recipient);

            // Recipient display should not be empty
            assert!(
                !formatted.is_empty(),
                "Recipient display should not be empty"
            );

            // Recipient display should contain direction indicator
            assert!(
                formatted.contains("→"),
                "Recipient display should contain direction indicator: {}",
                formatted
            );

            // Role-based recipients should have @ prefix
            if let MessageRecipient::Role { .. } = recipient {
                assert!(
                    formatted.contains("@"),
                    "Role recipient should have @ indicator: {}",
                    formatted
                );
            }
        }
    }
}
