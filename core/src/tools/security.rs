use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const FORBIDDEN_PATHS: &[&str] = &[
    "/etc",
    "/root",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/opt",
    "/boot",
    "/dev",
    "/proc",
    "/sys",
];

const DANGEROUS_COMMANDS: &[&str] = &[
    "rm",
    "mkfs",
    "dd",
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "sudo",
    "su",
    "chown",
    "chmod",
    "useradd",
    "userdel",
    "usermod",
    "passwd",
    "mount",
    "umount",
    "iptables",
    "ufw",
    "firewall-cmd",
    "fdisk",
    "parted",
    "wipefs",
    "shred",
    "nc",
    "ncat",
    "netcat",
];

const NETWORK_COMMANDS: &[&str] = &[
    "curl",
    "wget",
    "scp",
    "ssh",
    "ftp",
    "telnet",
];

const ALLOWED_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LANG",
    "TERM",
    "PWD",
    "SHELL",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRisk {
    Low,
    Medium,
    High,
}

pub struct RateLimiter {
    window_start: AtomicU64,
    count: AtomicU64,
    max_actions: u64,
    window_secs: u64,
}

impl RateLimiter {
    pub fn new(max_actions: u64, window_secs: u64) -> Self {
        Self {
            window_start: AtomicU64::new(0),
            count: AtomicU64::new(0),
            max_actions,
            window_secs,
        }
    }

    pub fn check_and_record(&self) -> bool {
        let now = current_timestamp();
        let start = self.window_start.load(Ordering::Relaxed);

        if now < start || now - start >= self.window_secs {
            self.window_start.store(now, Ordering::Relaxed);
            self.count.store(1, Ordering::Relaxed);
            return true;
        }

        let count = self.count.fetch_add(1, Ordering::Relaxed);
        count < self.max_actions
    }

    pub fn is_limited(&self) -> bool {
        let now = current_timestamp();
        let start = self.window_start.load(Ordering::Relaxed);

        if now < start || now - start >= self.window_secs {
            return false;
        }

        self.count.load(Ordering::Relaxed) >= self.max_actions
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(60, 3600)
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn classify_command_risk(command: &str) -> CommandRisk {
    let trimmed = command.trim();
    let lower = trimmed.to_lowercase();

    if lower.contains("rm -rf /")
        || lower.contains("rm -fr /")
        || lower.contains(":(){:|:&};:")
        || lower.contains("mkfs")
        || lower.contains("dd if=")
        || lower.contains("dd of=")
    {
        return CommandRisk::High;
    }

    let first_word = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .split('/')
        .next_back()
        .unwrap_or("");

    if DANGEROUS_COMMANDS.contains(&first_word) {
        return CommandRisk::High;
    }

    if NETWORK_COMMANDS.contains(&first_word) {
        return CommandRisk::Medium;
    }

    if lower.contains("| sh")
        || lower.contains("| bash")
        || lower.contains("$(curl")
        || lower.contains("$(wget")
        || lower.contains("> /dev/")
        || lower.contains("&& rm ")
    {
        return CommandRisk::High;
    }

    CommandRisk::Low
}

pub fn validate_command(command: &str, rate_limiter: &RateLimiter) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err("Empty command".to_string());
    }

    if rate_limiter.is_limited() {
        return Err("Rate limit exceeded: too many shell commands. Please wait a moment.".to_string());
    }

    let risk = classify_command_risk(command);
    match risk {
        CommandRisk::High => {
            Err(format!(
                "High-risk command blocked for safety. Command: {}",
                command.split_whitespace().next().unwrap_or(command)
            ))
        }
        CommandRisk::Medium => {
            Err("Network commands are blocked for security. Use http_request tool instead.".to_string())
        }
        CommandRisk::Low => Ok(()),
    }
}

pub fn sanitize_env_vars(env: &[(String, String)]) -> Vec<(String, String)> {
    env.iter()
        .filter(|(key, _)| ALLOWED_ENV_VARS.contains(&key.as_str()))
        .cloned()
        .collect()
}

pub fn is_path_allowed(path: &str) -> bool {
    if path.contains('\0') {
        return false;
    }

    if Path::new(path)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return false;
    }

    let lower = path.to_lowercase();
    if lower.contains("..%2f")
        || lower.contains("..%5c")
        || lower.contains("%2e%2e")
        || lower.contains("%252e")
    {
        return false;
    }

    let expanded = expand_home(path);
    for forbidden in FORBIDDEN_PATHS {
        if expanded.starts_with(forbidden) {
            return false;
        }
    }

    true
}

pub fn validate_workspace_path(path: &str, workspace: &Path) -> Result<PathBuf, String> {
    if !is_path_allowed(path) {
        return Err(format!("Path contains forbidden patterns: {}", path));
    }

    let full_path = workspace.join(path);

    let canonical_workspace = workspace
        .canonicalize()
        .map_err(|e| format!("Cannot canonicalize workspace: {}", e))?;

    let canonical_full = match full_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let parent = full_path.parent().unwrap_or(Path::new("."));
            let canonical_parent = parent
                .canonicalize()
                .map_err(|e| format!("Cannot canonicalize parent: {}", e))?;
            canonical_parent.join(full_path.file_name().unwrap_or_default())
        }
    };

    if !canonical_full.starts_with(&canonical_workspace) {
        return Err(format!(
            "Path escapes workspace: {} is not under {}",
            canonical_full.display(),
            canonical_workspace.display()
        ));
    }

    Ok(canonical_full)
}

fn expand_home(path: &str) -> String {
    if (path == "~" || path.starts_with("~/"))
        && let Some(home) = std::env::var_os("HOME")
    {
        return path.replacen('~', &home.to_string_lossy(), 1);
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_byte_blocked() {
        assert!(!is_path_allowed("file\0.txt"));
    }

    #[test]
    fn test_parent_dir_blocked() {
        assert!(!is_path_allowed("../etc/passwd"));
        assert!(!is_path_allowed("foo/../../etc/passwd"));
    }

    #[test]
    fn test_url_encoded_traversal_blocked() {
        assert!(!is_path_allowed("..%2fetc%2fpasswd"));
        assert!(!is_path_allowed("..%5cetc%5cpasswd"));
    }

    #[test]
    fn test_forbidden_paths_blocked() {
        assert!(!is_path_allowed("/etc/passwd"));
        assert!(!is_path_allowed("/root/.ssh/id_rsa"));
    }

    #[test]
    fn test_valid_path_allowed() {
        assert!(is_path_allowed("src/main.rs"));
        assert!(is_path_allowed("foo/bar.txt"));
    }

    #[test]
    fn test_workspace_escape_blocked() {
        let workspace = Path::new("/tmp/workspace");
        let result = validate_workspace_path("../etc/passwd", workspace);
        assert!(result.is_err());
    }

    #[test]
    fn test_high_risk_commands() {
        assert_eq!(classify_command_risk("rm -rf /"), CommandRisk::High);
        assert_eq!(classify_command_risk("sudo su"), CommandRisk::High);
        assert_eq!(classify_command_risk("dd if=/dev/zero of=/dev/sda"), CommandRisk::High);
        assert_eq!(classify_command_risk("mkfs.ext4 /dev/sda1"), CommandRisk::High);
    }

    #[test]
    fn test_medium_risk_commands() {
        assert_eq!(classify_command_risk("curl https://example.com"), CommandRisk::Medium);
        assert_eq!(classify_command_risk("wget http://test.com/file"), CommandRisk::Medium);
    }

    #[test]
    fn test_low_risk_commands() {
        assert_eq!(classify_command_risk("ls -la"), CommandRisk::Low);
        assert_eq!(classify_command_risk("git status"), CommandRisk::Low);
        assert_eq!(classify_command_risk("cat file.txt"), CommandRisk::Low);
    }

    #[test]
    fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new(5, 60);
        for _ in 0..5 {
            assert!(limiter.check_and_record());
        }
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new(2, 60);
        assert!(limiter.check_and_record());
        assert!(limiter.check_and_record());
        assert!(!limiter.check_and_record());
    }

    #[test]
    fn test_sanitize_env_vars() {
        let env = vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("SECRET_KEY".to_string(), "supersecret".to_string()),
            ("HOME".to_string(), "/home/user".to_string()),
        ];
        let sanitized = sanitize_env_vars(&env);
        assert_eq!(sanitized.len(), 2);
        assert!(sanitized.iter().any(|(k, _)| k == "PATH"));
        assert!(sanitized.iter().any(|(k, _)| k == "HOME"));
        assert!(!sanitized.iter().any(|(k, _)| k == "SECRET_KEY"));
    }

    #[test]
    fn test_validate_command_blocks_high_risk() {
        let limiter = RateLimiter::new(100, 3600);
        assert!(validate_command("rm -rf /", &limiter).is_err());
        assert!(validate_command("sudo rm /etc/passwd", &limiter).is_err());
    }

    #[test]
    fn test_validate_command_allows_low_risk() {
        let limiter = RateLimiter::new(100, 3600);
        assert!(validate_command("ls -la", &limiter).is_ok());
        assert!(validate_command("git status", &limiter).is_ok());
    }
}
