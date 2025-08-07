//!
//!  ``src/model/command_palette.rs``
//!

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPaletteState {
    pub input: String,
    pub filtered: Vec<Command>,
    pub all_commands: Vec<Command>,
    pub selected: usize,
    pub completions: Vec<String>,
    pub completion_index: usize,
    pub show_completions: bool,
}

impl CommandPaletteState {
    #[must_use]
    pub fn new(commands: Vec<Command>) -> Self {
        Self {
            input: String::new(),
            filtered: commands.clone(),
            all_commands: commands,
            selected: 0,
            completions: Vec::new(),
            completion_index: 0,
            show_completions: false,
        }
    }

    /// Call this on input change to filter commands and update completions.
    pub fn update_filter(&mut self) {
        let q = self.input.to_lowercase();
        self.filtered = self
            .all_commands
            .iter()
            .filter(|cmd| cmd.title.to_lowercase().contains(&q))
            .cloned()
            .collect();
        self.selected = 0;
        self.update_completions();
    }

    /// Update auto-completion suggestions based on current input
    pub fn update_completions(&mut self) {
        self.completions.clear();
        self.completion_index = 0;
        self.show_completions = false;

        if self.input.trim().is_empty() {
            return;
        }

        let input_parts: Vec<&str> = self.input.split_whitespace().collect();
        if input_parts.is_empty() {
            return;
        }

        let command_part = input_parts[0].to_lowercase();

        // Get all available command keywords
        let mut candidates = Vec::new();

        // Add built-in command aliases (canonical commands only)
        let built_in_commands = ["nf", "nd", "reload", "grep", "config"];

        for cmd in &built_in_commands {
            if cmd.starts_with(&command_part) {
                candidates.push((*cmd).to_string());
            }
        }

        // Only add commands from all_commands if they don't have built-in aliases
        for cmd in &self.all_commands {
            let cmd_lower = cmd.title.to_lowercase().replace(' ', "");
            let cmd_key = match cmd_lower.as_str() {
                "openconfig" => "config", // Map to built-in alias
                _ => continue,            // Skip other commands that have built-in aliases
            };

            if cmd_key.starts_with(&command_part) && !candidates.contains(&cmd_key.to_string()) {
                candidates.push(cmd_key.to_string());
            }
        }

        // Sort and limit completions
        candidates.sort();
        candidates.dedup(); // Remove duplicates
        self.completions = candidates.into_iter().take(10).collect();

        // Always show completions if we have any matches
        self.show_completions = !self.completions.is_empty();

        // Debug logging for single match issue
        if self.completions.len() == 1 {
            tracing::debug!(
                "Single completion found: {:?}, show_completions: {}",
                self.completions,
                self.show_completions
            );
        }
    }

    /// Apply the currently selected completion
    pub fn apply_completion(&mut self) {
        tracing::debug!(
            "apply_completion called: index={}, completions={:?}",
            self.completion_index,
            self.completions
        );

        if self.completion_index < self.completions.len() {
            let completion = &self.completions[self.completion_index];
            let input_parts: Vec<&str> = self.input.split_whitespace().collect();

            tracing::debug!(
                "Before apply: input='{0}', completion='{completion}'",
                self.input,
            );

            if input_parts.is_empty() {
                self.input = completion.clone();
            } else {
                // Replace the first part with the completion
                let mut new_parts = vec![completion.as_str()];
                if input_parts.len() > 1 {
                    new_parts.extend(&input_parts[1..]);
                }
                self.input = new_parts.join(" ");
            }

            tracing::debug!("After apply: input='{}', hiding completions", self.input);
            self.show_completions = false;
            self.update_filter();
        }
    }

    /// Move to next completion suggestion
    pub const fn next_completion(&mut self) {
        if !self.completions.is_empty() {
            self.completion_index = (self.completion_index + 1) % self.completions.len();
        }
    }

    /// Move to previous completion suggestion
    pub const fn prev_completion(&mut self) {
        if !self.completions.is_empty() {
            self.completion_index = if self.completion_index == 0 {
                self.completions.len() - 1
            } else {
                self.completion_index - 1
            };
        }
    }

    /// Hide completions
    pub const fn hide_completions(&mut self) {
        self.show_completions = false;
    }

    /// Show completions if available
    pub const fn show_completions_if_available(&mut self) {
        self.show_completions = !self.completions.is_empty();
    }

    #[must_use]
    /// Parse the current input for command with arguments (e.g., "nf filename.txt")
    pub fn parse_command(&self) -> Option<CommandAction> {
        let input = self.input.trim();
        if input.is_empty() {
            return None;
        }

        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        let cmd = parts[0];
        match cmd {
            "nf" => {
                if parts.len() > 1 {
                    Some(CommandAction::NewFileWithName(parts[1..].join(" ")))
                } else {
                    Some(CommandAction::NewFile)
                }
            }

            "nd" => {
                if parts.len() > 1 {
                    Some(CommandAction::NewFolderWithName(parts[1..].join(" ")))
                } else {
                    Some(CommandAction::NewFolder)
                }
            }

            "reload" => Some(CommandAction::Reload),

            "grep" => Some(CommandAction::SearchContent),

            "config" => Some(CommandAction::OpenConfig),

            _ => {
                // Try to match existing commands by title
                self.all_commands
                    .iter()
                    .find(|c| c.title.to_lowercase().contains(&input.to_lowercase()))
                    .map(|c| c.action.clone())
            }
        }
    }
}

/// A user-invokable command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub title: String,
    pub action: CommandAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    OpenConfig,
    Reload,
    NewFile,
    NewFolder,
    NewFileWithName(String),
    NewFolderWithName(String),
    SearchContent,
    SearchContentWithPattern(String),
    // ...extend with more actions or plugins
    Custom(String),
}

impl Default for CommandPaletteState {
    fn default() -> Self {
        Self::new(vec![])
    }
}

#[must_use]
/// Get command descriptions for enhanced completions
pub fn get_command_description(command: &str) -> Option<&'static str> {
    match command {
        "nf" => Some("Create a new file"),
        "nd" => Some("Create a new folder"),
        "reload" => Some("Reload current directory"),
        "grep" => Some("Enter content search overlay"),
        "config" => Some("Open configuration"),
        _ => None,
    }
}

#[must_use]
/// Get all available commands with descriptions for help documentation  
pub fn get_all_commands_with_descriptions() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("nf", "", "Create a new file (nf [filename])"),
        ("nd", "", "Create a new folder (nd [foldername])"),
        ("reload", "", "Reload current directory"),
        ("grep", "", "Enter content search overlay"),
        ("config", "", "Open configuration"),
    ]
}
