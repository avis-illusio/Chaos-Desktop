//! `cdc` 集成测试：真的起进程、真的看退出码。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_cdc")
}

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cdc-it-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn file(&self, name: &str, body: &str) -> PathBuf {
        let p = self.dir.join(name);
        fs::write(&p, body).unwrap();
        p
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn cdc(&self, args: &[&str]) -> std::process::Output {
        Command::new(bin()).args(args).output().unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn code(out: &std::process::Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn run_command_creates_file() {
    let sb = Sandbox::new("run-command");
    let marker = sb.path("marker");
    let f = sb.file(
        "a.cdc",
        &format!("run command touch {}\n", marker.display()),
    );

    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(marker.exists());
}

#[test]
fn run_file_executes_script() {
    let sb = Sandbox::new("run-file");
    let marker = sb.path("marker2");
    let script = sb.file("s.sh", &format!("#!/bin/sh\ntouch {}\n", marker.display()));
    let f = sb.file("a.cdc", &format!("run file {}\n", script.display()));

    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(marker.exists());
}

#[test]
fn script_does_not_need_shebang_or_exec_bit() {
    let sb = Sandbox::new("script-mode");
    let marker = sb.path("marker3");
    let script = sb.file("plain.sh", &format!("touch {}\n", marker.display()));
    let f = sb.file("a.cdc", &format!("run file {}\n", script.display()));

    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(marker.exists());
}

#[test]
fn xrun_runs_a_real_binary() {
    let sb = Sandbox::new("xrun");
    let f = sb.file("a.cdc", "xrun /bin/true\n");
    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
}

#[test]
fn xrun_missing_file_is_a_runtime_error() {
    let sb = Sandbox::new("xrun-missing");
    let f = sb.file("a.cdc", "xrun /definitely/not/here\n");
    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("E101"), "stderr: {}", stderr(&out));
}

#[test]
fn non_zero_exit_is_reported_but_execution_continues() {
    let sb = Sandbox::new("nonzero");
    let marker = sb.path("after");
    let f = sb.file(
        "a.cdc",
        &format!(
            "run command exit 7\nrun command touch {}\n",
            marker.display()
        ),
    );

    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("E202"), "stderr: {}", stderr(&out));
    assert!(marker.exists(), "后面的命令仍应执行");
}

#[test]
fn parse_error_aborts_before_any_execution() {
    let sb = Sandbox::new("parse-error");
    let marker = sb.path("never");
    let f = sb.file(
        "a.cdc",
        &format!("run command touch {}\nlaunch kitty\n", marker.display()),
    );

    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 3, "stderr: {}", stderr(&out));
    assert!(stderr(&out).contains("E003"));
    assert!(!marker.exists(), "解析失败时不许执行任何命令");
}

#[test]
fn safe_mode_skips_process_commands() {
    let sb = Sandbox::new("safe");
    let marker = sb.path("skipped");
    let f = sb.file(
        "a.cdc",
        &format!("run command touch {}\n", marker.display()),
    );

    let out = sb.cdc(&["--safe", f.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(stderr(&out).contains("W005"));
    assert!(!marker.exists());
}

#[test]
fn validate_never_executes() {
    let sb = Sandbox::new("validate");
    let marker = sb.path("validated");
    let f = sb.file(
        "a.cdc",
        &format!("run command touch {}\n", marker.display()),
    );

    let out = sb.cdc(&["validate", f.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(!marker.exists(), "validate 不能执行命令");
}

#[test]
fn validate_checks_paths() {
    let sb = Sandbox::new("validate-path");
    let f = sb.file("a.cdc", "xrun /nope\n");
    let out = sb.cdc(&["validate", f.to_str().unwrap()]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("E101"));
}

#[test]
fn identity_blocks_and_continue_run_as_units() {
    let sb = Sandbox::new("identity");
    let a = sb.path("a");
    let b = sb.path("b");
    let f = sb.file(
        "a.cdc",
        &format!(
            "@synchronizer\n  run command touch {}\n  run command touch {}\n@synchronizer!\n",
            a.display(),
            b.display()
        ),
    );

    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(a.exists() && b.exists());
}

#[test]
fn nested_block_is_its_own_unit() {
    let sb = Sandbox::new("nested");
    let inner = sb.path("inner");
    let f = sb.file(
        "a.cdc",
        &format!(
            "run command true\n  run command touch {}\n",
            inner.display()
        ),
    );

    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(inner.exists());
}

#[test]
fn json_output_is_parseable_enough() {
    let sb = Sandbox::new("json");
    let f = sb.file("a.cdc", "xrun /nope\n");
    let out = sb.cdc(&["--json", f.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.starts_with('{'), "stdout: {stdout}");
    assert!(stdout.contains("\"code\":\"E101\""), "stdout: {stdout}");
    assert!(stdout.contains("\"failed\":1"), "stdout: {stdout}");
}

#[test]
fn missing_home_relative_path_is_reported() {
    let sb = Sandbox::new("home-relative");
    let f = sb.file("a.cdc", "xrun definitely-not-a-real-binary-xyz\n");
    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 1);
    // 裸路径被解析到 $HOME 下
    let home = std::env::var("HOME").unwrap_or_default();
    assert!(stderr(&out).contains(&home), "stderr: {}", stderr(&out));
}

#[test]
fn help_and_version() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(code(&out), 0);
    let out = Command::new(bin()).arg("--version").output().unwrap();
    assert_eq!(code(&out), 0);
}

#[test]
fn usage_error_when_no_file() {
    let out = Command::new(bin()).output().unwrap();
    assert_eq!(code(&out), 2);
}

#[allow(dead_code)]
fn assert_exists(p: &Path) {
    assert!(p.exists(), "{} 不存在", p.display());
}

#[test]
fn scope_out_of_range_is_a_runtime_error() {
    let sb = Sandbox::new("scope-range");
    let f = sb.file("a.cdc", "run command true\n@synchronizer{99}\n");
    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("E011"), "stderr: {}", stderr(&out));
}

#[test]
fn filter_that_matches_nothing_warns_only() {
    let sb = Sandbox::new("scope-nomatch");
    let f = sb.file(
        "a.cdc",
        "@synchronizer{:}{no-such-command}\nrun command true\n",
    );
    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(stderr(&out).contains("W006"), "stderr: {}", stderr(&out));
}

#[test]
fn scoped_filter_only_covers_matching_lines() {
    let sb = Sandbox::new("scope-filter");
    let a = sb.path("scoped");
    let b = sb.path("unscoped");
    let f = sb.file(
        "a.cdc",
        &format!(
            "@synchronizer{{:}}{{scoped}}\nrun command touch {}\nrun command touch {}\n",
            a.display(),
            b.display()
        ),
    );
    let out = sb.cdc(&[f.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(a.exists() && b.exists(), "两条命令都要执行");
}
