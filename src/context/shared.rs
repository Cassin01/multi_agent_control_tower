use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub made_by: u32,
    pub timestamp: DateTime<Utc>,
    pub topic: String,
    pub decision: String,
    pub rationale: String,
    #[serde(default)]
    pub affects_experts: Vec<u32>,
}

impl Decision {
    pub fn new(made_by: u32, topic: String, decision: String, rationale: String) -> Self {
        Self {
            id: format!("decision-{}", Utc::now().timestamp()),
            made_by,
            timestamp: Utc::now(),
            topic,
            decision,
            rationale,
            affects_experts: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn with_affects_experts(mut self, experts: Vec<u32>) -> Self {
        self.affects_experts = experts;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Convention {
    pub pattern: String,
    pub description: String,
    pub discovered_at: DateTime<Utc>,
    pub discovered_by: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileDependency {
    pub file: String,
    pub depends_on: Vec<String>,
    pub depended_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SharedContext {
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub conventions: Vec<Convention>,
    #[serde(default)]
    pub dependencies: Vec<FileDependency>,
}

impl SharedContext {
    pub fn add_decision(&mut self, decision: Decision) {
        self.decisions.push(decision);
    }

    #[allow(dead_code)]
    pub fn get_decisions_for_expert(&self, expert_id: u32) -> Vec<&Decision> {
        self.decisions
            .iter()
            .filter(|d| d.affects_experts.is_empty() || d.affects_experts.contains(&expert_id))
            .collect()
    }

    #[allow(dead_code)]
    pub fn get_decisions_by_topic(&self, topic: &str) -> Vec<&Decision> {
        let topic_lower = topic.to_lowercase();
        self.decisions
            .iter()
            .filter(|d| d.topic.to_lowercase().contains(&topic_lower))
            .collect()
    }

    #[allow(dead_code)]
    pub fn add_convention(&mut self, convention: Convention) {
        self.conventions.push(convention);
    }

    #[allow(dead_code)]
    pub fn get_conventions(&self) -> &[Convention] {
        &self.conventions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_new_creates_with_timestamp() {
        let decision = Decision::new(
            0,
            "Architecture".to_string(),
            "Use microservices".to_string(),
            "Better scalability".to_string(),
        );

        assert!(decision.id.starts_with("decision-"));
        assert_eq!(decision.made_by, 0);
        assert_eq!(decision.topic, "Architecture");
        assert!(decision.affects_experts.is_empty());
    }

    #[test]
    fn decision_with_affects_experts_sets_list() {
        let decision = Decision::new(
            0,
            "API".to_string(),
            "Use REST".to_string(),
            "Standard".to_string(),
        )
        .with_affects_experts(vec![1, 2]);

        assert_eq!(decision.affects_experts, vec![1, 2]);
    }

    #[test]
    fn shared_context_add_decision_appends() {
        let mut ctx = SharedContext::default();

        ctx.add_decision(Decision::new(
            0,
            "Auth".to_string(),
            "Use JWT".to_string(),
            "Stateless".to_string(),
        ));

        assert_eq!(ctx.decisions.len(), 1);
    }

    #[test]
    fn shared_context_get_decisions_for_expert_filters_correctly() {
        let mut ctx = SharedContext::default();

        ctx.add_decision(Decision::new(
            0,
            "Global".to_string(),
            "Use TypeScript".to_string(),
            "Type safety".to_string(),
        ));

        ctx.add_decision(
            Decision::new(
                0,
                "Frontend".to_string(),
                "Use React".to_string(),
                "Popular".to_string(),
            )
            .with_affects_experts(vec![1]),
        );

        ctx.add_decision(
            Decision::new(
                0,
                "Backend".to_string(),
                "Use Rust".to_string(),
                "Performance".to_string(),
            )
            .with_affects_experts(vec![2]),
        );

        let expert1_decisions = ctx.get_decisions_for_expert(1);
        assert_eq!(expert1_decisions.len(), 2);

        let expert2_decisions = ctx.get_decisions_for_expert(2);
        assert_eq!(expert2_decisions.len(), 2);
    }

    #[test]
    fn shared_context_get_decisions_by_topic_filters_case_insensitive() {
        let mut ctx = SharedContext::default();

        ctx.add_decision(Decision::new(
            0,
            "Authentication".to_string(),
            "Use OAuth2".to_string(),
            "Standard".to_string(),
        ));

        ctx.add_decision(Decision::new(
            0,
            "Database".to_string(),
            "Use PostgreSQL".to_string(),
            "Reliable".to_string(),
        ));

        let auth_decisions = ctx.get_decisions_by_topic("auth");
        assert_eq!(auth_decisions.len(), 1);
        assert_eq!(auth_decisions[0].topic, "Authentication");
    }

    #[test]
    fn shared_context_serializes_to_yaml() {
        let mut ctx = SharedContext::default();
        ctx.add_decision(Decision::new(
            0,
            "Test".to_string(),
            "Decision".to_string(),
            "Reason".to_string(),
        ));

        let yaml = serde_yaml::to_string(&ctx).unwrap();
        assert!(yaml.contains("topic: Test"));
        assert!(yaml.contains("decision: Decision"));
    }

    #[test]
    fn shared_context_deserializes_from_yaml() {
        let yaml = r#"
decisions:
  - id: decision-123
    made_by: 0
    timestamp: "2024-01-15T10:00:00Z"
    topic: Architecture
    decision: Use microservices
    rationale: Scalability
    affects_experts: [1, 2]
conventions:
  - pattern: "*.test.ts"
    description: "Test file naming"
    discovered_at: "2024-01-15T10:00:00Z"
    discovered_by: 3
dependencies: []
"#;

        let ctx: SharedContext = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ctx.decisions.len(), 1);
        assert_eq!(ctx.decisions[0].topic, "Architecture");
        assert_eq!(ctx.conventions.len(), 1);
    }
}
