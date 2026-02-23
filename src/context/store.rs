use anyhow::Result;
use std::path::PathBuf;
use tokio::fs;

use super::expert::ExpertContext;
use super::role::SessionExpertRoles;
use super::shared::{Decision, SharedContext};

#[derive(Clone)]
pub struct ContextStore {
    base_path: PathBuf,
}

impl ContextStore {
    pub fn new(queue_path: PathBuf) -> Self {
        Self {
            base_path: queue_path.join("sessions"),
        }
    }

    fn session_path(&self, session_hash: &str) -> PathBuf {
        self.base_path.join(session_hash)
    }

    fn expert_path(&self, session_hash: &str, expert_id: u32) -> PathBuf {
        self.session_path(session_hash)
            .join("experts")
            .join(format!("expert{expert_id}"))
    }

    fn shared_path(&self, session_hash: &str) -> PathBuf {
        self.session_path(session_hash).join("shared")
    }

    pub async fn init_session(&self, session_hash: &str, num_experts: u32) -> Result<()> {
        let session_path = self.session_path(session_hash);
        fs::create_dir_all(&session_path).await?;
        fs::create_dir_all(self.shared_path(session_hash)).await?;

        for i in 0..num_experts {
            let expert_path = self.expert_path(session_hash, i);
            fs::create_dir_all(&expert_path).await?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn session_exists(&self, session_hash: &str) -> bool {
        self.session_path(session_hash).exists()
    }

    pub async fn load_expert_context(
        &self,
        session_hash: &str,
        expert_id: u32,
    ) -> Result<Option<ExpertContext>> {
        let path = self
            .expert_path(session_hash, expert_id)
            .join("context.yaml");

        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).await?;
        let ctx: ExpertContext = serde_yaml::from_str(&content)?;
        Ok(Some(ctx))
    }

    pub async fn save_expert_context(&self, ctx: &ExpertContext) -> Result<()> {
        let expert_path = self.expert_path(&ctx.session_hash, ctx.expert_id);
        fs::create_dir_all(&expert_path).await?;

        let path = expert_path.join("context.yaml");
        let content = serde_yaml::to_string(ctx)?;
        fs::write(&path, content).await?;
        Ok(())
    }

    pub async fn clear_expert_context(&self, session_hash: &str, expert_id: u32) -> Result<()> {
        let expert_path = self.expert_path(session_hash, expert_id);

        for file in ["context.yaml", "learnings.yaml"] {
            let file_path = expert_path.join(file);
            if file_path.exists() {
                fs::remove_file(&file_path).await?;
            }
        }

        Ok(())
    }

    pub async fn load_shared_context(&self, session_hash: &str) -> Result<SharedContext> {
        let path = self.shared_path(session_hash).join("decisions.yaml");

        if !path.exists() {
            return Ok(SharedContext::default());
        }

        let content = fs::read_to_string(&path).await?;
        let ctx: SharedContext = serde_yaml::from_str(&content)?;
        Ok(ctx)
    }

    pub async fn save_shared_context(&self, session_hash: &str, ctx: &SharedContext) -> Result<()> {
        let shared_path = self.shared_path(session_hash);
        fs::create_dir_all(&shared_path).await?;

        let path = shared_path.join("decisions.yaml");
        let content = serde_yaml::to_string(ctx)?;
        fs::write(&path, content).await?;
        Ok(())
    }

    pub async fn add_decision(&self, session_hash: &str, decision: Decision) -> Result<()> {
        let mut ctx = self.load_shared_context(session_hash).await?;
        ctx.add_decision(decision);
        self.save_shared_context(session_hash, &ctx).await?;
        Ok(())
    }

    pub async fn cleanup_session(&self, session_hash: &str) -> Result<()> {
        let session_path = self.session_path(session_hash);
        if session_path.exists() {
            fs::remove_dir_all(&session_path).await?;
        }
        Ok(())
    }

    pub async fn load_session_roles(
        &self,
        session_hash: &str,
    ) -> Result<Option<SessionExpertRoles>> {
        let path = self.session_path(session_hash).join("expert_roles.yaml");
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path).await?;
        let roles: SessionExpertRoles = serde_yaml::from_str(&content)?;
        Ok(Some(roles))
    }

    pub async fn save_session_roles(&self, roles: &SessionExpertRoles) -> Result<()> {
        let session_path = self.session_path(&roles.session_hash);
        fs::create_dir_all(&session_path).await?;
        let path = session_path.join("expert_roles.yaml");
        let content = serde_yaml::to_string(roles)?;
        fs::write(&path, content).await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn list_sessions(&self) -> Result<Vec<String>> {
        let mut sessions = Vec::new();

        if !self.base_path.exists() {
            return Ok(sessions);
        }

        let mut entries = fs::read_dir(&self.base_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    sessions.push(name.to_string());
                }
            }
        }

        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_store() -> (ContextStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = ContextStore::new(temp_dir.path().to_path_buf());
        (store, temp_dir)
    }

    #[tokio::test]
    async fn context_store_init_session_creates_directories() {
        let (store, _temp) = create_test_store().await;

        store.init_session("abc123", 3).await.unwrap();

        assert!(store.session_path("abc123").exists());
        assert!(store.shared_path("abc123").exists());
        assert!(store.expert_path("abc123", 0).exists());
        assert!(store.expert_path("abc123", 1).exists());
        assert!(store.expert_path("abc123", 2).exists());
    }

    #[tokio::test]
    async fn context_store_session_exists_returns_correct_value() {
        let (store, _temp) = create_test_store().await;

        assert!(!store.session_exists("abc123"));

        store.init_session("abc123", 2).await.unwrap();

        assert!(store.session_exists("abc123"));
    }

    #[tokio::test]
    async fn context_store_save_and_load_expert_context() {
        let (store, _temp) = create_test_store().await;
        store.init_session("abc123", 2).await.unwrap();

        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        ctx.set_session_id("session-xyz".to_string());

        store.save_expert_context(&ctx).await.unwrap();

        let loaded = store.load_expert_context("abc123", 0).await.unwrap();
        assert!(loaded.is_some());

        let loaded = loaded.unwrap();
        assert_eq!(loaded.expert_id, 0);
        assert_eq!(loaded.expert_name, "architect");
        assert_eq!(
            loaded.claude_session.session_id,
            Some("session-xyz".to_string())
        );
    }

    #[tokio::test]
    async fn context_store_load_expert_context_returns_none_when_missing() {
        let (store, _temp) = create_test_store().await;
        store.init_session("abc123", 2).await.unwrap();

        let loaded = store.load_expert_context("abc123", 0).await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn context_store_clear_expert_context_removes_files() {
        let (store, _temp) = create_test_store().await;
        store.init_session("abc123", 2).await.unwrap();

        let ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        store.save_expert_context(&ctx).await.unwrap();

        assert!(store
            .load_expert_context("abc123", 0)
            .await
            .unwrap()
            .is_some());

        store.clear_expert_context("abc123", 0).await.unwrap();

        assert!(store
            .load_expert_context("abc123", 0)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn context_store_add_decision_persists() {
        let (store, _temp) = create_test_store().await;
        store.init_session("abc123", 2).await.unwrap();

        let decision = Decision::new(
            0,
            "Architecture".to_string(),
            "Use microservices".to_string(),
            "Scalability".to_string(),
        );

        store.add_decision("abc123", decision).await.unwrap();

        let ctx = store.load_shared_context("abc123").await.unwrap();
        assert_eq!(ctx.decisions.len(), 1);
        assert_eq!(ctx.decisions[0].topic, "Architecture");
    }

    #[tokio::test]
    async fn context_store_cleanup_session_removes_all() {
        let (store, _temp) = create_test_store().await;
        store.init_session("abc123", 2).await.unwrap();

        let ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        store.save_expert_context(&ctx).await.unwrap();

        assert!(store.session_exists("abc123"));

        store.cleanup_session("abc123").await.unwrap();

        assert!(!store.session_exists("abc123"));
    }

    #[tokio::test]
    async fn context_store_list_sessions_returns_all() {
        let (store, _temp) = create_test_store().await;

        store.init_session("session1", 2).await.unwrap();
        store.init_session("session2", 2).await.unwrap();

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.contains(&"session1".to_string()));
        assert!(sessions.contains(&"session2".to_string()));
    }

    #[tokio::test]
    async fn context_store_save_and_load_session_roles() {
        let (store, _temp) = create_test_store().await;
        store.init_session("abc123", 2).await.unwrap();

        let mut roles = SessionExpertRoles::new("abc123".to_string());
        roles.set_role(0, "architect".to_string());
        roles.set_role(1, "frontend".to_string());

        store.save_session_roles(&roles).await.unwrap();

        let loaded = store.load_session_roles("abc123").await.unwrap();
        assert!(loaded.is_some());

        let loaded = loaded.unwrap();
        assert_eq!(loaded.get_role(0), Some("architect"));
        assert_eq!(loaded.get_role(1), Some("frontend"));
    }

    #[tokio::test]
    async fn context_store_load_session_roles_returns_none_when_missing() {
        let (store, _temp) = create_test_store().await;
        store.init_session("abc123", 2).await.unwrap();

        let loaded = store.load_session_roles("abc123").await.unwrap();
        assert!(loaded.is_none());
    }
}
