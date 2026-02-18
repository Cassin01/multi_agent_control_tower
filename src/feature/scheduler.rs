use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::feature::task_parser::TaskEntry;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SchedulerMode {
    #[default]
    Dag,
    Sequential,
}

#[derive(Debug)]
pub struct BlockedDiagnostic {
    pub blocked_tasks: Vec<BlockedTask>,
    pub has_cycle: bool,
}

#[derive(Debug)]
pub struct BlockedTask {
    pub number: String,
    pub missing_deps: Vec<String>,
}

#[derive(Debug)]
pub enum ScheduleResult<'a> {
    /// Tasks available for execution.
    Runnable(Vec<&'a TaskEntry>),
    /// All tasks completed.
    AllDone,
    /// Uncompleted tasks exist but none are runnable.
    Blocked(BlockedDiagnostic),
}

/// Select runnable tasks according to the given scheduling mode.
pub fn select_runnable<'a>(tasks: &'a [TaskEntry], mode: SchedulerMode) -> ScheduleResult<'a> {
    match mode {
        SchedulerMode::Dag => select_runnable_dag(tasks),
        SchedulerMode::Sequential => select_runnable_sequential(tasks),
    }
}

fn select_runnable_dag<'a>(tasks: &'a [TaskEntry]) -> ScheduleResult<'a> {
    let completed_set: HashSet<&str> = tasks
        .iter()
        .filter(|t| t.completed)
        .map(|t| t.number.as_str())
        .collect();

    let known_set: HashSet<&str> = tasks.iter().map(|t| t.number.as_str()).collect();

    let uncompleted: Vec<&TaskEntry> = tasks.iter().filter(|t| !t.completed).collect();

    if uncompleted.is_empty() {
        return ScheduleResult::AllDone;
    }

    let mut runnable = Vec::new();
    let mut blocked_tasks = Vec::new();

    for task in &uncompleted {
        let missing_deps: Vec<String> = task
            .dependencies
            .iter()
            .filter(|dep| !completed_set.contains(dep.as_str()))
            .cloned()
            .collect();

        if missing_deps.is_empty() {
            runnable.push(*task);
        } else {
            blocked_tasks.push(BlockedTask {
                number: task.number.clone(),
                missing_deps,
            });
        }
    }

    if !runnable.is_empty() {
        ScheduleResult::Runnable(runnable)
    } else {
        let has_cycle = blocked_tasks.iter().all(|bt| {
            bt.missing_deps
                .iter()
                .all(|dep| known_set.contains(dep.as_str()))
        });

        ScheduleResult::Blocked(BlockedDiagnostic {
            blocked_tasks,
            has_cycle,
        })
    }
}

fn select_runnable_sequential<'a>(tasks: &'a [TaskEntry]) -> ScheduleResult<'a> {
    let uncompleted: Vec<&TaskEntry> = tasks.iter().filter(|t| !t.completed).collect();

    if uncompleted.is_empty() {
        ScheduleResult::AllDone
    } else {
        ScheduleResult::Runnable(uncompleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(number: &str, completed: bool, deps: &[&str]) -> TaskEntry {
        TaskEntry {
            number: number.to_string(),
            title: format!("Task {}", number),
            completed,
            indent_level: 0,
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    // --- DAG mode tests (Task 4.1) ---

    #[test]
    fn dag_simple_chain() {
        // A -> B -> C: only A is runnable initially
        let tasks = vec![
            task("1", false, &[]),
            task("2", false, &["1"]),
            task("3", false, &["2"]),
        ];
        match select_runnable(&tasks, SchedulerMode::Dag) {
            ScheduleResult::Runnable(r) => {
                assert_eq!(
                    r.len(),
                    1,
                    "dag_simple_chain: only task 1 should be runnable"
                );
                assert_eq!(r[0].number, "1");
            }
            other => panic!("dag_simple_chain: expected Runnable, got {:?}", other),
        }
    }

    #[test]
    fn dag_parallel_after_common_dep() {
        // B and C depend on A; after A completes, both are runnable
        let tasks = vec![
            task("1", true, &[]),
            task("2", false, &["1"]),
            task("3", false, &["1"]),
        ];
        match select_runnable(&tasks, SchedulerMode::Dag) {
            ScheduleResult::Runnable(r) => {
                assert_eq!(
                    r.len(),
                    2,
                    "dag_parallel_after_common_dep: both tasks 2 and 3 should be runnable"
                );
                let numbers: Vec<&str> = r.iter().map(|t| t.number.as_str()).collect();
                assert!(numbers.contains(&"2"));
                assert!(numbers.contains(&"3"));
            }
            other => panic!(
                "dag_parallel_after_common_dep: expected Runnable, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn dag_blocked_when_deps_incomplete() {
        // All uncompleted tasks have unsatisfied deps
        let tasks = vec![task("1", false, &["2"]), task("2", false, &["1"])];
        match select_runnable(&tasks, SchedulerMode::Dag) {
            ScheduleResult::Blocked(diag) => {
                assert_eq!(
                    diag.blocked_tasks.len(),
                    2,
                    "dag_blocked_when_deps_incomplete: both tasks should be blocked"
                );
            }
            other => panic!(
                "dag_blocked_when_deps_incomplete: expected Blocked, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn dag_cycle_detected() {
        // A -> B -> A: circular dependency
        let tasks = vec![task("1", false, &["2"]), task("2", false, &["1"])];
        match select_runnable(&tasks, SchedulerMode::Dag) {
            ScheduleResult::Blocked(diag) => {
                assert!(
                    diag.has_cycle,
                    "dag_cycle_detected: should detect cycle when all deps are known"
                );
            }
            other => panic!("dag_cycle_detected: expected Blocked, got {:?}", other),
        }
    }

    #[test]
    fn dag_missing_dep_blocks_task() {
        // Task depends on non-existent task 99
        let tasks = vec![task("1", false, &["99"])];
        match select_runnable(&tasks, SchedulerMode::Dag) {
            ScheduleResult::Blocked(diag) => {
                assert_eq!(diag.blocked_tasks.len(), 1);
                assert_eq!(diag.blocked_tasks[0].missing_deps, vec!["99"]);
                assert!(
                    !diag.has_cycle,
                    "dag_missing_dep_blocks_task: should not detect cycle for external dep"
                );
            }
            other => panic!(
                "dag_missing_dep_blocks_task: expected Blocked, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn dag_no_deps_always_runnable() {
        let tasks = vec![
            task("1", false, &[]),
            task("2", false, &[]),
            task("3", false, &[]),
        ];
        match select_runnable(&tasks, SchedulerMode::Dag) {
            ScheduleResult::Runnable(r) => {
                assert_eq!(
                    r.len(),
                    3,
                    "dag_no_deps_always_runnable: all tasks should be runnable"
                );
            }
            other => panic!(
                "dag_no_deps_always_runnable: expected Runnable, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn dag_all_done() {
        let tasks = vec![task("1", true, &[]), task("2", true, &["1"])];
        assert!(
            matches!(
                select_runnable(&tasks, SchedulerMode::Dag),
                ScheduleResult::AllDone
            ),
            "dag_all_done: should return AllDone when all tasks completed"
        );
    }

    // --- Sequential mode tests (Task 4.2) ---

    #[test]
    fn sequential_ignores_deps() {
        // In Sequential mode, deps are ignored and all uncompleted tasks returned
        let tasks = vec![
            task("1", false, &["2"]),
            task("2", false, &["1"]),
            task("3", false, &[]),
        ];
        match select_runnable(&tasks, SchedulerMode::Sequential) {
            ScheduleResult::Runnable(r) => {
                assert_eq!(
                    r.len(),
                    3,
                    "sequential_ignores_deps: all uncompleted tasks should be returned"
                );
                assert_eq!(r[0].number, "1");
                assert_eq!(r[1].number, "2");
                assert_eq!(r[2].number, "3");
            }
            other => panic!(
                "sequential_ignores_deps: expected Runnable, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn sequential_all_done() {
        let tasks = vec![task("1", true, &[]), task("2", true, &["1"])];
        assert!(
            matches!(
                select_runnable(&tasks, SchedulerMode::Sequential),
                ScheduleResult::AllDone
            ),
            "sequential_all_done: should return AllDone when all tasks completed"
        );
    }
}
