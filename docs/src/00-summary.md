Chaos Desktop 是一个用 Rust 从零构建的 Arch Linux Wayland 桌面壳：Smithay 做合成器，slint 做 Shell UI，`.cdc` 是操控合成器的唯一格式。目标是以平铺为主替代 KDE，并带一块 9 屏大小的有限画布（鸟瞰模式 PZ）。**当前状态**：`.cdc` 解析器与 `cdc` 执行器已可用；合成器、Shell 与配置系统尚未开始。

# 硬约束

- **全栈纯 Rust 运行时**：不引入解释器、VM、脚本引擎（QML / Python / Lua / Node）；链接 libwayland、libEGL 这类系统 C 库不算破坏约束。
- **不软链系统库**：卫星工具用 `provides` / `conflicts` 声明。
- **`.cdc` 是封闭命令集合**：不存在任意 shell 语法；复杂逻辑走 `.sh`，只能由 `xfile` 进入。
- **`unsafe` 只在合成器 crate 内允许**，且必须注释安全前提。
