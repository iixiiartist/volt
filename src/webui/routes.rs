#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Page {
    #[default]
    Dashboard,
    Chat,
    Tools,
    Sessions,
    Settings,
    Workflows,
    Worktrees,
    Jobs,
    Routines,
    Skills,
    Registry,
    Audit,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Page::Dashboard => "Dashboard",
            Page::Chat => "Chat",
            Page::Tools => "Tools",
            Page::Sessions => "Sessions",
            Page::Settings => "Settings",
            Page::Workflows => "Workflows",
            Page::Worktrees => "Worktrees",
            Page::Jobs => "Jobs",
            Page::Routines => "Routines",
            Page::Skills => "Skills",
            Page::Registry => "MCP Registry",
            Page::Audit => "Audit Log",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Page::Dashboard => "\u{1F3E0}",
            Page::Chat => "\u{1F4AC}",
            Page::Tools => "\u{1F527}",
            Page::Sessions => "\u{1F4C1}",
            Page::Settings => "\u{2699}\u{FE0F}",
            Page::Workflows => "\u{1F504}",
            Page::Worktrees => "\u{1F33F}",
            Page::Jobs => "\u{23F0}",
            Page::Routines => "\u{1F4A1}",
            Page::Skills => "\u{2728}",
            Page::Registry => "\u{1F4E6}",
            Page::Audit => "\u{1F50D}",
        }
    }
}
