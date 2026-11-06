# 快速开始

`cdc` 已经可用，合成器还没开始。以下是仓库自身的构建与文档命令。

```sh
cargo build --workspace              # 构建
cargo test  --workspace              # 测试
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## 文档

技术文档由 `cargo-onedoc` 从 `docs/src/` 合成到 `docs/design.md`；`README.md` 是手写的，不经生成。

```sh
cargo install cargo-onedoc --version 0.3.2 --locked   # 装到 ~/.cargo/bin

cargo onedoc           # 重新生成
cargo onedoc --check   # CI：生成物是否过期
```

若 `~/.cargo` 只读，装到仓库内（`target/` 已忽略）：`CARGO_HOME="$PWD/target/cargo-home" cargo install cargo-onedoc --version 0.3.2 --locked --root "$PWD/target/onedoc-tool"`，然后跑 `./target/onedoc-tool/bin/cargo-onedoc onedoc`。

## 依赖

- 合成器：`smithay = "=0.7.0"`（精确 pin）
- Shell UI：`slint = "1.18"` + `renderer-software`
- Wayland 客户端：`smithay-client-toolkit = "0.21"`
- 事件循环：`calloop = "0.14"`
- 配置读写：`toml_edit = "0.25"`

工具链固定 **rustc 1.99.0 / edition 2024**，只支持 Linux + Wayland，不建跨平台 CI 矩阵。
