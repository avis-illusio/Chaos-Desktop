# 实施计划

估算口径：单人、业余每周 15 小时。给区间，不给假精度。

当前进度：阶段 0 的 `chaos-path`、`cdc-parser`、`cdc` 三个 crate 已落地并有测试；解析器与执行器按 `12-cdc` 的语法工作。

## 阶段

| 阶段 | 内容 | 出口标准 | 周 |
|---|---|---|---|
| 0 地基 | `chaos-path` / `cdc-parser` / `chaos-config` / `chaos-ipc` / `cdc` CLI / 设置应用最小版 | 在 KDE 里可用 `cdc validate` 与设置应用；零图形依赖；全链路有测试 | 4–5 |
| 1 合成器 | `--nested`（winit，默认开发形态）+ `--headless`（CI）+ udev/DRM、libinput、xdg-shell、activation | 嵌套窗口里跑 kitty 能打字能退出；`--headless` 能出帧；TTY 带看门狗验证 | 5–6 |
| 2 白板 | 装箱算法、自由移动/缩放、pending、configure 回路、layer-shell + popup | 10 窗口不闪烁；放不下进 pending；waybar 可挂 | 4 |
| 3 配置接入 | 在线配置服务、热重载、快照回退、systemd 托管 | 坏配置连续失败两次 → 自动进恢复模式 | 2–3 |
| 4 in-shell | `chaos-settings --in-shell` 子进程 | `kill -9` 面板不影响合成器；禁用后面板功能对等 | 2 |
| 5 PZ | 常规/鸟瞰两态、三指平移、小地图、动画、边界（见 D12） | 落点像素级对齐；鸟瞰帧时间 < 8ms | 4–5 |
| 6 Shell | Dock / 启动器 / 落点指示器 / PCManFM-Qt 兼容 | Dock 常驻静态 CPU < 3%；启动窗口落到预期列 | 4–6 |
| 7 打包 | PKGBUILD（release + `-git`）、systemd unit、desktop 文件 | 干净 chroot 里 `makepkg -si` 后能从 TTY 起会话 | 1–2 |
| 8 叠层 | 叠层模式、动画美化 | 按日常使用反馈排优先级 | — |

## 里程碑

| 里程碑 | 交付 | 累计周 |
|---|---|---|
| M0 | 地基完成，KDE 内可用 | 4–5 |
| M1 | 合成器能跑 kitty | 9–11 |
| M2 | 平铺 + layer-shell | 13–15 |
| M3 | 配置热接 + 崩溃回退 | 15–18 |
| M4 | in-shell 面板 | 17–20 |
| M5 | PZ 两档 + 动画 | 21–25 |
| M6 | Dock / 启动器 / 落点 | 25–31 |
| M7 | v0.1 打包发布 | 26–33 |

与交接文档「前六个月不设为日常桌面」一致：M6 约在第 6 个月。在此之前一律在 KDE 里开发，TTY 只做验证。

## 阶段 0 的拆解（唯一可立即开工的部分）

| 子阶段 | 内容 | 验收 |
|---|---|---|
| 0.1 | workspace 拆分、toolchain 固定、lints、CI、首个提交 | `cargo test --workspace` + `cargo onedoc --check` 全绿 |
| 0.2 | `chaos-path`：newtype、规范化、五条路径规则 | 交接文档 §4.3 每个等价示例是一条测试 |
| 0.3 | `cdc-parser`：词法/AST → 缩进与身份栈 → 索引预扫描 | 每条规则至少一正一反 fixture |
| 0.4 | `chaos-config`：读取扁平化 → 三层合并 → 写回与快照 | 改一个键 diff 只有一行；来源精确到 `file:line:col` |
| 0.5 | `chaos-ipc` + `cdc` CLI | 无合成器时退出码 2 并停止；`validate` 完全离线 |
| 0.6 | 设置应用最小版 | KDE 内可改值、保存、看到来源 |

## 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| Smithay API 中途破坏性变更 | 中 | 高 | 精确 pin；升版只在阶段边界 |
| slint 承载不了 layer-shell | 中 | 中 | S1 提前到阶段 0 之后；退路已定（见 D8） |
| 坏配置导致启动循环 | 中 | 高 | 阶段 1 起就做「校验失败 → 内置默认」兜底；阶段 3 补恢复模式 |
| TTY 测试锁死机器 | 中 | 高 | 进 TTY 前开 SSH / 第二个 TTY，备好 `chvt` 救回脚本 |
| PZ 视觉 bug 无限打磨 | 高 | 中 | 静态两档 + 网格 overlay 先验收；量化指标代替目测 |
| 多输出模型定错导致 PZ 返工 | 中 | 高 | G1 在阶段 2 前落定 |
| 范围膨胀（叠层/动画/主题） | 高 | 中 | 阶段 8 之前一律进 backlog |
| 单人长期项目失去节奏 | 高 | 高 | 每阶段一个可演示 demo；里程碑打 tag；每周全量测试 |

## 开工顺序

1. ADR-0001（纯 Rust 运行时判定标准）、ADR-0002（TOML 选型）。
2. 阶段 0.1 + spike S4（2 小时）+ spike S1（1–2 天）。
3. 0.2 → 0.3 → 0.4 → 0.5 → 0.6，一个子阶段一个 PR。
