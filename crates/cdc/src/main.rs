//! `cdc` —— Chaos Desktop Command 可执行文件。
//!
//! 用法：
//! ```text
//! cdc <file.cdc>              解析并执行
//! cdc validate <file.cdc>     只解析与校验路径，不执行
//! cdc --safe <file.cdc>       安全模式：跳过所有创建进程的命令
//!
//! 选项：--json  机器可读输出    --verbose  打印每条被执行的命令
//! ```
//!
//! 退出码：0 成功；1 有运行期错误；2 用法错误；3 解析失败（未执行任何命令）。

mod exec;

use std::process::ExitCode;

use cdc_parser::{Diagnostic, Severity};

const EXIT_OK: u8 = 0;
const EXIT_RUNTIME: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_PARSE: u8 = 3;

#[derive(Debug, Default)]
struct Args {
    path: String,
    validate: bool,
    safe: bool,
    json: bool,
    verbose: bool,
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return ExitCode::from(EXIT_OK);
    }
    if argv.iter().any(|a| a == "-V" || a == "--version") {
        println!("cdc {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::from(EXIT_OK);
    }

    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("cdc: {msg}");
            eprintln!("试试 `cdc --help`。");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let source = match std::fs::read_to_string(&args.path) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("cdc: 读不了 {}：{err}", args.path);
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let parsed = cdc_parser::parse(&source);

    // 解析阶段失败：整文件拒绝执行。
    if parsed.has_errors() {
        if args.json {
            print_json(&args.path, &parsed.diagnostics, 0, 0);
        } else {
            print_diagnostics(&args.path, &parsed.diagnostics, args.verbose);
            eprintln!("cdc: 解析失败，未执行任何命令。");
        }
        return ExitCode::from(EXIT_PARSE);
    }

    if args.validate {
        let mut diags = parsed.diagnostics.clone();
        diags.extend(exec::validate(&parsed.program));
        let bad = diags.iter().any(|d| d.severity == Severity::Error);
        if args.json {
            print_json(&args.path, &diags, 0, 0);
        } else {
            print_diagnostics(&args.path, &diags, args.verbose);
            if !bad {
                println!("{}: 校验通过", args.path);
            }
        }
        return ExitCode::from(if bad { EXIT_RUNTIME } else { EXIT_OK });
    }

    let outcome = exec::Executor::new(exec::Options {
        safe: args.safe,
        verbose: args.verbose,
    })
    .run(&parsed.program);

    let mut diags = parsed.diagnostics;
    diags.extend(outcome.diagnostics);

    if args.json {
        print_json(&args.path, &diags, outcome.executed, outcome.failed);
    } else {
        print_diagnostics(&args.path, &diags, args.verbose);
        if args.verbose {
            eprintln!(
                "cdc: 执行 {} 条命令，失败 {} 条",
                outcome.executed, outcome.failed
            );
        }
    }

    let bad = diags.iter().any(|d| d.severity == Severity::Error) || outcome.failed > 0;
    ExitCode::from(if bad { EXIT_RUNTIME } else { EXIT_OK })
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut file = None;

    for a in argv {
        match a.as_str() {
            "validate" => args.validate = true,
            "--safe" => args.safe = true,
            "--json" => args.json = true,
            "--verbose" | "-v" => args.verbose = true,
            s if s.starts_with('-') => return Err(format!("未知选项 `{s}`")),
            s => {
                if file.replace(s.to_string()).is_some() {
                    return Err("一次只能处理一个文件".into());
                }
            }
        }
    }

    args.path = file.ok_or("缺少 .cdc 文件路径")?;
    Ok(args)
}

fn print_help() {
    println!(
        "cdc — Chaos Desktop Command\n\
         \n\
         用法:\n  \
         cdc [选项] <file.cdc>\n  \
         cdc validate [选项] <file.cdc>\n\
         \n\
         选项:\n  \
         --safe      安全模式，跳过 xrun / run file / run command\n  \
         --json      以 JSON 输出诊断\n  \
         --verbose   打印每条被执行的命令\n  \
         -h, --help  显示帮助\n  \
         -V, --version  显示版本\n\
         \n\
         退出码: 0 成功 · 1 运行期错误 · 2 用法错误 · 3 解析失败"
    );
}

fn print_diagnostics(file: &str, diags: &[Diagnostic], _verbose: bool) {
    for d in diags {
        eprintln!(
            "{file}:{}:{} {}[{}] {}",
            d.span.line,
            d.span.col,
            d.severity.as_str(),
            d.code,
            d.message
        );
    }
}

fn print_json(file: &str, diags: &[Diagnostic], executed: usize, failed: usize) {
    let items: Vec<String> = diags
        .iter()
        .map(|d| {
            format!(
                "{{\"code\":\"{}\",\"severity\":\"{}\",\"line\":{},\"col\":{},\"message\":\"{}\"}}",
                d.code,
                d.severity.as_str(),
                d.span.line,
                d.span.col,
                json_escape(&d.message)
            )
        })
        .collect();
    println!(
        "{{\"file\":\"{}\",\"diagnostics\":[{}],\"executed\":{},\"failed\":{}}}",
        json_escape(file),
        items.join(","),
        executed,
        failed
    );
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
