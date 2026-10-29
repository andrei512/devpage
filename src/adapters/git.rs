//! Git, asked for one thing: the name of whoever runs devpage, which is the
//! local comment author.

use std::path::Path;
use std::process::Command;

/// `git config user.name` as git sees it from `folder` — its global setting
/// counts, so no `.git` folder is needed — or the system's user name when
/// git has none.
pub fn user_name(folder: &Path) -> String {
    Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(["config", "user.name"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("USERNAME").ok())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_name_needs_no_git_folder() {
        let folder = std::env::temp_dir().join(format!("devpage-author-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        assert!(!folder.join(".git").exists());
        let expected = Command::new("git")
            .args(["config", "--global", "user.name"])
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
            .filter(|name| !name.is_empty())
            .or_else(|| std::env::var("USER").ok());
        assert_eq!(Some(user_name(&folder)), expected);
    }
}
