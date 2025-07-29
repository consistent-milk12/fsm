// fsm-core/src/controller/action_dispatcher/command_dispatcher.rs
// Unified command execution with validation

use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, MutexGuard, RwLock, RwLockReadGuard};
use tokio::fs as TokioFs;

use crate::controller::Action;
use crate::controller::actions::InputPromptType;
use crate::controller::state_provider::StateProvider;
use crate::fs::object_info::ObjectInfo;
use crate::model::ui_state::{RedrawFlag, UIOverlay, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

#[derive(Clone, Debug)]
pub struct CommandDispatcher {
    state_provider: Arc<dyn StateProvider>,
}

impl CommandDispatcher {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    fn parse_command(&self, command: &str) -> Result<(String, Vec<String>)> {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            anyhow::bail!("Empty command");
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let cmd = parts[0].to_string();
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        Ok((cmd, args))
    }

    async fn execute_command(&self, command: &str) -> Result<()> {
        let (cmd, args) = self.parse_command(command)?;

        match cmd.as_str() {
            "cd" => self.handle_cd(args).await,
            "mkdir" => self.handle_mkdir(args).await,
            "touch" => self.handle_touch(args).await,
            "reload" => self.handle_reload().await,
            "pwd" => self.handle_pwd(),
            "quit" | "q" => Err(anyhow::anyhow!("User requested quit")),
            "find" => self.handle_find(args),
            _ => anyhow::bail!("Unknown command: {}", cmd),
        }
    }

    async fn handle_cd(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: cd <path>");
        }

        let target_path = PathBuf::from(&args[0]);
        if !target_path.exists() || !target_path.is_dir() {
            anyhow::bail!("Invalid directory: {}", target_path.display());
        }

        let entries = self.load_directory(&target_path).await?;

        {
            let mut fs = self.state_provider.fs_state();
            let pane = fs.active_pane_mut();
            pane.cwd = target_path;
            pane.entries = entries;
            pane.selected.store(0, Ordering::Relaxed);
        }

        self.success("Directory changed");
        Ok(())
    }

    async fn handle_mkdir(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: mkdir <name>");
        }

        let (current_dir, new_dir) = {
            let fs = self.state_provider.fs_state();
            let current_dir = fs.active_pane().cwd.clone();
            (current_dir.clone(), current_dir.join(&args[0]))
        };

        TokioFs::create_dir(&new_dir)
            .await
            .with_context(|| format!("Failed to create directory: {}", args[0]))?;

        self.success(&format!("Created directory: {}", args[0]));

        // Reload
        let entries = self.load_directory(&current_dir).await?;
        {
            let mut fs = self.state_provider.fs_state();
            fs.active_pane_mut().entries = entries;
        }

        Ok(())
    }

    async fn handle_touch(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: touch <filename>");
        }

        let (current_dir, new_file) = {
            let fs = self.state_provider.fs_state();
            let current_dir = fs.active_pane().cwd.clone();
            (current_dir.clone(), current_dir.join(&args[0]))
        };

        TokioFs::File::create(&new_file)
            .await
            .with_context(|| format!("Failed to create file: {}", args[0]))?;

        self.success(&format!("Created file: {}", args[0]));

        // Reload
        let entries = self.load_directory(&current_dir).await?;
        {
            let mut fs = self.state_provider.fs_state();
            fs.active_pane_mut().entries = entries;
        }

        Ok(())
    }

    async fn handle_reload(&self) -> Result<()> {
        let current_dir = {
            let fs = self.state_provider.fs_state();
            fs.active_pane().cwd.clone()
        };

        let entries = self.load_directory(&current_dir).await?;
        {
            let mut fs = self.state_provider.fs_state();
            fs.active_pane_mut().entries = entries;
        }

        self.success("Directory reloaded");
        Ok(())
    }

    fn handle_pwd(&self) -> Result<()> {
        let current_dir = {
            let fs = self.state_provider.fs_state();
            fs.active_pane().cwd.clone()
        };

        self.info(&format!("Current: {}", current_dir.display()));
        Ok(())
    }

    fn handle_find(&self, args: Vec<String>) -> Result<()> {
        if args.is_empty() {
            anyhow::bail!("Usage: find <pattern>");
        }

        let pattern: &String = &args[0];
        let results: Vec<ObjectInfo> = self.search_files(pattern);
        let count: usize = results.len();

        {
            let mut fs: MutexGuard<'_, crate::FSState> = self.state_provider.fs_state();
            fs.active_pane_mut().search_results = results;
        }

        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.overlay = UIOverlay::SearchResults;
                ui.request_redraw(RedrawFlag::All);
            }));

        self.info(&format!("Found {count} matches"));
        Ok(())
    }

    async fn load_directory(&self, dir: &std::path::Path) -> Result<Vec<ObjectInfo>> {
        let mut entries: Vec<ObjectInfo> = Vec::new();
        let mut dir_reader: TokioFs::ReadDir = TokioFs::read_dir(dir).await?;

        while let Some(entry) = dir_reader.next_entry().await? {
            let path: PathBuf = entry.path();

            if let Some(name) = path.file_name().and_then(|n: &OsStr| n.to_str())
                && !name.starts_with('.')
                && let Ok(info) = ObjectInfo::from_path_light(&path).await
            {
                entries.push(ObjectInfo::with_placeholder_metadata(info));
            }
        }

        Ok(entries)
    }

    fn search_files(&self, query: &str) -> Vec<ObjectInfo> {
        let fs: MutexGuard<'_, crate::FSState> = self.state_provider.fs_state();
        let query_lower: String = query.to_lowercase();

        fs.active_pane()
            .entries
            .iter()
            .filter(|entry: &&ObjectInfo| entry.name.to_lowercase().contains(&query_lower))
            .cloned()
            .collect()
    }

    async fn handle_submit_input(&self, input: String) -> Result<DispatchResult> {
        let prompt_type: Option<InputPromptType> = {
            let binding: Arc<RwLock<UIState>> = self.state_provider.ui_state();

            let ui: RwLockReadGuard<'_, UIState> = binding.read().expect("UI lock poisoned");

            ui.input_prompt_type.clone()
        };

        match prompt_type {
            Some(InputPromptType::Custom(ref name)) if name == "command" => {
                if !input.is_empty() {
                    if let Err(e) = self.execute_command(&input).await {
                        if e.to_string().contains("quit") {
                            return Ok(DispatchResult::Terminate);
                        }
                        self.error(&format!("Command failed: {e}"));
                    }
                }
                self.close_overlay();
                Ok(DispatchResult::Continue)
            }
            Some(InputPromptType::CreateFile) => {
                if !input.is_empty() {
                    if let Err(e) = self.execute_command(&format!("touch {input}")).await {
                        self.error(&format!("Failed to create file: {e}"));
                    }
                }
                self.close_overlay();
                Ok(DispatchResult::Continue)
            }
            Some(InputPromptType::CreateDirectory) => {
                if !input.is_empty() {
                    if let Err(e) = self.execute_command(&format!("mkdir {input}")).await {
                        self.error(&format!("Failed to create directory: {e}"));
                    }
                }
                self.close_overlay();
                Ok(DispatchResult::Continue)
            }
            _ => {
                self.close_overlay();
                Ok(DispatchResult::Continue)
            }
        }
    }

    fn close_overlay(&self) {
        self.state_provider
            .update_ui_state(Box::new(|ui: &mut UIState| {
                ui.overlay = UIOverlay::None;
                ui.input_prompt_type = None;
                ui.request_redraw(RedrawFlag::All);
            }));
    }

    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            Action::SubmitInputPrompt(input) => self.handle_submit_input(input).await,
            _ => Ok(DispatchResult::NotHandled),
        }
    }

    fn success(&self, msg: &str) {
        let msg = msg.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.success(&msg);
            }));
    }

    fn error(&self, msg: &str) {
        let msg = msg.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.error(&msg);
            }));
    }

    fn info(&self, msg: &str) {
        let msg = msg.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.info(&msg);
            }));
    }
}

impl ActionMatcher for CommandDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(action, Action::SubmitInputPrompt(_))
    }

    async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        self.handle(action).await
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "command"
    }
}
