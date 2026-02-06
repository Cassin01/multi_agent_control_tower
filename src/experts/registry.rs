use std::collections::HashMap;
use thiserror::Error;

use crate::models::{ExpertInfo, ExpertState, Role, ExpertId};

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("Expert not found: {0}")]
    ExpertNotFound(ExpertId),
    
    #[error("Expert name already exists: {0}")]
    DuplicateName(String),
    
    #[error("Invalid expert state transition from {from:?} to {to:?}")]
    InvalidStateTransition { from: ExpertState, to: ExpertState },
}

/// Registry for tracking expert states and capabilities
/// 
/// The ExpertRegistry maintains a centralized view of all experts in the system,
/// providing efficient lookups by ID, name, and role. It tracks expert states
/// to support non-blocking message delivery and role-based recipient targeting.
#[derive(Debug, Clone)]
pub struct ExpertRegistry {
    /// Primary storage of expert information indexed by ID
    experts: HashMap<ExpertId, ExpertInfo>,
    
    /// Fast lookup from expert name to ID
    name_to_id: HashMap<String, ExpertId>,
    
    /// Fast lookup from role to list of expert IDs
    role_to_ids: HashMap<Role, Vec<ExpertId>>,
    
    /// Next available expert ID for registration
    next_id: ExpertId,
}

impl ExpertRegistry {
    /// Create a new empty expert registry
    pub fn new() -> Self {
        Self {
            experts: HashMap::new(),
            name_to_id: HashMap::new(),
            role_to_ids: HashMap::new(),
            next_id: 1,
        }
    }

    /// Register a new expert in the registry
    /// 
    /// Returns the assigned expert ID. Expert names must be unique.
    /// The expert is initially registered in Offline state.
    pub fn register_expert(&mut self, mut expert_info: ExpertInfo) -> Result<ExpertId, RegistryError> {
        // Check for duplicate names
        if self.name_to_id.contains_key(&expert_info.name) {
            return Err(RegistryError::DuplicateName(expert_info.name.clone()));
        }

        // Assign ID if not already set
        if expert_info.id == 0 {
            expert_info.id = self.next_id;
            self.next_id += 1;
        } else {
            // Update next_id if the provided ID is higher
            if expert_info.id >= self.next_id {
                self.next_id = expert_info.id + 1;
            }
        }

        let expert_id = expert_info.id;
        let name = expert_info.name.clone();
        let role = expert_info.role.clone();

        // Add to primary storage
        self.experts.insert(expert_id, expert_info);

        // Add to name lookup
        self.name_to_id.insert(name, expert_id);

        // Add to role lookup
        self.role_to_ids
            .entry(role)
            .or_insert_with(Vec::new)
            .push(expert_id);

        Ok(expert_id)
    }

    /// Update the state of an expert
    /// 
    /// This method updates the expert's state and last activity timestamp.
    /// State transitions are validated to ensure consistency.
    pub fn update_expert_state(&mut self, expert_id: ExpertId, new_state: ExpertState) -> Result<(), RegistryError> {
        // First check if expert exists and get current state
        let current_state = self.experts
            .get(&expert_id)
            .ok_or(RegistryError::ExpertNotFound(expert_id))?
            .state.clone();

        // Validate state transition (basic validation - can be extended)
        if !self.is_valid_state_transition(&current_state, &new_state) {
            return Err(RegistryError::InvalidStateTransition {
                from: current_state,
                to: new_state,
            });
        }

        // Now update the state
        let expert = self.experts
            .get_mut(&expert_id)
            .ok_or(RegistryError::ExpertNotFound(expert_id))?;

        expert.set_state(new_state);
        Ok(())
    }

    /// Find expert ID by name (case-insensitive)
    pub fn find_by_name(&self, name: &str) -> Option<ExpertId> {
        // First try exact match
        if let Some(&expert_id) = self.name_to_id.get(name) {
            return Some(expert_id);
        }

        // Fall back to case-insensitive search
        for (expert_name, &expert_id) in &self.name_to_id {
            if expert_name.eq_ignore_ascii_case(name) {
                return Some(expert_id);
            }
        }

        None
    }

    /// Find all expert IDs with the specified role
    pub fn find_by_role(&self, role: &Role) -> Vec<ExpertId> {
        self.role_to_ids
            .get(role)
            .cloned()
            .unwrap_or_default()
    }

    /// Find all expert IDs with a role matching the given string
    pub fn find_by_role_str(&self, role_str: &str) -> Vec<ExpertId> {
        let mut matching_experts = Vec::new();

        for (role, expert_ids) in &self.role_to_ids {
            if role.matches(role_str) {
                matching_experts.extend(expert_ids);
            }
        }

        matching_experts
    }

    /// Get all expert IDs that are currently idle
    pub fn get_idle_experts(&self) -> Vec<ExpertId> {
        self.experts
            .iter()
            .filter_map(|(&id, expert)| {
                if expert.is_idle() {
                    Some(id)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all expert IDs with the specified role that are currently idle
    pub fn get_idle_experts_by_role(&self, role: &Role) -> Vec<ExpertId> {
        let role_experts = self.find_by_role(role);
        role_experts
            .into_iter()
            .filter(|&expert_id| {
                self.experts
                    .get(&expert_id)
                    .map(|expert| expert.is_idle())
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Get all expert IDs with a role matching the string that are currently idle
    pub fn get_idle_experts_by_role_str(&self, role_str: &str) -> Vec<ExpertId> {
        let role_experts = self.find_by_role_str(role_str);
        role_experts
            .into_iter()
            .filter(|&expert_id| {
                self.experts
                    .get(&expert_id)
                    .map(|expert| expert.is_idle())
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Check if a specific expert is idle
    pub fn is_expert_idle(&self, expert_id: ExpertId) -> Option<bool> {
        self.experts
            .get(&expert_id)
            .map(|expert| expert.is_idle())
    }

    /// Get expert information by ID
    pub fn get_expert(&self, expert_id: ExpertId) -> Option<&ExpertInfo> {
        self.experts.get(&expert_id)
    }

    /// Get mutable expert information by ID
    pub fn get_expert_mut(&mut self, expert_id: ExpertId) -> Option<&mut ExpertInfo> {
        self.experts.get_mut(&expert_id)
    }

    /// Get all registered experts
    pub fn get_all_experts(&self) -> Vec<&ExpertInfo> {
        self.experts.values().collect()
    }

    /// Remove an expert from the registry
    /// 
    /// This removes the expert from all lookup tables and returns the expert info
    /// if it existed.
    pub fn remove_expert(&mut self, expert_id: ExpertId) -> Option<ExpertInfo> {
        if let Some(expert) = self.experts.remove(&expert_id) {
            // Remove from name lookup
            self.name_to_id.remove(&expert.name);

            // Remove from role lookup
            if let Some(role_experts) = self.role_to_ids.get_mut(&expert.role) {
                role_experts.retain(|&id| id != expert_id);
                // Remove empty role entries
                if role_experts.is_empty() {
                    self.role_to_ids.remove(&expert.role);
                }
            }

            Some(expert)
        } else {
            None
        }
    }

    /// Get the number of registered experts
    pub fn len(&self) -> usize {
        self.experts.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.experts.is_empty()
    }

    /// Validate if a state transition is allowed
    /// 
    /// Currently allows all transitions, but can be extended with business logic
    fn is_valid_state_transition(&self, _from: &ExpertState, _to: &ExpertState) -> bool {
        // For now, allow all state transitions
        // This can be extended with specific business rules if needed
        true
    }
}

impl Default for ExpertRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_expert(name: &str, role: Role) -> ExpertInfo {
        ExpertInfo::new(
            0, // ID will be assigned by registry
            name.to_string(),
            role,
            "test-session".to_string(),
            "0".to_string(),
        )
    }

    #[test]
    fn registry_new_creates_empty_registry() {
        let registry = ExpertRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert_eq!(registry.next_id, 1);
    }

    #[test]
    fn register_expert_assigns_id_and_updates_lookups() {
        let mut registry = ExpertRegistry::new();
        let expert = create_test_expert("backend-dev", Role::Developer);

        let expert_id = registry.register_expert(expert).unwrap();
        
        assert_eq!(expert_id, 1);
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.next_id, 2);

        // Check lookups are updated
        assert_eq!(registry.find_by_name("backend-dev"), Some(1));
        assert_eq!(registry.find_by_role(&Role::Developer), vec![1]);
    }

    #[test]
    fn register_expert_with_existing_id() {
        let mut registry = ExpertRegistry::new();
        let mut expert = create_test_expert("test", Role::Analyst);
        expert.id = 5; // Set specific ID

        let expert_id = registry.register_expert(expert).unwrap();
        
        assert_eq!(expert_id, 5);
        assert_eq!(registry.next_id, 6); // Should update next_id
    }

    #[test]
    fn register_expert_duplicate_name_fails() {
        let mut registry = ExpertRegistry::new();
        let expert1 = create_test_expert("duplicate", Role::Developer);
        let expert2 = create_test_expert("duplicate", Role::Analyst);

        registry.register_expert(expert1).unwrap();
        let result = registry.register_expert(expert2);

        assert!(matches!(result, Err(RegistryError::DuplicateName(_))));
    }

    #[test]
    fn update_expert_state_changes_state() {
        let mut registry = ExpertRegistry::new();
        let expert = create_test_expert("test", Role::Developer);
        let expert_id = registry.register_expert(expert).unwrap();

        // Initially offline
        assert_eq!(registry.get_expert(expert_id).unwrap().state, ExpertState::Offline);

        // Update to idle
        registry.update_expert_state(expert_id, ExpertState::Idle).unwrap();
        assert_eq!(registry.get_expert(expert_id).unwrap().state, ExpertState::Idle);

        // Update to busy
        registry.update_expert_state(expert_id, ExpertState::Busy).unwrap();
        assert_eq!(registry.get_expert(expert_id).unwrap().state, ExpertState::Busy);
    }

    #[test]
    fn update_expert_state_nonexistent_expert_fails() {
        let mut registry = ExpertRegistry::new();
        let result = registry.update_expert_state(999, ExpertState::Idle);
        assert!(matches!(result, Err(RegistryError::ExpertNotFound(999))));
    }

    #[test]
    fn find_by_name_case_insensitive() {
        let mut registry = ExpertRegistry::new();
        let expert = create_test_expert("Backend-Expert", Role::Developer);
        let expert_id = registry.register_expert(expert).unwrap();

        assert_eq!(registry.find_by_name("Backend-Expert"), Some(expert_id));
        assert_eq!(registry.find_by_name("backend-expert"), Some(expert_id));
        assert_eq!(registry.find_by_name("BACKEND-EXPERT"), Some(expert_id));
        assert_eq!(registry.find_by_name("nonexistent"), None);
    }

    #[test]
    fn find_by_role_returns_all_matching_experts() {
        let mut registry = ExpertRegistry::new();
        
        let dev1 = create_test_expert("dev1", Role::Developer);
        let dev2 = create_test_expert("dev2", Role::Developer);
        let analyst = create_test_expert("analyst1", Role::Analyst);

        let dev1_id = registry.register_expert(dev1).unwrap();
        let dev2_id = registry.register_expert(dev2).unwrap();
        let _analyst_id = registry.register_expert(analyst).unwrap();

        let developers = registry.find_by_role(&Role::Developer);
        assert_eq!(developers.len(), 2);
        assert!(developers.contains(&dev1_id));
        assert!(developers.contains(&dev2_id));

        let analysts = registry.find_by_role(&Role::Analyst);
        assert_eq!(analysts.len(), 1);
    }

    #[test]
    fn find_by_role_str_matches_role_strings() {
        let mut registry = ExpertRegistry::new();
        
        let dev = create_test_expert("dev", Role::Developer);
        let specialist = create_test_expert("spec", Role::specialist("backend"));

        let dev_id = registry.register_expert(dev).unwrap();
        let spec_id = registry.register_expert(specialist).unwrap();

        let developers = registry.find_by_role_str("developer");
        assert_eq!(developers, vec![dev_id]);

        let backend_experts = registry.find_by_role_str("backend");
        assert_eq!(backend_experts, vec![spec_id]);

        let nonexistent = registry.find_by_role_str("nonexistent");
        assert!(nonexistent.is_empty());
    }

    #[test]
    fn get_idle_experts_filters_by_state() {
        let mut registry = ExpertRegistry::new();
        
        let expert1 = create_test_expert("expert1", Role::Developer);
        let expert2 = create_test_expert("expert2", Role::Analyst);
        let expert3 = create_test_expert("expert3", Role::Reviewer);

        let id1 = registry.register_expert(expert1).unwrap();
        let id2 = registry.register_expert(expert2).unwrap();
        let id3 = registry.register_expert(expert3).unwrap();

        // Initially all offline
        assert!(registry.get_idle_experts().is_empty());

        // Set some to idle
        registry.update_expert_state(id1, ExpertState::Idle).unwrap();
        registry.update_expert_state(id2, ExpertState::Idle).unwrap();
        registry.update_expert_state(id3, ExpertState::Busy).unwrap();

        let idle_experts = registry.get_idle_experts();
        assert_eq!(idle_experts.len(), 2);
        assert!(idle_experts.contains(&id1));
        assert!(idle_experts.contains(&id2));
        assert!(!idle_experts.contains(&id3));
    }

    #[test]
    fn get_idle_experts_by_role_filters_by_role_and_state() {
        let mut registry = ExpertRegistry::new();
        
        let dev1 = create_test_expert("dev1", Role::Developer);
        let dev2 = create_test_expert("dev2", Role::Developer);
        let analyst = create_test_expert("analyst", Role::Analyst);

        let dev1_id = registry.register_expert(dev1).unwrap();
        let dev2_id = registry.register_expert(dev2).unwrap();
        let analyst_id = registry.register_expert(analyst).unwrap();

        // Set states
        registry.update_expert_state(dev1_id, ExpertState::Idle).unwrap();
        registry.update_expert_state(dev2_id, ExpertState::Busy).unwrap();
        registry.update_expert_state(analyst_id, ExpertState::Idle).unwrap();

        let idle_developers = registry.get_idle_experts_by_role(&Role::Developer);
        assert_eq!(idle_developers, vec![dev1_id]);

        let idle_analysts = registry.get_idle_experts_by_role(&Role::Analyst);
        assert_eq!(idle_analysts, vec![analyst_id]);
    }

    #[test]
    fn get_idle_experts_by_role_str_filters_by_role_string_and_state() {
        let mut registry = ExpertRegistry::new();
        
        let dev = create_test_expert("dev", Role::Developer);
        let backend_spec = create_test_expert("backend", Role::specialist("backend"));

        let dev_id = registry.register_expert(dev).unwrap();
        let backend_id = registry.register_expert(backend_spec).unwrap();

        // Set to idle
        registry.update_expert_state(dev_id, ExpertState::Idle).unwrap();
        registry.update_expert_state(backend_id, ExpertState::Idle).unwrap();

        let idle_developers = registry.get_idle_experts_by_role_str("developer");
        assert_eq!(idle_developers, vec![dev_id]);

        let idle_backend = registry.get_idle_experts_by_role_str("backend");
        assert_eq!(idle_backend, vec![backend_id]);
    }

    #[test]
    fn is_expert_idle_returns_correct_state() {
        let mut registry = ExpertRegistry::new();
        let expert = create_test_expert("test", Role::Developer);
        let expert_id = registry.register_expert(expert).unwrap();

        // Initially offline (not idle)
        assert_eq!(registry.is_expert_idle(expert_id), Some(false));

        // Set to idle
        registry.update_expert_state(expert_id, ExpertState::Idle).unwrap();
        assert_eq!(registry.is_expert_idle(expert_id), Some(true));

        // Set to busy
        registry.update_expert_state(expert_id, ExpertState::Busy).unwrap();
        assert_eq!(registry.is_expert_idle(expert_id), Some(false));

        // Nonexistent expert
        assert_eq!(registry.is_expert_idle(999), None);
    }

    #[test]
    fn remove_expert_cleans_up_all_lookups() {
        let mut registry = ExpertRegistry::new();
        let expert = create_test_expert("to-remove", Role::Developer);
        let expert_id = registry.register_expert(expert).unwrap();

        // Verify expert is registered
        assert!(registry.get_expert(expert_id).is_some());
        assert!(registry.find_by_name("to-remove").is_some());
        assert!(!registry.find_by_role(&Role::Developer).is_empty());

        // Remove expert
        let removed = registry.remove_expert(expert_id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name, "to-remove");

        // Verify all lookups are cleaned up
        assert!(registry.get_expert(expert_id).is_none());
        assert!(registry.find_by_name("to-remove").is_none());
        assert!(registry.find_by_role(&Role::Developer).is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn remove_nonexistent_expert_returns_none() {
        let mut registry = ExpertRegistry::new();
        let result = registry.remove_expert(999);
        assert!(result.is_none());
    }

    #[test]
    fn get_all_experts_returns_all_registered() {
        let mut registry = ExpertRegistry::new();
        
        let expert1 = create_test_expert("expert1", Role::Developer);
        let expert2 = create_test_expert("expert2", Role::Analyst);

        registry.register_expert(expert1).unwrap();
        registry.register_expert(expert2).unwrap();

        let all_experts = registry.get_all_experts();
        assert_eq!(all_experts.len(), 2);
        
        let names: Vec<&str> = all_experts.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"expert1"));
        assert!(names.contains(&"expert2"));
    }

    #[test]
    fn registry_default_creates_empty_registry() {
        let registry = ExpertRegistry::default();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // Generators for property-based testing
    fn arbitrary_expert_state() -> impl Strategy<Value = ExpertState> {
        prop_oneof![
            Just(ExpertState::Idle),
            Just(ExpertState::Busy),
            Just(ExpertState::Offline),
        ]
    }

    fn arbitrary_role() -> impl Strategy<Value = Role> {
        prop_oneof![
            Just(Role::Analyst),
            Just(Role::Developer),
            Just(Role::Reviewer),
            Just(Role::Coordinator),
            "[a-zA-Z0-9-]{1,20}".prop_map(Role::specialist),
        ]
    }

    fn arbitrary_expert_info() -> impl Strategy<Value = ExpertInfo> {
        (
            "[a-zA-Z0-9-]{1,30}",
            arbitrary_role(),
            "[a-zA-Z0-9-]{1,20}",
            "[0-9]{1,2}",
        ).prop_map(|(name, role, session, pane)| {
            ExpertInfo::new(0, name, role, session, pane)
        })
    }

    // Feature: inter-expert-messaging, Property 9: Expert State Tracking
    // **Validates: Requirements 7.2, 7.3, 7.4**
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn expert_state_tracking_accuracy(
            experts in prop::collection::vec(arbitrary_expert_info(), 1..20),
            state_changes in prop::collection::vec((0usize..19, arbitrary_expert_state()), 1..50)
        ) {
            let mut registry = ExpertRegistry::new();
            let mut expert_ids = Vec::new();

            // Register all experts, handling duplicate names by making them unique
            for (index, mut expert) in experts.into_iter().enumerate() {
                // Make names unique by appending index if needed
                expert.name = format!("{}-{}", expert.name, index);
                let expert_id = registry.register_expert(expert).unwrap();
                expert_ids.push(expert_id);
            }

            // Apply state changes and verify tracking accuracy
            for (expert_index, new_state) in state_changes {
                if expert_index < expert_ids.len() {
                    let expert_id = expert_ids[expert_index];
                    
                    // Update the expert state
                    registry.update_expert_state(expert_id, new_state.clone()).unwrap();
                    
                    // Verify the state was accurately tracked
                    let expert_info = registry.get_expert(expert_id).unwrap();
                    assert_eq!(expert_info.state, new_state);
                    
                    // Verify state-based queries return correct information
                    match new_state {
                        ExpertState::Idle => {
                            assert!(registry.is_expert_idle(expert_id).unwrap());
                            assert!(registry.get_idle_experts().contains(&expert_id));
                        },
                        ExpertState::Busy | ExpertState::Offline => {
                            assert!(!registry.is_expert_idle(expert_id).unwrap());
                            assert!(!registry.get_idle_experts().contains(&expert_id));
                        }
                    }
                    
                    // Verify role-based idle queries work correctly
                    let expert_role = &expert_info.role;
                    let idle_experts_by_role = registry.get_idle_experts_by_role(expert_role);
                    
                    if new_state == ExpertState::Idle {
                        assert!(idle_experts_by_role.contains(&expert_id));
                    } else {
                        assert!(!idle_experts_by_role.contains(&expert_id));
                    }
                    
                    // Verify last activity timestamp was updated
                    let current_time = chrono::Utc::now();
                    let time_diff = current_time.signed_duration_since(expert_info.last_activity);
                    assert!(time_diff.num_seconds() < 5); // Should be very recent
                }
            }
        }

        #[test]
        fn expert_state_consistency_across_lookups(
            experts in prop::collection::vec(arbitrary_expert_info(), 1..10),
            final_states in prop::collection::vec(arbitrary_expert_state(), 1..10)
        ) {
            let mut registry = ExpertRegistry::new();
            let mut expert_ids = Vec::new();

            // Register experts and set their final states
            for (expert, final_state) in experts.into_iter().zip(final_states.into_iter()) {
                let expert_id = registry.register_expert(expert).unwrap();
                registry.update_expert_state(expert_id, final_state).unwrap();
                expert_ids.push(expert_id);
            }

            // Verify consistency across all lookup methods
            for expert_id in expert_ids {
                let expert_info = registry.get_expert(expert_id).unwrap();
                let is_idle_direct = registry.is_expert_idle(expert_id).unwrap();
                let is_idle_computed = expert_info.is_idle();
                
                // Direct query and computed state should match
                assert_eq!(is_idle_direct, is_idle_computed);
                
                // Idle experts list should be consistent
                let idle_experts = registry.get_idle_experts();
                assert_eq!(idle_experts.contains(&expert_id), is_idle_direct);
                
                // Role-based idle lookup should be consistent
                let idle_by_role = registry.get_idle_experts_by_role(&expert_info.role);
                assert_eq!(idle_by_role.contains(&expert_id), is_idle_direct);
                
                // String-based role lookup should be consistent
                let idle_by_role_str = registry.get_idle_experts_by_role_str(expert_info.role.as_str());
                assert_eq!(idle_by_role_str.contains(&expert_id), is_idle_direct);
            }
        }

        #[test]
        fn expert_state_delivery_decision_support(
            expert in arbitrary_expert_info(),
            state_sequence in prop::collection::vec(arbitrary_expert_state(), 1..20)
        ) {
            let mut registry = ExpertRegistry::new();
            let expert_id = registry.register_expert(expert).unwrap();
            
            // Test that state information supports delivery decisions
            for state in state_sequence {
                registry.update_expert_state(expert_id, state.clone()).unwrap();
                
                // Message router should be able to determine delivery eligibility
                let can_deliver = registry.is_expert_idle(expert_id).unwrap();
                let expected_can_deliver = matches!(state, ExpertState::Idle);
                
                assert_eq!(can_deliver, expected_can_deliver);
                
                // Role-based delivery targeting should work correctly
                let expert_info = registry.get_expert(expert_id).unwrap();
                let idle_experts_for_role = registry.get_idle_experts_by_role(&expert_info.role);
                
                if expected_can_deliver {
                    assert!(idle_experts_for_role.contains(&expert_id));
                } else {
                    assert!(!idle_experts_for_role.contains(&expert_id));
                }
            }
        }
    }
}