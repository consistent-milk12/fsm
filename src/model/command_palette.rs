// src/model/command_palette.rs

#[derive(Debug, Clone)]
pub struct CommandPaletteState {
    pub input: String,
    pub filtered: Vec<Command>,
    pub all_commands: Vec<Command>,
    pub selected: usize,
}

impl CommandPaletteState {
    pub fn new(commands: Vec<Command>) -> Self {
        Self {
            input: String::new(),
            filtered: commands.clone(),
            all_commands: commands,
            selected: 0,
        }
    }

    /// Call this on input change to filter commands.
    pub fn update_filter(&mut self) {
        let q = self.input.to_lowercase();
        self.filtered = self
            .all_commands
            .iter()
            .cloned()
            .filter(|cmd| cmd.title.to_lowercase().contains(&q))
            .collect();
        self.selected = 0;
    }

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
            "nf" | "newfile" => {
                if parts.len() > 1 {
                    Some(CommandAction::NewFileWithName(parts[1..].join(" ")))
                } else {
                    Some(CommandAction::NewFile)
                }
            }

            "nd" | "newfolder" | "mkdir" => {
                if parts.len() > 1 {
                    Some(CommandAction::NewFolderWithName(parts[1..].join(" ")))
                } else {
                    Some(CommandAction::NewFolder)
                }
            }

            "reload" | "r" => Some(CommandAction::Reload),

            "search" | "grep" => {
                if parts.len() > 1 {
                    Some(CommandAction::SearchContentWithPattern(
                        parts[1..].join(" "),
                    ))
                } else {
                    Some(CommandAction::SearchContent)
                }
            }

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
#[derive(Debug, Clone)]
pub struct Command {
    pub title: String,
    pub action: CommandAction,
}

#[derive(Debug, Clone)]
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
