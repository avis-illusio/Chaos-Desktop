//! 路径解析：只认四个特殊内容 —— `/`、`..`、`$HOME`、`~`。
//!
//! 规则（顺序即优先级）：
//! 1. 以 `/` 开头 → 原样（绝对路径）。
//! 2. `~` 或 `~/...` → 替换为 home。
//! 3. `$HOME` 或 `$HOME/...` → 替换为 home；其它 `$VAR` 不展开。
//! 4. 其余（含裸路径、`./`、`../`、`~user/`）→ 拼到 home 后面。
//! 5. 在字符串层规范化 `.` / `..`，不解析符号链接、不读 cwd。
//!
//! 规范化可能越出 home（`~/../../etc` → `/etc`），这是允许的；
//! 存在性与「是否目录」由调用方（validate）检查，不在本模块做 IO。

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// 路径解析失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// 输入为空或只有空白。
    Empty,
    /// 环境里没有 `$HOME`。
    NoHome,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("路径为空"),
            Self::NoHome => f.write_str("无法确定 $HOME"),
        }
    }
}

impl std::error::Error for PathError {}

/// 文件路径：保证没有末尾 `/`。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FilePath(PathBuf);

impl FilePath {
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.to_string_lossy())
    }
}

/// 目录路径：保证有末尾 `/`（根目录 `/` 也算有）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DirPath(PathBuf);

impl DirPath {
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl fmt::Display for DirPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.0.to_string_lossy();
        if s.ends_with('/') {
            f.write_str(&s)
        } else {
            write!(f, "{s}/")
        }
    }
}

/// 用 `$HOME` 解析文件路径。
pub fn resolve_file(input: &str) -> Result<FilePath, PathError> {
    let home = home_dir()?;
    resolve_file_with_home(input, &home)
}

/// 用 `$HOME` 解析目录路径。
pub fn resolve_dir(input: &str) -> Result<DirPath, PathError> {
    let home = home_dir()?;
    resolve_dir_with_home(input, &home)
}

/// 可注入 home 的文件路径解析（测试与离线校验用）。
pub fn resolve_file_with_home(input: &str, home: &Path) -> Result<FilePath, PathError> {
    Ok(FilePath(normalize(&expand(input, home)?)))
}

/// 可注入 home 的目录路径解析（测试与离线校验用）。
pub fn resolve_dir_with_home(input: &str, home: &Path) -> Result<DirPath, PathError> {
    Ok(DirPath(normalize(&expand(input, home)?)))
}

/// 取 `$HOME`。空值视为未设置。
pub fn home_dir() -> Result<PathBuf, PathError> {
    match std::env::var_os("HOME") {
        Some(h) if !h.is_empty() => Ok(PathBuf::from(h)),
        _ => Err(PathError::NoHome),
    }
}

fn expand(input: &str, home: &Path) -> Result<PathBuf, PathError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(PathError::Empty);
    }

    let p = if s.starts_with('/') {
        PathBuf::from(s)
    } else if s == "~" {
        home.to_path_buf()
    } else if let Some(rest) = s.strip_prefix("~/") {
        home.join(rest)
    } else if s == "$HOME" {
        home.to_path_buf()
    } else if let Some(rest) = s.strip_prefix("$HOME/") {
        home.join(rest)
    } else {
        // 含 `~user/`、`./`、`../`、裸路径：一律相对 home。
        home.join(s)
    };

    Ok(p)
}

/// 字符串层规范化：去掉 `.`，消解 `..`，保留根。
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                // 已经在根上时 `..` 无效；否则弹掉上一段。
                if !out.pop() {
                    out.push("/");
                }
            }
            Component::Normal(seg) => out.push(seg),
            Component::Prefix(_) => {}
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/tester";

    fn file(s: &str) -> String {
        resolve_file_with_home(s, Path::new(HOME))
            .unwrap()
            .to_string()
    }

    fn dir(s: &str) -> String {
        resolve_dir_with_home(s, Path::new(HOME))
            .unwrap()
            .to_string()
    }

    #[test]
    fn four_forms_are_equivalent() {
        let want = "/home/tester/Desktop";
        assert_eq!(file("~/Desktop"), want);
        assert_eq!(file("$HOME/Desktop"), want);
        assert_eq!(file("Desktop"), want);
        assert_eq!(file("/home/tester/Desktop"), want);
    }

    #[test]
    fn bare_tilde_is_home() {
        assert_eq!(file("~"), "/home/tester");
        assert_eq!(dir("~"), "/home/tester/");
        assert_eq!(file("~/"), "/home/tester");
    }

    #[test]
    fn other_vars_are_not_expanded() {
        assert_eq!(
            file("$XDG_CONFIG_HOME/x"),
            "/home/tester/$XDG_CONFIG_HOME/x"
        );
    }

    #[test]
    fn tilde_user_is_a_relative_segment() {
        assert_eq!(file("~other/x"), "/home/tester/~other/x");
        assert_eq!(dir("~other/"), "/home/tester/~other/");
    }

    #[test]
    fn dot_and_dotdot_are_normalized() {
        assert_eq!(file("a/./b"), "/home/tester/a/b");
        assert_eq!(file("a/../b"), "/home/tester/b");
        assert_eq!(file("a/b/../../c"), "/home/tester/c");
    }

    #[test]
    fn can_escape_home() {
        assert_eq!(file("~/../../etc"), "/etc");
        assert_eq!(file("~/../.."), "/");
    }

    #[test]
    fn dotdot_at_root_stays_at_root() {
        assert_eq!(file("/../../etc"), "/etc");
        assert_eq!(file("/.."), "/");
    }

    #[test]
    fn dir_has_trailing_slash_file_does_not() {
        assert_eq!(dir("a/b"), "/home/tester/a/b/");
        assert_eq!(file("a/b/"), "/home/tester/a/b");
        assert_eq!(dir("/"), "/");
    }

    #[test]
    fn empty_is_an_error() {
        assert_eq!(
            resolve_file_with_home("   ", Path::new(HOME)),
            Err(PathError::Empty)
        );
    }

    #[test]
    fn repeated_slashes_collapse() {
        assert_eq!(file("a//b///c"), "/home/tester/a/b/c");
        assert_eq!(file("//etc//x"), "/etc/x");
    }
}
