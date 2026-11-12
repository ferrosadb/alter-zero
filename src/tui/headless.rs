//! Assembling a **headless** run (`docs/cli.md` `-p/--print`): the same reply
//! backend the TUI builds, with none of the terminal.
//!
//! [`bootstrap`](super::bootstrap) is one long sequence whose order is the
//! behaviour of a *screen* — the banner, the first frame, the startup review,
//! the picker. A stream has none of that, so this module builds the pieces the
//! agent loop actually needs and stops there: the provider/backend selection,
//! the scratchpad, the skills and subagent rosters the system prompt lists,
//! and the shared gates.
//!
//! What it deliberately leaves out, and why each is safe:
//!
//! - **No permission gate.** Nobody can answer a modal, so the backend runs
//!   tools unasked — `ALTER_ZERO_PERMISSIONS=0`'s documented behaviour. A turn
//!   can therefore never park on a prompt that will never come.
//! - **No MCP servers.** A voice turn should not spawn server processes (and
//!   their auth flows) on a phone's behalf.
//! - **No lifecycle hooks.** They run commands; an unattended stream is not a
//!   place to fire a user's `PreToolUse` guard that expects a human.
//! - **No recorder, checkpoints, telemetry or update check.** The rollout file
//!   is the only one worth debating: `--resume {id}` still *reads* one (the
//!   resolution happens before this, in cooked mode), and the conversation
//!   continues in memory for the life of the process.
//!
//! The ask gate **is** built and handed to the loop, which declines any
//! question on the absent user's behalf rather than letting the tool thread
//! park forever.

use std::io::{self, IsTerminal};
use std::path::PathBuf;

use alter_zero::cli::{HelpPage, HelpStyle};
use alter_zero::headless::{self, Options};

use super::config;
use super::host;
use super::models::ModelSession;
use super::startup::Startup;

/// Run the headless stream to completion. stdout is NDJSON and nothing else;
/// every diagnostic belongs on stderr.
///
/// # Errors
/// Only stdout's failures (a closed pipe, a full disk) — a backend error is a
/// record in the stream, and the run carries on to the next turn.
pub(crate) async fn run(startup: Startup) -> io::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let project = cwd.display().to_string();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    // Minted once, like the TUI's: it names this run's temporary tree and is
    // the id the first record reports, so a caller can resume the session.
    let session_id = host::session_id();
    let session_tmp = config::session_tmp_root(&session_id);
    let scratchpad = config::prepare_scratchpad(&session_tmp);

    // The user's secrets, so a stored placeholder in the prompt expands and a
    // tool's output is redacted exactly as it is interactively.
    let (secrets, secrets_error) = match config::secrets_json_path()
        .map(|path| alter_zero::llm::keystore::load_secrets_file(&path))
    {
        Some(Ok(store)) => (store, None),
        Some(Err(error)) => (alter_zero::secrets::SecretStore::new(), Some(error)),
        None => (alter_zero::secrets::SecretStore::new(), None),
    };
    let secrets = alter_zero::secrets::SecretRegistry::new(secrets);
    if let Some(error) = secrets_error {
        eprintln!("warning: {error}");
    }

    // Both registries are attachments the backend tool set is built around.
    // Their channels are never drained here — a background shell or subagent
    // launched mid-turn simply reports into a stream nobody reads, and the
    // process is done when stdin is.
    let (bg_tx, _bg_rx) = tokio::sync::mpsc::unbounded_channel();
    let registry = alter_zero::background::BackgroundRegistry::new(
        bg_tx,
        alter_zero::scratchpad::tasks_dir(&session_tmp),
    )
    .with_detach_helper(std::env::current_exe().ok())
    .with_secrets(secrets);
    let (agent_tx, _agent_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent_registry = alter_zero::agents::AgentRegistry::new(agent_tx);

    // The rosters the system prompt lists, read from the same roots the TUI
    // reads them from — so a headless turn sees the same skills and subagent
    // types an interactive one would.
    let config_home = config::config_home();
    let (found_skills, _) = alter_zero::llm::skill::load_skills(&cwd, config_home.as_deref());
    let skills = alter_zero::skills::SkillRegistry::new(found_skills);
    skills.set_disabled(
        config::load_skills_file(config::skills_json_path().as_deref())
            .disabled_for(&project),
    );
    let (found_agents, _) = alter_zero::llm::subagent::load_agents(&cwd, config_home.as_deref());
    let subagents = alter_zero::subagents::SubagentRegistry::new(found_agents);

    let ask = alter_zero::ask::AskGate::new();
    let tasks = alter_zero::tasks::TaskRegistry::new();
    let steer = alter_zero::steer::SteerQueue::new();

    // The `/settings` knobs, this directory's entry with the `ALTER_ZERO_*`
    // overrides on top — the rule every other entry point follows.
    let settings = config::apply_setting_overrides(
        config::load_settings_file(config::settings_json_path().as_deref()).settings_for(&project),
    );

    let models = ModelSession::resolve(
        &cwd,
        home.as_deref(),
        scratchpad.as_deref(),
        &registry,
        &agent_registry,
        &steer,
        // No gate: nothing can ask (`docs/permissions.md`).
        None,
        &ask,
        &tasks,
        &skills,
        &subagents,
        // No MCP manager, no hook sink: see the module doc.
        None,
        &settings,
        None,
    );

    if models.is_offline() {
        eprintln!(
            "warning: no provider or API key resolved — answering from the built-in dummy backend"
        );
    }

    // Where further turns come from. A pipe is a conversation; a terminal is
    // not — reading it would hang the run behind a prompt that never comes.
    let stdin = io::stdin();
    let piped = !stdin.is_terminal();
    if startup.prompt.is_none() && !piped {
        eprintln!(
            "{}",
            alter_zero::cli::usage_error(
                HelpPage::Main,
                "no PROMPT to send and stdin is a terminal (quote a prompt, or pipe turns on stdin)",
                HelpStyle::Plain,
            )
        );
        std::process::exit(2);
    }

    let model = models.model_name();
    let options = Options {
        model: &model,
        session_id: &session_id,
        cwd: &cwd,
        ask: Some(&ask),
    };
    let stdout = io::stdout();
    let turns = piped.then(|| stdin.lock());
    headless::run(models.backend(), options, startup.prompt, turns, stdout.lock()).await
}
