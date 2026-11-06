# 技术决策

每条决策写「选了什么 / 为什么 / 代价」。要改先改这里，再改代码。

## D1 配置格式用 TOML + `toml_edit`

需求是键路径扁平化、逐键写回、保留注释与顺序。KDL 的优势（嵌套表达）在扁平配置里用不上，工具链生态也弱。代价：手写 include 逻辑。

## D2 `.cdc` 手写词法/语法分析器

交接文档的语法是缩进敏感 + `@身份` + `{索引集}`，与 KDL 6 规范不同。复用 `kdl` 会制造「半个 KDL」的解释负担。代价：自建测试基线。

## D3 Schema 用 serde + 自建元数据宏

`settings-schema-rs` 绑定 GTK/libadwaita 的 widget 抽象，与 slint 不兼容。只需要「键路径 / 类型 / 控件提示 / 等级」四元组。

## D4 calloop 单线程 + IPC 独立线程

Smithay 0.7 基于 calloop 0.14。引入 tokio 会变成双 runtime + 跨线程状态同步；把非阻塞 socket 塞进 calloop 会把协议状态机摊进事件循环。取独立线程阻塞 IO + `calloop::channel` 回主循环。

## D5 IPC 用 JSON-lines

换行分隔、单帧 ≤ 1 MiB、带 `v` 字段。配置与命令消息极小，JSON 可 `nc -U` 手工调试。codec 与帧格式解耦，将来换 postcard 不动协议。

## D6 `chaos-layout` 独立成 crate

平铺算法最容易出 bug 又最需要回归。用自己的 `Rect` / `WindowId`，不引入 Smithay 类型，几何可 proptest。

## D7 `smithay = "=0.7.0"` 精确 pin

pre-1.0 快速演进，API 会碎；参考实现与版本强绑定。只在阶段边界升版，升版单独一个 PR。

## D8 slint 用 `renderer-software` + 自建 `Platform`

slint 的 winit 后端只能出 `xdg_toplevel`，做不了 layer-shell。软件渲染直接画进 `wl_shm` buffer，与 SCTK layer surface 天然契合；Dock 面积小，CPU 足够。代价：自建 `WindowAdapter` + damage 管理。

## D9 一套 UI 双宿主

`chaos-settings` 带 `--in-shell` 模式，in-shell 面板就是它。交接文档要求双前端功能对等；两份实现必然漂移。

## D10 PZ 是渲染期全局变换

一次变换覆盖所有客户端，客户端不需要 fractional-scale，XWayland 也不需要占位缩略图降级。输入侧做坐标反变换。

## D11 进程创建统一走 `chaos-proc`

`posix_spawn` + `setsid` + 显式环境清理，集中处理 activation token 与启动上下文关联。避免各处手写 spawn 长出孤儿。

## D12 PZ 几何：局部视野 1/9 ↔ 全景视野 1/1

- **世界画布** = 输出逻辑尺寸 × 3（每维 3 倍，面积 9 倍）。
- **局部视图（默认）**：1:1 显示，视野 = 画布的 **1/9**，即正好一个屏幕。
- **全景视图（PZ 开）**：整块画布缩放到屏幕内，比例 **1/3**，视野 = 9/9。
- 切换入口：Dock `[PZ]` 按钮 + 快捷键；合成器侧完成视口变换，Shell UI 只切按钮状态。
- **Dock / layer-shell 面板不参与缩放**，固定物理尺寸，否则全景下点不到。
- 全景下点击 = 跳到该世界坐标并回到局部视图；不做「缩略图内直接拖窗」。
- 光标固定物理尺寸，不随缩放变大。

## 待验证 spike

| ID | 问题 | 判定标准 | 失败退路 |
|---|---|---|---|
| S1 | slint 渲染进 SCTK layer-shell surface | Dock 静态 CPU < 3%、交互 ≥ 30fps、无撕裂、命中准确 | Dock 退化为纯 SCTK 自绘；启动器保留 slint + `xdg_toplevel` |
| S2 | 任意比例缩放的清晰度与开销 | 1/3 档 GPU 帧时间 < 8ms；缩略模糊可接受 | 缩放档改 1/2 |
| S3 | 非整数缩放下客户端是否偏移 | 点击落点误差 0 像素 | 临时下发 `wp_fractional_scale` |
| S4 | `toml_edit` 只改目标键 | 改一个键 `git diff` 只有 1 行；往返幂等 | 整文件重写 + 注释锚点；或改 KDL |
| S5 | IPC 独立线程是否卡帧 | 忙时 1000 条 `apply`，主循环单帧 < 16ms | 改 io_uring / 非阻塞状态机（成本高） |

## D13 白板 = 严格不重叠的自动拼图

不是列式（Niri）也不是主栈（dwm）：窗口是画布上的矩形块，自动装箱、不重叠，放不下就拒绝并提示。理由：与 3×3 画布 + PZ 的设计同源，且「位置由用户决定」比「由算法决定」更符合项目定位。代价：需要装箱算法与 pending 机制，`win move` 的语义变成「平移或交换」而不是「换列」。

## D14 装饰：默认无标题栏，单应用可切 CSD / SSD

默认不给标题栏（Niri 风格），焦点靠边框区分；需要标题栏的应用可以按应用或用 `win decorate` 临时切到 CSD / SSD。理由：全局统一画 SSD 要为每个窗口维护按钮与拖拽状态；纯 CSD 又会让风格散掉。代价：`xdg-decoration` 协商要处理「客户端坚持 CSD」的降级。

## D15 默认不持久化，`@persist` 身份才写回

cdc 改的是会话状态，重启回落到配置文件。只有 `@persist` 作用域内的命令，在块成功结束后**一次性**合并写回主配置。理由：按键脚本高频改盘会不断触发快照与热重载；显式标记让「哪些改动会留下」可读可控。代价：用户容易忘记加 `@persist`，需要在文档与 `--verbose` 里提示。

## D16 测试形态：嵌套窗口优先，TTY 只做验证

- **默认开发自测**：`chaos-compositor --nested`（Smithay winit 后端），跑在 KDE 的一个窗口里。DRM/KMS 完全没被接管，合成器崩溃只死那个窗口。
- **CI / 回归**：`--headless`（pixman / renderer_test），跑协议与渲染快照，不需要显示服务器。
- **TTY / DRM**：只在里程碑验证，且必须带看门狗——启动后若 60s 内无输入、或无健康心跳，自动退出并 `chvt` 回原 VT。
- **跨进程预览**：需要 `ext-image-copy-capture`（或旧版 `wlr-screencopy`）由客户端主动拉画面；这类工具（Noctalia 的桌面窗口就是）只能看、不能保命，不放进核心路径，阶段 6 之后再考虑。

理由：预览客户端寄生在同一个合成器上，合成器一崩它一起没；嵌套后端才是真正的崩溃域隔离。代价：嵌套模式下输入要经过外层合成器转发，测不到 libinput/DRM 路径，所以 DRM 验证不能省。
