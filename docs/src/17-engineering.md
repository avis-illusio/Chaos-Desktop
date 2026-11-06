# 工程规范

## 环境

| 项 | 状态 |
|---|---|
| Rust | 1.99.0，edition 2024 |
| 系统 | Arch 系（当前发行版标识 `Chaos Linux`） |
| 已就位 | wayland、libinput、libdrm、mesa、libxkbcommon、xwayland-satellite、xorg-xwayland |
| **缺** | **libseat / seatd**（阶段 1 前必装）、EGL/GBM 头文件 |
| crates 源 | `rsproxy` 镜像（全局 `~/.cargo/config.toml`） |

onedoc 私有安装，不污染 `~/.cargo`：

```sh
CARGO_HOME="$PWD/target/cargo-home" \
  cargo install cargo-onedoc --version 0.3.2 --locked --root "$PWD/target/onedoc-tool"
./target/onedoc-tool/bin/cargo-onedoc onedoc
```

## 文档规范

- **`docs/design.md` 是生成物**：只改 `docs/src/*.md`，再跑 `cargo onedoc`；生成物带 `DO NOT EDIT` 头。
- 文档按「是什么 / 契约 / 决策 / 计划 / 缺口 / 工程」分层，一件事只写在一个文件里。
- `README.md` 手写，只讲定位与特性；技术内容、约束、命令、排期一律进 design，不要往 README 里搬。
- 新增配置项、诊断码、决策：先改文档，再写代码，同一 PR。
- 交接文档 `docs/handover-v0.1.md` 是归档，不参与生成，也不追改。

## 测试分层

| 层 | 手段 | 门禁 |
|---|---|---|
| `chaos-path` | 表格驱动 + 边界（`..` 越出 home、`~user`、尾斜杠） | 文档每个等价示例一条测试 |
| `cdc-parser` | fixture + `insta` 快照（AST + 诊断） | 每条规则至少一正一反 |
| `chaos-config` | 三层合并矩阵 + 写回幂等 + 原子写 | 来源断言精确到行 |
| `chaos-layout` | `proptest`：不重叠 / 满铺减 gap / 顺序稳定 | 随机 1000 例无 panic |
| 合成器 | `Xvfb` + winit 后端 smoke：起 → 映射窗口 → 退出 | 无 panic、退出码 0 |
| 视觉 | headless 渲染 + 基准图 diff（阶段 5 起） | 差异面积阈值 |

## CI 门禁

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
./target/onedoc-tool/bin/cargo-onedoc onedoc --check
```

只跑 Linux。`cargo deny` 检查许可证与安全公告。

## 红线

- [ ] 不软链系统库（`libwayland-server`、`libinput`、Qt 库）
- [ ] 不引入非 Rust 运行时（QML / Python / Lua / Node）
- [ ] `.cdc` 无 shell 语法入口；`.sh` 只能经 `xfile`
- [ ] IPC socket 0600 + uid 校验
- [ ] 任何 `apply` 前有快照
- [ ] 子进程全部 `setsid`，无孤儿
- [ ] 合成器 crate 之外无 `unsafe`

## 提交

- Conventional Commits，一个子阶段一个 PR，`main` 始终可构建。
- 不主动 commit / push，除非明确要求。
- 破坏性依赖升级单独成 PR，并在 `docs/src/05-decisions.md` 记一笔。

## 测试形态（按危险度递增）

1. **`--nested`（默认）**：合成器跑在 KDE 窗口里，崩溃只死窗口。日常开发 100% 走这条。
2. **`--headless`（CI）**：无显示服务器，跑协议与渲染快照。
3. **TTY / DRM（里程碑）**：唯一能测到 libinput、KMS、VT 切换的路径，也是最容易把机器锁死的一条。

## TTY 测试守则

1. 进 TTY 前确认 SSH 可用，或已开第二个 TTY（`Ctrl+Alt+F2`）。
2. 用**看门狗启动**，不要直接跑裸二进制：

   ```sh
   timeout 60 ./target/debug/chaos-compositor || chvt 1
   ```

   合成器内部也实现同一套：60s 无输入 / 无健康心跳 → 自行退出并 `chvt` 回原 VT。
3. 准备救回脚本：`chvt 1`、`pkill -f chaos-compositor`。
4. 首次只跑只读模式（不接管 DRM），确认日志正常再逐步放开。
5. 合成器启动前把当前 VT 号记下来，恢复时切回去，而不是假定是 1。

## 预览与截图

- 内置 PZ 全景由合成器自己渲染，不需要协议。
- 跨进程预览（把桌面显示在另一个窗口里，类似 Noctalia 的桌面窗口）需要 `ext-image-copy-capture`；它是**观察工具**，不提供崩溃隔离，排在阶段 6 之后。
