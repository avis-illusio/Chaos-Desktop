# 架构

进程边界与职责划分。语义细节以交接文档为准，这里只记实现契约。

## 进程与分层

```text
合成器进程（chaos-compositor, Smithay）
├─ 配置服务：加载 / 校验 / 快照 / 热重载 / IPC 端点
├─ 布局引擎：白板装箱 + 自由移动（LayoutEngine trait）
├─ cdc-parser（进程内）：解析 → 执行计划
├─ IPC 服务：独立线程，阻塞 IO + calloop channel 回主循环
└─ in-shell 面板：独立子进程（崩溃隔离），默认启用、可禁用

cdc（bin）        ── 链接 cdc-parser + chaos-ipc，逐条把命令送回合成器
chaos-settings    ── 在线走 IPC，离线直读写配置文件（同一 UI 代码，双宿主）
chaos-shell       ── Wayland 客户端，不链接合成器核心
```

## 关键原则

1. **配置逻辑住在合成器核心的配置服务**，不在任何 UI 里。UI 只做「读、改、提交」。
2. **双前端功能对等**：in-shell 与独立应用是同一份代码的两种宿主，不是两套实现。
3. **Shell UI 不是合成器的一部分**：独立进程、独立崩溃域、走 Wayland 协议通信。
4. **单一解析真相**：`.cdc` 只由 `cdc-parser` 解析；合成器内与 `cdc` 可执行文件的结果必须逐字节一致。

## 一条快捷键的完整路径

```text
键盘事件 → 合成器查 keybinds 表得 cdc 路径
        → 合成器内解析（或用户手动执行 cdc file.cdc）
        → 逐条执行：internal 直调 API；launch 关联启动上下文
        → 若来自 cdc 可执行文件：经 IPC 回合成器，合成器按同一执行器重放
```

## 执行器分层

- `cdc-parser` 只产出 `Vec<Command>` + 诊断，**不做 IO**。
- 执行由 `trait Executor` 注入：合成器内实现直接调 API；`cdc` 可执行文件实现走 IPC。
- 这样「解析一致」由类型系统保证，而不是靠两处代码约定。

## 技术选型

| 层 | 选型 | 关键理由 |
|---|---|---|
| 合成器 | Smithay 0.7 | 参考实现 anvil/smallvil 可读；渲染与协议控制力足够 |
| Shell UI | slint 1.18 + `renderer-software` | 编译期生成纯 Rust；软件渲染可直接画进 `wl_shm`，与 layer-shell 契合 |
| 配置 / `.cdc` | 自建 Rust crate | 三方共用；无运行时依赖，可离线 |
| X11 | `xwayland-satellite`（外部，可选） | 保持合成器核心干净 |
| 文件管理器 | PCManFM-Qt | 可独立运行；不提供 activation token，需合成器侧兼容 |

## 不采用

- PySide6 / Qt Quick：引入 QML 引擎，破坏纯 Rust 运行时；Wayland 抽象层不完整。
- 手写 `wayland-client` + SCTK 承担全部 Shell UI：视觉 bug 风险高，与「准确健康优先」冲突。
