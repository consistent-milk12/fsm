// src/model/command_palette.rs

#[derive(Debug, Clone)]
pub struct CommandPaletteState {
    pub visible: bool,
    pub input: String,
    pub filtered: Vec<Command>,
    pub all_commands: Vec<Command>,
    pub selected: usize,
}

impl CommandPaletteState {
    pub fn new(commands: Vec<Command>) -> Self {
        Self {
            visible: false,
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
    // ...extend with more actions or plugins
    Custom(String),
}

impl Default for CommandPaletteState {
    fn default() -> Self {
        Self::new(vec![])
    }
}