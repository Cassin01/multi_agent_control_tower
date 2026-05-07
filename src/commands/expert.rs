//! `macot expert <subcommand>` — dynamic expert management surface.
//!
//! Wraps [`crate::expert::add::ExpertAddService`] so users can add an
//! expert to a running session without a full `down` / `start` cycle.
//! See dynamic-expert-add-design.md §3.6.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use serde::Serialize;

use crate::commands::common::resolve_existing_session;
use crate::expert::add::{ExpertAddRequest, ExpertAddService, ExpertAdded};
use crate::expert::role::{BuiltinRole, RoleSpec};
use crate::experts::persist::ManifestPersistor;
use crate::session::TmuxManager;

#[derive(ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    pub command: ExpertCmd,
}

#[derive(Subcommand)]
pub enum ExpertCmd {
    /// Add a new expert to a running session.
    Add(AddArgs),

    /// List experts in a session.
    List(ListArgs),
}

#[derive(ClapArgs)]
pub struct AddArgs {
    /// Session name (optional if exactly one session is running).
    #[arg(short, long)]
    pub session: Option<String>,

    /// Role spec: `architect` | `planner` | `general` | custom name.
    #[arg(short, long, default_value = "general")]
    pub role: String,

    /// User-supplied display name. Auto-picked from the literary pool
    /// when omitted.
    #[arg(short, long)]
    pub name: Option<String>,

    /// Custom role template path (overrides role lookup).
    #[arg(long)]
    pub prompt_file: Option<PathBuf>,

    /// Launch the new expert directly inside a git worktree
    /// (delegates to the existing Ctrl+W flow).
    #[arg(long)]
    pub worktree: bool,

    /// Branch name for the worktree (defaults to a sanitised role).
    #[arg(long)]
    pub worktree_branch: Option<String>,

    /// Validate inputs and report planned ID/files without writing or
    /// spawning.
    #[arg(long)]
    pub dry_run: bool,

    /// Emit the [`ExpertAdded`] outcome as JSON on stdout (suppresses the
    /// human-readable line).
    #[arg(long)]
    pub json: bool,
}

#[derive(ClapArgs)]
pub struct ListArgs {
    /// Session name (optional if exactly one session is running).
    #[arg(short, long)]
    pub session: Option<String>,
}

pub async fn execute(args: Args) -> Result<()> {
    match args.command {
        ExpertCmd::Add(add) => add_command(add).await,
        ExpertCmd::List(list) => list_command(list).await,
    }
}

#[derive(Debug, Serialize)]
struct ExpertAddedJson<'a> {
    session: &'a str,
    expert_id: u32,
    name: &'a str,
    role: &'a str,
    tmux_window_index: u32,
}

impl<'a> From<&'a ExpertAdded> for ExpertAddedJson<'a> {
    fn from(value: &'a ExpertAdded) -> Self {
        Self {
            session: &value.session,
            expert_id: value.expert_id,
            name: &value.name,
            role: &value.role,
            tmux_window_index: value.tmux_window_index,
        }
    }
}

async fn add_command(args: AddArgs) -> Result<()> {
    let role_spec = build_role_spec(&args)?;

    if args.dry_run {
        return print_dry_run(&args, &role_spec).await;
    }

    let (tmux, metadata) = resolve_existing_session(args.session.clone()).await?;
    let session_name = tmux.session_name().to_string();
    let project_path = metadata
        .project_path
        .ok_or_else(|| anyhow!("session metadata is missing project_path"))?;
    let project_root = PathBuf::from(&project_path);
    let session_hash = session_name
        .strip_prefix("macot-")
        .unwrap_or(&session_name)
        .to_string();

    let svc = ExpertAddService::new(
        project_root.clone(),
        session_name.clone(),
        session_hash,
        tmux,
    );

    let req = ExpertAddRequest {
        role: role_spec,
        name: args.name.clone(),
    };
    let added = svc
        .add_expert(req)
        .await
        .with_context(|| "expert add failed")?;

    if args.worktree {
        eprintln!(
            "Note: --worktree is not yet supported from the CLI. \
             Use Ctrl+W inside the tower TUI to convert expert {} into a worktree expert.",
            added.expert_id
        );
    }

    if args.json {
        let payload = ExpertAddedJson::from(&added);
        let line =
            serde_json::to_string(&payload).context("failed to serialize ExpertAdded as JSON")?;
        println!("{line}");
    } else {
        println!(
            "Added expert {id} ({name}, {role}) in session {sess} (window {idx})",
            id = added.expert_id,
            name = added.name,
            role = added.role,
            sess = added.session,
            idx = added.tmux_window_index,
        );
    }

    Ok(())
}

async fn list_command(args: ListArgs) -> Result<()> {
    let session_name = match args.session {
        Some(name) => {
            let tmux = TmuxManager::new(name.clone());
            if !tmux.session_exists().await {
                bail!("Session {name} does not exist");
            }
            name
        }
        None => match crate::commands::common::resolve_single_session_default().await {
            Ok(name) => name,
            Err(_) => {
                let cwd = std::env::current_dir()?;
                if !cwd.join(".macot").join("experts_manifest.json").exists() {
                    bail!(
                        "No running macot sessions and no manifest at {}",
                        cwd.display()
                    );
                }
                String::new()
            }
        },
    };

    let project_root = if session_name.is_empty() {
        std::env::current_dir()?
    } else {
        let tmux = TmuxManager::new(session_name.clone());
        let metadata = tmux.load_session_metadata().await?;
        metadata
            .project_path
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().expect("cwd"))
    };

    let persistor = ManifestPersistor::new(project_root);
    let entries = persistor
        .load_entries()
        .context("failed to read experts_manifest.json")?;

    if entries.is_empty() {
        println!("No experts registered.");
        return Ok(());
    }

    println!("{:>3}  {:<16}  {:<16}  WORKTREE", "ID", "NAME", "ROLE",);
    println!("{}", "-".repeat(60));
    for e in entries {
        let wt = e.worktree_path.unwrap_or_default();
        println!("{:>3}  {:<16}  {:<16}  {}", e.expert_id, e.name, e.role, wt);
    }

    Ok(())
}

fn build_role_spec(args: &AddArgs) -> Result<RoleSpec> {
    if let Some(path) = &args.prompt_file {
        bail!(
            "--prompt-file is not yet supported (got {}). Use a custom role under \
             .macot/templates/roles/ instead.",
            path.display()
        );
    }
    if let Ok(builtin) = args.role.parse::<BuiltinRole>() {
        return Ok(RoleSpec::Builtin(builtin));
    }
    Ok(RoleSpec::Custom {
        name: args.role.clone(),
    })
}

async fn print_dry_run(args: &AddArgs, role_spec: &RoleSpec) -> Result<()> {
    // dry-run resolves session (if running) so the planned ID is
    // accurate, but writes nothing and never touches tmux.
    let (project_root, session_name) = match resolve_existing_session(args.session.clone()).await {
        Ok((tmux, metadata)) => {
            let session_name = tmux.session_name().to_string();
            let project_root = metadata
                .project_path
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
            (project_root, session_name)
        }
        Err(_) => (std::env::current_dir()?, "<no-running-session>".to_string()),
    };

    let persistor = ManifestPersistor::new(project_root.clone());
    let entries = persistor.load_entries().unwrap_or_default();
    let next_id = entries
        .iter()
        .map(|e| e.expert_id)
        .max()
        .map_or(0, |m| m + 1);

    let prompt_path = project_root
        .join(".macot")
        .join("system_prompt")
        .join(format!("expert{next_id}.md"));
    let template_source = match role_spec {
        RoleSpec::Builtin(b) => format!("builtin:{}", b.canonical_name()),
        RoleSpec::Custom { name } => format!("custom:{name}"),
    };
    let auto_name = match &args.name {
        Some(n) => n.clone(),
        None => format!("<auto-pick (next available literary name or Expert{next_id:02})>"),
    };

    if args.json {
        let role_canonical = match role_spec {
            RoleSpec::Builtin(b) => b.canonical_name().to_string(),
            RoleSpec::Custom { name } => name.clone(),
        };
        let prompt_str = prompt_path.to_string_lossy().to_string();
        let dry = DryRunJson {
            session: &session_name,
            planned_expert_id: next_id,
            planned_name: &auto_name,
            planned_role: &role_canonical,
            template_source: &template_source,
            prompt_path: &prompt_str,
        };
        println!(
            "{}",
            serde_json::to_string(&dry).context("failed to serialize dry-run JSON")?
        );
    } else {
        println!("DRY RUN — no state files written, no tmux operations performed.");
        println!("  session         : {session_name}");
        println!("  planned id      : {next_id}");
        println!("  planned name    : {auto_name}");
        println!("  template source : {template_source}");
        println!("  prompt path     : {}", prompt_path.display());
    }
    Ok(())
}

#[derive(Serialize)]
struct DryRunJson<'a> {
    session: &'a str,
    planned_expert_id: u32,
    planned_name: &'a str,
    planned_role: &'a str,
    template_source: &'a str,
    prompt_path: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experts::persist::ExpertEntry;

    fn add_args(name: Option<&str>, role: &str, dry_run: bool, json: bool) -> AddArgs {
        AddArgs {
            session: None,
            role: role.to_string(),
            name: name.map(str::to_string),
            prompt_file: None,
            worktree: false,
            worktree_branch: None,
            dry_run,
            json,
        }
    }

    #[test]
    fn build_role_spec_resolves_builtin_general() {
        let spec = build_role_spec(&add_args(None, "general", true, false)).unwrap();
        assert!(matches!(spec, RoleSpec::Builtin(BuiltinRole::General)));
    }

    #[test]
    fn build_role_spec_resolves_builtin_architect() {
        let spec = build_role_spec(&add_args(None, "architect", true, false)).unwrap();
        assert!(matches!(spec, RoleSpec::Builtin(BuiltinRole::Architect)));
    }

    #[test]
    fn build_role_spec_falls_back_to_custom_for_unknown_role() {
        let spec = build_role_spec(&add_args(None, "qa-bot", true, false)).unwrap();
        match spec {
            RoleSpec::Custom { name } => assert_eq!(name, "qa-bot"),
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn build_role_spec_rejects_prompt_file_for_now() {
        let mut args = add_args(None, "general", true, false);
        args.prompt_file = Some(PathBuf::from("/tmp/role.md"));
        let err = build_role_spec(&args).expect_err("prompt-file should be rejected");
        assert!(
            err.to_string().contains("prompt-file"),
            "error should mention prompt-file, got: {err}"
        );
    }

    #[test]
    fn expert_added_json_serializes_with_snake_case_fields() {
        let added = ExpertAdded {
            session: "macot-deadbeef".to_string(),
            expert_id: 4,
            name: "Smerdyakov".to_string(),
            role: "general".to_string(),
            tmux_window_index: 4,
        };
        let payload = ExpertAddedJson::from(&added);
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"session\":\"macot-deadbeef\""));
        assert!(json.contains("\"expert_id\":4"));
        assert!(json.contains("\"name\":\"Smerdyakov\""));
        assert!(json.contains("\"role\":\"general\""));
        assert!(json.contains("\"tmux_window_index\":4"));
    }

    #[tokio::test]
    async fn dry_run_does_not_touch_filesystem() {
        // No tmux session, no manifest — exercises the fallback path
        // where `resolve_existing_session` fails and we fall back to
        // cwd-based reasoning.
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let args = add_args(Some("Smerdyakov"), "general", true, true);
        let role = build_role_spec(&args).unwrap();
        let result = print_dry_run(&args, &role).await;

        std::env::set_current_dir(cwd).unwrap();

        assert!(result.is_ok(), "dry-run should succeed: {result:?}");
        // Dry-run must not create the manifest, system_prompt dir, or
        // status dir.
        assert!(
            !tmp.path()
                .join(".macot")
                .join("experts_manifest.json")
                .exists(),
            "dry-run leaked a manifest file"
        );
        assert!(
            !tmp.path().join(".macot").join("system_prompt").exists(),
            "dry-run leaked the system_prompt directory"
        );
    }

    #[test]
    fn dry_run_planned_id_uses_max_plus_one() {
        // Pure derivation test — exercises the same `next_id` math used
        // in `print_dry_run` against a synthetic manifest.
        let entries = vec![
            ExpertEntry {
                expert_id: 0,
                name: "Alyosha".to_string(),
                role: "architect".to_string(),
                worktree_path: None,
            },
            ExpertEntry {
                expert_id: 3,
                name: "Ilyusha".to_string(),
                role: "planner".to_string(),
                worktree_path: None,
            },
        ];
        let next_id: u32 = entries
            .iter()
            .map(|e| e.expert_id)
            .max()
            .map_or(0, |m| m + 1);
        assert_eq!(next_id, 4);
    }
}
