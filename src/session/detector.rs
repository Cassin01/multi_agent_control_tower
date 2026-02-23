use anyhow::Result;
use std::path::PathBuf;

use crate::models::ExpertState;

pub struct ExpertStateDetector {
    status_dir: PathBuf,
}

impl ExpertStateDetector {
    pub fn new(status_dir: PathBuf) -> Self {
        Self { status_dir }
    }

    pub fn detect_state(&self, expert_id: u32) -> ExpertState {
        let path = self.status_dir.join(format!("expert{expert_id}"));

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return ExpertState::Busy, // missing/unreadable → safe default
        };

        let trimmed = content.trim();

        match trimmed {
            "pending" => ExpertState::Idle,
            "processing" => ExpertState::Busy,
            _ => ExpertState::Busy, // unknown content → safe default
        }
    }

    pub fn detect_all(&self, expert_ids: &[u32]) -> Vec<(u32, ExpertState)> {
        expert_ids
            .iter()
            .map(|&id| (id, self.detect_state(id)))
            .collect()
    }

    pub fn set_marker(&self, expert_id: u32, content: &str) -> Result<()> {
        let path = self.status_dir.join(format!("expert{expert_id}"));
        std::fs::write(&path, content)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn ensure_status_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.status_dir)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (ExpertStateDetector, TempDir) {
        let tmp = TempDir::new().unwrap();
        let detector = ExpertStateDetector::new(tmp.path().to_path_buf());
        (detector, tmp)
    }

    #[test]
    fn pending_content_returns_idle() {
        let (detector, _tmp) = setup();
        std::fs::write(_tmp.path().join("expert0"), "pending").unwrap();

        assert_eq!(
            detector.detect_state(0),
            ExpertState::Idle,
            "detect_state: pending content with fresh mtime should return Idle"
        );
    }

    #[test]
    fn processing_content_returns_busy() {
        let (detector, _tmp) = setup();
        std::fs::write(_tmp.path().join("expert0"), "processing").unwrap();

        assert_eq!(
            detector.detect_state(0),
            ExpertState::Busy,
            "detect_state: processing content should return Busy"
        );
    }

    #[test]
    fn missing_file_returns_busy() {
        let (detector, _tmp) = setup();

        assert_eq!(
            detector.detect_state(99),
            ExpertState::Busy,
            "detect_state: missing file should return Busy as safe default"
        );
    }

    #[test]
    fn unknown_content_returns_busy() {
        let (detector, _tmp) = setup();
        std::fs::write(_tmp.path().join("expert0"), "something_else").unwrap();

        assert_eq!(
            detector.detect_state(0),
            ExpertState::Busy,
            "detect_state: unknown content should return Busy"
        );
    }

    #[test]
    fn whitespace_trimming() {
        let (detector, _tmp) = setup();
        std::fs::write(_tmp.path().join("expert0"), "  pending  \n").unwrap();

        assert_eq!(
            detector.detect_state(0),
            ExpertState::Idle,
            "detect_state: should trim whitespace from content"
        );
    }

    #[test]
    fn set_marker_writes_correctly() {
        let (detector, _tmp) = setup();

        detector.set_marker(0, "processing").unwrap();
        let content = std::fs::read_to_string(_tmp.path().join("expert0")).unwrap();
        assert_eq!(
            content, "processing",
            "set_marker: should write content to file"
        );
    }

    #[test]
    fn set_marker_then_detect() {
        let (detector, _tmp) = setup();

        detector.set_marker(1, "processing").unwrap();
        assert_eq!(detector.detect_state(1), ExpertState::Busy);

        detector.set_marker(1, "pending").unwrap();
        assert_eq!(detector.detect_state(1), ExpertState::Idle);
    }

    #[test]
    fn detect_all_returns_states_for_all_ids() {
        let (detector, _tmp) = setup();
        std::fs::write(_tmp.path().join("expert0"), "pending").unwrap();
        std::fs::write(_tmp.path().join("expert1"), "processing").unwrap();
        // expert2 missing

        let results = detector.detect_all(&[0, 1, 2]);

        assert_eq!(results.len(), 3);
        assert_eq!(results[0], (0, ExpertState::Idle));
        assert_eq!(results[1], (1, ExpertState::Busy));
        assert_eq!(results[2], (2, ExpertState::Busy));
    }

    #[test]
    fn ensure_status_dir_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let status_dir = tmp.path().join("nested").join("status");
        let detector = ExpertStateDetector::new(status_dir.clone());

        assert!(!status_dir.exists());
        detector.ensure_status_dir().unwrap();
        assert!(status_dir.exists());
    }
}
