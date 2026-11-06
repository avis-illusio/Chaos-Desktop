# 仓库布局

目标是 workspace 多 crate；当前只有根清单与空 `src/main.rs`，拆分为阶段 0.1 的第一件事。

```text
chaos-desktop/
├─ Cargo.toml              # workspace 根（含一个只作锚点的根包）
├─ README.md               # 手写：定位与特性，不放技术细节
├─ onedoc.toml             # 文档生成配置（只产出 docs/design.md）
├─ crates/
│  ├─ chaos-path/          # ✅ FilePath/DirPath 解析（零依赖）
│  ├─ cdc-parser/          # ✅ .cdc 词法 / 缩进树 / AST / 诊断
│  ├─ cdc/                 # ✅ cdc 可执行文件（执行器 + CLI）
│  ├─ chaos-ipc/           # ⬜ 线协议与客户端/服务端
│  ├─ chaos-config/        # ⬜ 配置值模型、合并、来源追踪、校验、快照
│  ├─ chaos-layout/        # ⬜ LayoutEngine trait 与纯几何实现
│  ├─ chaos-compositor/    # ⬜ Smithay 合成器（内含配置服务）
│  ├─ chaos-shell/         # ⬜ Dock / 启动器 / PZ 按钮（slint）
│  └─ chaos-settings/      # ⬜ 设置应用（同一二进制兼 in-shell 模式）
└─ docs/
   ├─ src/                 # 手写文档源（唯一可编辑处）
   ├─ templates/           # onedoc 模板
   ├─ design.md            # 生成物，勿手改
   └─ handover-v0.1.md     # 归档
```

## 依赖方向

```text
chaos-path ← chaos-config ← cdc-parser
     ↑            ↑            ↑
     └────────────┴────────────┴──→ chaos-ipc ← compositor / cdc / shell / settings
chaos-layout（纯几何，不依赖 Smithay 类型）
```

- `chaos-ipc` 不依赖合成器：消息是纯数据。
- `chaos-config` 不依赖任何 Wayland / 图形 crate，保证离线可用。
- `cdc-parser` 不依赖 IPC：只产出执行计划，由调用方注入 `Executor`。
- `chaos-layout` 用自己的 `Rect` / `WindowId`，保证几何可单测。
