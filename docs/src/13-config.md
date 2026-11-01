# 配置系统契约

## 格式与解析

- **TOML + `toml_edit`**（决策 D1）。要的是「键路径扁平化 + 逐键写回 + 保留注释与顺序」，不是嵌套表达力。
- KDL 只用于 `.cdc` 的**外观**参考，配置层不用。

## 层次与覆盖

```text
优先级（高 → 低）：主配置文件 > include（按书写顺序，后覆盖先） > 内置默认
```

- `include "path"` 与 `include { path = "...", optional = true }`；optional 缺失静默跳过。
- 命令组文件路径的 include 是叶子，不再嵌套。
- 扁平化为 `BTreeMap<KeyPath, (Value, Origin)>`，逐层覆盖。

## 根源追踪

```rust
struct Origin {
    file: Option<PathBuf>,  // 内置默认层为 None
    line: u32,
    col: u32,
}
```

- 每个最终值携带来源，合并时逐层覆盖，指向**最后覆盖它**的位置。
- GUI 展示把 `$HOME` 折叠为 `~/`，**比较与写回一律绝对路径**。
- 写回目标永远是主配置文件；`include` 层只读（要改就把值提升到主文件，并在 UI 明示）。

## 写回与原子性

1. 只改写目标键的 span，保留注释、空行、键顺序、多行数组格式（spike S4 验证）。
2. 写入 `*.tmp` → `fsync` → `rename` → `fsync` 父目录。
3. 任何 `apply` 之前写快照：`$XDG_STATE_HOME/chaos-desktop/snapshots/<RFC3339>.toml`，保留最近 20 份、单份 < 1 MiB。
4. 迁移前、进入安全模式前也各写一份快照。

## 分级

| 等级 | 行为 |
|---|---|
| `safe` | 即时生效，可回滚 |
| `requires_restart` | 提示 + 记录回滚点，重启后生效 |
| `unsafe` | 默认折叠，**仅配置文件可写**，GUI 只读展示 |

## Schema

- 用 Rust 结构体 + 自建派生宏（决策 D3），产出四元组：`键路径 / 类型 / 控件提示 / 等级`。
- 一个 schema，三个消费方：in-shell 控件、独立应用控件、`validate`。
- 校验器是独立库 crate，**不依赖 Wayland**，合成器崩溃后仍可离线校验。

## 版本与迁移

- 顶层强制 `schema_version`（缺省视为 0）。
- 迁移链 `v0→v1→v2…` 为纯函数，逐级可单测；迁移失败不写盘、报错、加载默认。
- GUI 不展示、不允许编辑 `schema_version`。

## v0.1 配置项（最小集合）

| 键 | 类型 | 等级 |
|---|---|---|
| `output.<name>.scale` | 1.0–3.0 | requires_restart |
| `layout.gap_inner` / `layout.gap_outer` | u32 px | safe |
| `layout.engine` | enum | requires_restart |
| `keybinds.<key>` | `.cdc` 路径 | requires_restart |
| `pz.zoom` | 3.0 | safe |
| `pz.animation_ms` | u32 | safe |
| `pz.center` | `pointer` / `screen` | safe |
| `shell.enabled` | bool | requires_restart |
| `shell.dock.height` / `shell.dock.position` | u32 / enum | safe |
| `shell.in_shell_panel` | bool | requires_restart |
| `safety.mode` | bool | safe |
| `safety.allowed_general_keys` | list | safe |
| `paths.actions_dir` | 目录 | safe |
| `schema_version` | u32 | — |

新增配置项必须先改本节，再写代码。

## 路径规则（摘要）

- 只认四个特殊内容：`/`、`..`、`$HOME`、`~`。其余按 home 相对解析。
- `FilePath` / `DirPath` 用 newtype 强制区分；文件路径无尾斜杠，目录路径有。
- 规范化在字符串层完成，不解析符号链接、不依赖 cwd；可能越出 home，由 `validate` 给提示而非拒绝。
- 存在性、是否目录、`..` 前段是否目录，全部在 `validate` 检查。
