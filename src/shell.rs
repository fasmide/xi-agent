#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    #[cfg(windows)]
    Cmd,
    #[cfg(windows)]
    PowerShell,
}

impl ShellKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            #[cfg(windows)]
            Self::Cmd => "cmd",
            #[cfg(windows)]
            Self::PowerShell => "PS",
        }
    }

    pub fn prompt_char(self) -> char {
        match self {
            Self::Bash => '$',
            #[cfg(windows)]
            Self::Cmd | Self::PowerShell => '>',
        }
    }
}

pub fn discover_available_shells() -> Vec<ShellKind> {
    #[cfg(windows)]
    {
        vec![ShellKind::PowerShell, ShellKind::Cmd]
    }

    #[cfg(not(windows))]
    {
        vec![ShellKind::Bash]
    }
}
