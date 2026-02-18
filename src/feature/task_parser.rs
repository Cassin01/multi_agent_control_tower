use regex::Regex;

#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub number: String,
    #[allow(dead_code)]
    pub title: String,
    pub completed: bool,
    #[allow(dead_code)]
    pub indent_level: usize,
    #[allow(dead_code)]
    pub dependencies: Vec<String>,
}

/// Parse a task file and extract task entries.
///
/// Matches lines of the form `- [ ] N. Title` or `- [x] N. Title`
/// where N is an integer or dot-notation number (e.g. 1, 1.1, 2.3).
pub fn parse_tasks(content: &str) -> Vec<TaskEntry> {
    let re =
        Regex::new(r"^(\s*)- \[([ x])\] (\d+(?:\.\d+)*)\.\s+(.+?)(?:\s+\[deps:\s*([^\]]*)\])?\s*$")
            .unwrap();
    let mut tasks = Vec::new();

    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            let leading_ws = caps.get(1).unwrap().as_str();
            let indent_level = if leading_ws.is_empty() { 0 } else { 1 };
            let completed = &caps[2] == "x";
            let number = caps[3].to_string();
            let title = caps[4].to_string();
            let dependencies = caps
                .get(5)
                .map(|m| {
                    m.as_str()
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();

            tasks.push(TaskEntry {
                number,
                title,
                completed,
                indent_level,
                dependencies,
            });
        }
    }

    tasks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tasks_incomplete_tasks() {
        let content = "\
- [ ] 1. Create module structure
  - Description of task
- [ ] 2. Implement parser
  - Another description
";
        let tasks = parse_tasks(content);
        assert_eq!(
            tasks.len(),
            2,
            "parse_tasks: should find 2 incomplete tasks"
        );
        assert_eq!(tasks[0].number, "1");
        assert_eq!(tasks[0].title, "Create module structure");
        assert!(
            !tasks[0].completed,
            "parse_tasks: task 1 should be incomplete"
        );
        assert_eq!(tasks[1].number, "2");
        assert_eq!(tasks[1].title, "Implement parser");
        assert!(
            !tasks[1].completed,
            "parse_tasks: task 2 should be incomplete"
        );
    }

    #[test]
    fn parse_tasks_completed_tasks() {
        let content = "\
- [x] 1. Create module structure
  - Done
- [x] 2. Implement parser
  - Done
";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 2, "parse_tasks: should find 2 completed tasks");
        assert!(
            tasks[0].completed,
            "parse_tasks: task 1 should be completed"
        );
        assert!(
            tasks[1].completed,
            "parse_tasks: task 2 should be completed"
        );
    }

    #[test]
    fn parse_tasks_subtasks_with_dot_notation() {
        let content = "\
- [ ] 1. Main task
  - Description

  - [ ] 1.1. Sub-task one
    - Sub description
  - [ ] 1.2. Sub-task two
    - Sub description
";
        let tasks = parse_tasks(content);
        assert_eq!(
            tasks.len(),
            3,
            "parse_tasks: should find main task and 2 sub-tasks"
        );
        assert_eq!(tasks[0].number, "1");
        assert_eq!(
            tasks[0].indent_level, 0,
            "parse_tasks: main task indent_level should be 0"
        );
        assert_eq!(tasks[1].number, "1.1");
        assert_eq!(tasks[1].title, "Sub-task one");
        assert_eq!(
            tasks[1].indent_level, 1,
            "parse_tasks: sub-task indent_level should be 1"
        );
        assert_eq!(tasks[2].number, "1.2");
        assert_eq!(
            tasks[2].indent_level, 1,
            "parse_tasks: sub-task indent_level should be 1"
        );
    }

    #[test]
    fn parse_tasks_ignores_non_task_lines() {
        let content = "\
# Implementation Plan

## Overview

Some description text.

- [ ] 1. Actual task
  - Description
  - _Requirements: F1_

Random paragraph here.
";
        let tasks = parse_tasks(content);
        assert_eq!(
            tasks.len(),
            1,
            "parse_tasks: should only find the actual task line"
        );
        assert_eq!(tasks[0].number, "1");
        assert_eq!(tasks[0].title, "Actual task");
    }

    #[test]
    fn parse_tasks_mixed_completed_and_incomplete() {
        let content = "\
- [x] 1. Completed task
  - Done
- [ ] 2. Incomplete task
  - Not done
- [x] 3. Another completed
  - Done
- [ ] 4. Another incomplete
  - Not done
";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 4, "parse_tasks: should find all 4 tasks");
        assert!(
            tasks[0].completed,
            "parse_tasks: task 1 should be completed"
        );
        assert!(
            !tasks[1].completed,
            "parse_tasks: task 2 should be incomplete"
        );
        assert!(
            tasks[2].completed,
            "parse_tasks: task 3 should be completed"
        );
        assert!(
            !tasks[3].completed,
            "parse_tasks: task 4 should be incomplete"
        );
    }

    #[test]
    fn parse_tasks_empty_content() {
        let tasks = parse_tasks("");
        assert!(
            tasks.is_empty(),
            "parse_tasks: empty content should return empty vec"
        );
    }

    #[test]
    fn parse_tasks_no_task_lines() {
        let content = "\
# Just a heading

Some text without any task lines.
- A regular list item
- Another list item
";
        let tasks = parse_tasks(content);
        assert!(
            tasks.is_empty(),
            "parse_tasks: content without task lines should return empty vec"
        );
    }

    #[test]
    fn parse_tasks_with_deps() {
        let content = "\
- [ ] 1. Setup database
- [ ] 2. Create API [deps: 1]
- [ ] 3. Build frontend [deps: 1, 2]
";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 3, "parse_tasks_with_deps: should find 3 tasks");
        assert!(
            tasks[0].dependencies.is_empty(),
            "parse_tasks_with_deps: task 1 should have no deps"
        );
        assert_eq!(
            tasks[1].dependencies,
            vec!["1"],
            "parse_tasks_with_deps: task 2 should depend on 1"
        );
        assert_eq!(
            tasks[2].dependencies,
            vec!["1", "2"],
            "parse_tasks_with_deps: task 3 should depend on 1 and 2"
        );
    }

    #[test]
    fn parse_tasks_without_deps() {
        let content = "\
- [ ] 1. Task one
- [ ] 2. Task two
";
        let tasks = parse_tasks(content);
        assert_eq!(
            tasks.len(),
            2,
            "parse_tasks_without_deps: should find 2 tasks"
        );
        assert!(
            tasks[0].dependencies.is_empty(),
            "parse_tasks_without_deps: task 1 should have empty deps"
        );
        assert!(
            tasks[1].dependencies.is_empty(),
            "parse_tasks_without_deps: task 2 should have empty deps"
        );
    }

    #[test]
    fn parse_tasks_mixed_deps_and_no_deps() {
        let content = "\
- [ ] 1. Independent task
- [ ] 2. Dependent task [deps: 1]
- [ ] 3. Another independent
";
        let tasks = parse_tasks(content);
        assert_eq!(
            tasks.len(),
            3,
            "parse_tasks_mixed_deps_and_no_deps: should find 3 tasks"
        );
        assert!(tasks[0].dependencies.is_empty());
        assert_eq!(tasks[1].dependencies, vec!["1"]);
        assert!(tasks[2].dependencies.is_empty());
    }

    #[test]
    fn parse_tasks_deps_with_dot_notation() {
        let content = "\
- [ ] 1. Main task
  - [ ] 1.1. Sub task
- [ ] 2. Depends on sub [deps: 1.1, 2.3]
";
        let tasks = parse_tasks(content);
        let task2 = tasks.iter().find(|t| t.number == "2").unwrap();
        assert_eq!(
            task2.dependencies,
            vec!["1.1", "2.3"],
            "parse_tasks_deps_with_dot_notation: should parse dot-notation deps"
        );
    }

    #[test]
    fn parse_tasks_deps_empty_bracket() {
        let content = "- [ ] 1. Task with empty deps [deps: ]\n";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 1);
        assert!(
            tasks[0].dependencies.is_empty(),
            "parse_tasks_deps_empty_bracket: empty [deps: ] should produce empty dependencies"
        );
    }

    #[test]
    fn parse_tasks_deps_whitespace_variants() {
        let content = "\
- [ ] 1. Tight [deps:1,2]
- [ ] 2. Spaced [deps: 1 , 2 ]
- [ ] 3. Extra spaces [deps:  1  ,  2  ]
";
        let tasks = parse_tasks(content);
        assert_eq!(
            tasks[0].dependencies,
            vec!["1", "2"],
            "parse_tasks_deps_whitespace_variants: tight spacing"
        );
        assert_eq!(
            tasks[1].dependencies,
            vec!["1", "2"],
            "parse_tasks_deps_whitespace_variants: normal spacing"
        );
        assert_eq!(
            tasks[2].dependencies,
            vec!["1", "2"],
            "parse_tasks_deps_whitespace_variants: extra spacing"
        );
    }

    #[test]
    fn parse_tasks_multi_level_dot_notation() {
        let content = "\
- [ ] 1. Root
  - [ ] 1.2. Mid
    - [ ] 1.2.3. Leaf
- [ ] 2. Depends on leaf [deps: 1.2.3]
";
        let tasks = parse_tasks(content);
        let leaf = tasks.iter().find(|t| t.number == "1.2.3").unwrap();
        assert_eq!(
            leaf.number, "1.2.3",
            "parse_tasks_multi_level_dot_notation: should parse multi-level number"
        );
        let task2 = tasks.iter().find(|t| t.number == "2").unwrap();
        assert_eq!(
            task2.dependencies,
            vec!["1.2.3"],
            "parse_tasks_multi_level_dot_notation: should parse multi-level dep reference"
        );
    }

    #[test]
    fn parse_tasks_title_preserved_with_deps() {
        let content = "- [ ] 1. Build the API server [deps: 2, 3]\n";
        let tasks = parse_tasks(content);
        assert_eq!(
            tasks[0].title, "Build the API server",
            "parse_tasks_title_preserved_with_deps: title should not include [deps: ...]"
        );
    }

    #[test]
    fn parse_tasks_real_world_format() {
        let content = "\
# Implementation Plan: Feature Executor

## Tasks

- [x] 1. Create `src/feature/mod.rs` module declaration
  - Create `src/feature/` directory
  - Add `mod.rs` with `pub mod executor;` and `pub mod task_parser;`
  - _Requirements: F4, NF1_

- [ ] 2. Implement `TaskEntry` struct and task file parser
  - Define `TaskEntry` struct
  - Implement `parse_tasks(content: &str) -> Vec<TaskEntry>` function
  - _Requirements: F4_

- [ ] 2.1. Write tests for task file parser
  - Test parsing `- [ ]` lines as incomplete tasks
  - _Requirements: F4_

- [ ] 3. Checkpoint - Parser correctness
  - Run `make test` to ensure all parser tests pass.
";
        let tasks = parse_tasks(content);
        assert_eq!(
            tasks.len(),
            4,
            "parse_tasks: should find 4 tasks in real-world format"
        );
        assert!(
            tasks[0].completed,
            "parse_tasks: task 1 should be completed"
        );
        assert!(
            !tasks[1].completed,
            "parse_tasks: task 2 should be incomplete"
        );
        assert_eq!(tasks[1].number, "2");
        assert_eq!(tasks[2].number, "2.1");
        assert_eq!(tasks[3].number, "3");
    }
}
