# `.cdc` 语法与执行器契约

`.cdc` 是结构化命令列表，不是脚本：命令类型是封闭集合，不存在任意 shell 语法。
本节的语法已实现于 `crates/cdc-parser` 与 `crates/cdc`。

## 语法总览

```text
@{identity}                 加载身份（只作用于当前层级）
@{identity}!                卸载身份（只作用于当前层级）
@continue                   继承「启动这个执行单元时」父层的身份快照

xrun {file_path}            执行一个可执行文件
run file {sh_file_path}     用 /bin/sh 执行脚本文件
run command {sh_command}    起一个 sh -c 执行一行命令

# 独占行首（缩进之后）是注释
```

- **所有参数取到行尾**，不需要引号；因此路径与命令里可以有空格。
- 参数里的 `#` 不是注释：`run command echo 'a # b'` 原样传给 sh。
- 一行最多一条命令；空行忽略；CRLF 归一。
- 换行与缩进是语法的一部分，见下节。

## 缩进与执行单元

- 缩进更深的行属于**上一条命令的子块**。
- 子块是**独立执行单元**：身份栈重新初始化，只能通过 `@continue` 继承父层身份。
- 缩进宽度不限（2/4/8 都行），但同层必须完全相同的缩进串；tab 与空格不得混用。
- 缩进跳跃（更深但没有父命令）报 `E001`。

```text
@continue
@synchronizer
  xrun /usr/bin/kitty
  run file ~/actions/notes.sh
@synchronizer!
run command notify-send done
```

## 身份

身份是**带作用域的属性**，不是栈状态：`@X{作用域}{正则}` 决定 X 作用在本执行单元的哪些语句上。

```cdc
@name                  # 等价于 @name{:}
@name{作用域}
@name{作用域}{正则}
@name!                 # 注销：结束该身份此后开始的作用区间
```

### 作用域

位置按**当前执行单元内** 1 基计，指令自身算第 1 条。

| 写法 | 含义 |
|---|---|
| `{:}` | 指令自身到单元结束（默认） |
| `{5}` | 单元内第 5 条 |
| `{:5}` | 指令自身到第 5 条 |
| `{5:}` | 第 5 条到注销或单元结束 |
| `{3:7}` | 第 3 条到第 7 条（端点无序） |
| `{-1}` | 倒数第 1 条 |

### 筛选

第二个 `{…}` 是**正则**，匹配语句原文（去掉缩进的整行），语法同 Rust `regex` crate。
身份指令自身不参与筛选——属性不能加到一条指令上。

```cdc
@continue{:}{xrun}
run command echo hi
xrun /bin/true
```

只有 `xrun` 那条会拿到继承身份，`run command` 不受影响。

### 规则

- `@X!` 结束该身份在**此后**的作用，不回溯已执行的语句。
- `@continue` 也是身份，所以 `@continue!` 合法，语义与其它身份一致。
- `@continue` 注入的是「调用点父层的身份快照」，可重复、不去重。
- 作用域内没有匹配语句 → `W006`；作用域越界 → `E011`，该指令跳过，其余继续。
- 子块（缩进块）是独立执行单元，只继承父语句在那一刻携带的身份。

## 执行语义

- 顺序模式：一条命令跑完（`wait`）再跑下一条。
- 并发模式：`synchronizer` 生效时命令丢进后台线程，主序列继续；块结束统一 join。
- 子进程 **`setpgid(0,0)`** 脱离 `cdc` 的进程组，`Ctrl+C` 不会连带打死后台命令。
  （交接文档要求 `setsid`；当前用 `setpgid` 已足够做信号隔离，真正的会话脱离待补。）
- 退出码非 0 → `E202`，**继续执行后续命令**。
- 命令 spawn 失败 → `E201`，继续。
- `--safe`：跳过所有会创建进程的命令，记 `W005`，退出码不受影响。
- 解析失败 → 整文件拒绝执行，退出码 `3`。

## 路径

- 所有路径参数走 `chaos-path`：只认 `/`、`..`、`$HOME`、`~`，其余相对 `$HOME`。
- `xrun` 要求存在、是普通文件、有执行位；`run file` 要求存在且是文件（不要求 shebang 或执行位）。
- 不存在的路径 → `E101`（`validate` 也会报）。

## 诊断码

| 码 | 含义 |
|---|---|
| `E001` | 缩进不一致 / 跳跃 |
| `E002` | tab 与空格混用 |
| `E003` | 未知命令 |
| `E004` | 缺少参数 |
| `E005` | 身份写法错误（空名、一行多个） |
| `E007` | 文件以缩进开头 |
| `E008` | 未定义的身份 |
| `E009` | 正则无法编译 |
| `E010` | 作用域语法错误 |
| `E011` | 作用域越界 |
| `E101` | 路径不存在 / 不是文件 / 无执行位 |
| `E201` | 进程启动失败 |
| `E202` | 退出码非 0 |
| `W004` | 父层没有身份可 `@continue` |
| `W005` | 安全模式跳过命令 |
| `W006` | 作用域里没有匹配的语句 |

## CLI 与退出码

```sh
cdc <file.cdc>               # 解析并执行
cdc validate <file.cdc>      # 只解析 + 校验路径，不执行
cdc --safe <file.cdc>        # 安全模式
cdc --json <file.cdc>        # JSON 诊断；可与其他选项组合
cdc --verbose <file.cdc>     # 打印每条被执行的命令
```

| 退出码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 有运行期错误（路径、退出码、校验失败） |
| 2 | 用法错误 / 读不了文件 / IPC 不可用 |
| 3 | 解析失败，未执行任何命令 |

## 命令表 v0.2（规划）

**范围**：cdc 只做「窗口 + 合成器」的交互。应用启动、配置读写、通知、截图、剪贴板不属于 cdc。

状态：`已实现` = 现在能跑；`规划` = 等合成器 / IPC。

### win —— 窗口

| 命令 | 语义 | 状态 |
|---|---|---|
| `win focus next` / `prev` | 循环焦点 | 规划 |
| `win focus left\|right\|up\|down` | 方向焦点（几何最近邻） | 规划 |
| `win focus last` | 回到上一个焦点窗口 | 规划 |
| `win focus --match app_id=…` | 按条件选中 | 规划 |
| `win move left\|right\|up\|down [px]` | 步进平移（默认 32px），冲突则拒绝 | 规划 |
| `win move X Y` | 绝对坐标；重叠或越界则拒绝 | 规划 |
| `win move by DX DY` | 相对位移 | 规划 |
| `win swap --with <match>` | 与指定窗口交换两块 | 规划 |
| `win resize W H` | 自由尺寸（受 min_size 约束） | 规划 |
| `win resize grow\|shrink [px]` | 相对尺寸；只吃空隙，压到邻窗拒绝 | 规划 |
| `win close` | 发 `xdg_toplevel.close` | 规划 |
| `win stash` | 从画布收起，Dock 留位 | 规划 |
| `win restore [--match …]` | 还原收起的窗口 | 规划 |
| `win opacity 0.0–1.0` | 单窗透明度（会话级） | 规划 |
| `win decorate none\|csd\|ssd` | 单窗装饰模式 | 规划 |
| `win float toggle` | 脱离拼图约束：可重叠、置顶 | 规划 |
| `win fullscreen toggle` | 全屏 | 规划 |
| `win maximize toggle` | 占满**当前局部视口**（不是整块画布） | 规划 |
| `win pin` | 跨工作区粘性 | 规划 |
| `win center` | 居中到当前视口 | 规划 |
| `win raise` / `win lower` | 层级（float / 叠层用） | 规划 |
| `win retry` | 重试处于 pending 的窗口 | 规划 |
| `win link <ws>` / `win unlink` | 跨工作区同动 / 解除 | 规划 |

### 白板模型（有限画布 + 拼图）

平铺不是列式，也不是主栈，而是**有限空间上的白板**：

1. **画布**：每个工作区一块独立画布，默认 = 输出逻辑尺寸 × 3（面积 9 屏），是**有限空间**，不划分网格。
2. **节点**：窗口是圆角矩形，可自由移动、自由缩放。约束只有三条——不重叠、不越界、不小于 `min_size`。
3. **新窗口**：按 `preferred size` 找空隙；失败则缩到 `min_size` 再找；仍失败 → **pending**（不 map、Dock 占位、提示一次）。布局变动自动重试一次，也可 `win retry`。
4. **落点优先级**：当前视口内 → 视口外由近及远。
5. **移动**：`win move X Y` 绝对坐标，`win move by DX DY` 相对位移，`win move left|right|up|down [px]` 步进微调；目标冲突直接拒绝，不推挤、不吸附。
6. **缩放**：`win resize W H` 自由尺寸；压到邻窗或越界则拒绝。
7. **float**：唯一允许重叠的状态；退出 float 时若放不下，退回 pending。
8. **视口**：常规模式下是输出尺寸的矩形，可在画布内**连续**移动，不吸附九个点；越界夹到画布边缘。
9. **平移**：触控板三指滑动；鼠标默认 `Mod + 左键拖拽`。
10. **跨工作区同动**：`win link <ws>` 把窗口链接到另一工作区的画布，两处显示同一窗口并同步；`win unlink` 解除。

### pz —— 全景

| 命令 | 语义 |
|---|---|
| `pz toggle` / `pz overview on\|off` | 常规 ↔ 鸟瞰 两态切换 |
| `pz center X Y` | 常规模式下把视口中心移到画布坐标 |
| `pz land X Y` | 鸟瞰下落地：视口中心移到该点并回常规 |
| `pz minimap on\|off` | 悬停 `[PZ]` 时是否弹出小地图（外框=画布，内框=视口） |
| `pz animate 200` | 切换动画时长，0 为瞬切 |
| `pz state` | 查询模式、视口矩形、画布尺寸 |

### ws / out —— 工作区与输出

| 命令 | 语义 |
|---|---|
| `ws switch N` / `next` / `prev` | 切工作区 |
| `ws send N` / `ws send --all N` | 把当前窗口 / 整块画布送到 N |
| `ws list` / `ws rename <name>` | 列出 / 命名 |
| `out focus next` | 输出间切焦点 |
| `out set scale 1.5` | 输出缩放 |
| `out set transform normal\|90\|180\|270` | 旋转 |
| `out set mode WxH@Hz` | 分辨率 / 刷新率 |
| `out set position X,Y` | 逻辑坐标 |
| `out enable` / `out disable` / `out set primary` | 开关 / 主输出 |

不做 `out mirror`：镜像要单独走一条渲染合成路径，收益不抵成本。

### layout —— 布局

| 命令 | 语义 |
|---|---|
| `layout gap inner\|outer N` | 块间距与画布边距 |
| `layout retry` | 对所有 pending 窗口重新装箱 |
| `layout rule add app_id=… [ws=N] [float] [deco=…] [opacity=…]` | 窗口规则（声明式，持久） |

没有 `layout set`：当前只有拼图一种引擎。

### dock —— Dock

| 命令 | 语义 |
|---|---|
| `dock show` / `dock hide` | Dock 自身显隐 |
| `dock mode always\|auto\|intellihide\|hidden` | 常驻 / 触边唤出 / 被遮挡时隐藏 / 永不显示 |
| `dock position bottom\|top\|left\|right` | 贴边位置（每输出各自一份） |
| `dock opacity 0.0–1.0` | 背景透明度 |
| `dock size 48` | 厚度（高度或宽度） |
| `dock slot <component> start\|center\|end [order N]` | 内部站位分配 |
| `dock component add\|remove <kind>` | 组件开关 |

组件：`icons`（应用图标/固定启动器）、`workspaces`（工作区指示）、`pz`（PZ 按钮）、`clock`、`tray`（StatusNotifier）、`custom`（脚本输出文本）。

### theme / bg / deco —— 外观

| 命令 | 语义 |
|---|---|
| `theme set <name>` / `theme list` | 切换 / 列出主题 |
| `theme mode light\|dark\|auto` | 亮暗；auto 按时间 |
| `bg set <color\|path\|gradient>` | 背景，默认全局一套 |
| `bg fit cover\|contain\|tile` | 背景铺法 |
| `deco border width N` / `color #RRGGBB` / `radius N` | 全局边框 |
| `deco shadow on\|off\|strength N` | 全局阴影 |
| `deco inactive-opacity A` | 非焦点窗口统一透明度 |

主题是 token 集合（背景、边框、Dock、文字、强调色、透明度）；取色由用户手动选或主题包声明，不做自动取色。

### query —— 查询

`query focused` / `windows` / `workspaces` / `outputs` / `layout` / `pending`

输出结构化值，可直接参与判断（`if query focused.app_id == "kitty"`）。

### 控制

| 命令 | 语义 |
|---|---|
| `quit` | 退出合成器 |
| `reload` | 重载配置与布局 |
| `log <level> <msg>` | 写日志 |

### `@persist`

**默认所有改动只活在当前会话**，重启回落到配置文件。`@persist` 是一个身份：

```text
@persist
  dock position left
  dock opacity 0.85
  theme mode dark
@persist!
```

- 只在当前层生效；块内命令依次执行。
- 块结束后**一次性**合并写回主配置（原子写 + 快照），不是每条命令写一次。
- 只写可持久化的键：`dock.*`、`theme.*`、`bg.*`、`deco.*`、`layout.gap*`、`layout.rule.*`、工作区命名。
- 瞬时状态（焦点、窗口位置、单窗透明度）不写回。
- 块内任何一条命令失败 → 整块不写回，报 `E2xx`。

## 尚未实现

- `win` / `pz` / `ws` / `out` / `layout` / `dock` / `theme` / `bg` / `deco` / `query` 全部命令：等合成器与 IPC。
- `@persist`：依赖配置服务。
- `@no-error-output` 及其索引集合：语法与预扫描设计见交接文档 §5.6。
- `cdc` 目前没有 IPC 客户端：所有命令本地执行。
