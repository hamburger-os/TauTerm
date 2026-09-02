# TauTerm 图标生成规范（V4）

本文件是 `src/assets/icons/` 的唯一语义依据；`style-contract.json` 是机器可读的家族、参考图与调色板契约。图标必须同时满足「资源键」「实际调用位置」「固定参考图」「12px 视觉验收」；不得只按名称、自由提示词或单张旧图猜测用途。

## 强制生成流水线

1. **先注册、后生成**：新图标先在本表新增唯一语义行，并同步 `Icon.tsx` 与 `scripts/check-icons.mjs` 的注册表。未注册的键不得生成或进入运行时目录。
2. **禁止纯文本裸生成**：运行 `npm run prompt:icon -- <key>`，使用命令输出的完整提示词，并把输出列出的三张参考图全部传给图像生成工具。不得自行删减全局正向、负向或参考图契约。
3. **固定家族锚点**：普通功能图标固定参考 `settings`、`plus`、`lock`；微型控制图标固定参考 `window-maximize`、`chevron-down`、`sidebar-left`。家族和键列表只在 `style-contract.json` 维护，生成者不能临时挑选“看起来相近”的参考图。
4. **候选不直写运行时**：生成稿先放在运行时目录之外，规范化为 256×256 RGBA 后运行 `npm run check:icons -- --strict --candidate <key> <png-path>`；通过后才替换目标 PNG。不合格稿直接丢弃，不通过局部去背或着色掩盖根本风格偏差。
5. **提交机器门禁**：替换后再次运行 `npm run check:icons -- --strict`。它要求 PNG、`PNG_MAP`、本语义表一一对应，并检查透明边距、alpha 核心、重复资产、黑边、浅冰蓝均值、深色核心和紫/靛色漂移。
6. **真实尺寸人工门禁**：运行 `npm run preview:icons`，在深色/浅色背景的 12/14/18/24px 下，将候选与其三张固定锚点并排检查语义、视觉重量、玻璃厚度和圆角。任何一项不一致都重新生成，不能以 256px 大图“看起来正常”为通过理由。

机器检查只能排除可测量的漂移，不能判断业务语义是否准确；因此严格检查与真实尺寸人工验收缺一不可。

## 固定技术与风格约束

- 本注册表的每个功能图标单独生成一张 **256×256、8-bit RGBA、透明背景 PNG**；不使用 SVG、字体字形或 CSS 蒙版。唯一例外是连接状态点，它们由 `Icon` 的 CSS 状态类提供主题色语义。
- 主图形位于 `x/y=32…223` 的 192px 光学框，透明安全边距至少 32px。不得留黑底、裁切边缘、去背毛边、投影或方形底板。
- 家族为「浅冰蓝、清晰优先的轻玻璃」：淡青冰蓝半透明主体、亮天蓝外缘、白色柔光、极少折射、圆润厚边与连接处。所有功能图标禁止银色金属、深蓝、紫色/靛色、左右双色或高反差镀铬；12px 时要先读出轮廓、方向或状态，禁止仅剩细线框、密集小孔、文字和装饰碎片。
- 窗口控制、箭头、折角、键盘、侧栏和视图切换同样是 PNG，但采用更少细节、更大留白的微型控制族，保证 12–18px 清晰度。
- 除语义上必须是短横的 `window-minimize` 外，新图标的高不透明度核心最长边目标为 180–188px；不能靠大面积透明留白、模糊阴影或低透明度外光凑尺寸。
- 同族关系必须保持共同几何基准：成组方向图标使用相同笔画、端点和视觉重量；镜像图标只做镜像；功能图标的玻璃厚度和高光柔度以固定锚点为准。单个语义行可以收紧形状，但不得覆盖全局调色板和材质契约。
- 生成后在深色与浅色背景各验收 12/14/18/24px；`logo` 另验收 16/32px。运行 `npm run check:icons -- --strict` 与 `npm run preview:icons`。

**基础正向提示词：**

`One isolated TauTerm UI glyph, exact 256 by 256 RGBA PNG with a fully transparent background. [SEMANTIC SHAPE]. Center the single readable silhouette inside a 192px optical frame with 32px transparent safe margins. Pale cyan ice-blue translucent glass body, bright sky-blue outer rim, soft white highlight, restrained refraction, thick rounded caps and joins, minimal 3D depth, crisp anti-aliased edges, consistent optical weight with the attached TauTerm reference icons, legible at 12px. No text.`

**负向提示词：**

`no background, no tile, no black matte, no frame, no external drop shadow, no green or teal cast, no dark navy body, no purple or indigo, no silver metal, no split two-tone halves, no central seam, no rainbow, no neon, no high-contrast chrome, no wireframe-only outline, no sharp knife-like edge, no letters, no numbers, no tiny interior decoration, no duplicated glyph, no cropped edge, no opaque canvas fringe.`

## 语义注册表与真实使用位置

| 键 | 实际位置与唯一语义 | 提示词形状／禁止形态 |
| --- | --- | --- |
| `logo` | 工具栏与空状态的 TauTerm 品牌标识 | 保留四色品牌流光；不套用功能图标蓝色规则。 |
| `appearance` | 设置 → 外观 | 中空半圆调色盘与三颗大圆点；不是画笔。 |
| `arrow-down` | 回到底部、文件降序、RX、更新下载进度 | 粗短下向箭头；不是 V 折角或托盘下载。 |
| `arrow-left` | 连接设置页返回 | 粗短左向箭头；不用于折叠。 |
| `arrow-right` | 设置 → 关于页版本从当前到新版本 | 粗短右向箭头；不用于树状展开。 |
| `arrow-up` | 文件升序、TX | 粗短上向箭头；不是上传托盘。 |
| `caret-down` | 下拉控件与发送历史 | 紧凑实心向下三角；只表示下拉，不使用 V 形折角。 |
| `chart` | iperf 吞吐与统计 | 三根高度明确柱加一条上升线；不画仪表盘。 |
| `check` | 选项已选、普通确认 | 单一粗圆角勾；没有圆环。 |
| `check-circle` | 传输完成、成功结果 | 中空圆环内实体勾。 |
| `chevron-down` | Terminal SearchBar 下一匹配、RightSidebar 基础图形（展开通过 CSS 旋转）、AutoReply/Script 输出面板展开态 | 对称短臂下折角；不作滚动或下载。 |
| `chevron-right` | 仅用于 AutoReply/Script 输出面板收起态 | 对称短臂右折角；不作版本前进或树状展开。 |
| `chevron-up` | 仅用于 Terminal SearchBar 上一匹配 | 对称短臂上折角；不作通用折叠。 |
| `clipboard` | 终端复制、复制动作 | 中空剪贴板、实体夹子、两条短内容线；外轮廓和夹子要足够大，12px 仍像剪贴板；不做实心纸块或密集文档。 |
| `close` | 普通弹窗、Toast、可关闭提示 | 紧凑圆角 X；不是错误状态和窗口关闭。 |
| `code` | 脚本编辑模式、代码操作 | 一对大圆角尖括号；无小字。 |
| `commands` | SendBar 的「指令」模式入口 | 三条疏朗命令行与一个简单提示符，整体是命令面板语义；不画细节密集终端窗口、浏览器窗口或键盘。 |
| `connection` | 串口会话、SSH 已连接时新建通道 | 两端圆角接头与短线缆；不能画无线电波。 |
| `construction` | 开发中／工具入口 | 中空扳手与小齿轮，二者保持大轮廓。 |
| `download` | 文件下载、TFTP 接收 | 竖直下箭头明确落入托盘；箭头杆、箭头头和整体笔画宽度与 `upload` 对齐，不得用过宽箭头头或厚重托盘；区别 `arrow-down`。 |
| `drag-handle` | 命令/规则真实拖拽排序 | 两列各三枚大圆点；不得用于视图切换。 |
| `edit` | 重命名、编辑文件/规则 | 粗圆角铅笔横跨短基线。 |
| `endpoint` | 地址、主机或端口 | 中空定位针与中心大圆点。 |
| `file` | 普通文件 | 中空单页、清晰折角；无多行细字。 |
| `folder` | 目录 | 中空文件夹与明显开口。 |
| `globe` | Telnet 与网络协议 | 地球外环和极少经纬线；不塞大陆。 |
| `hourglass` | 等待传输 | 两个清晰腔体的沙漏。 |
| `info` | 设置 → 关于 | 中空圆环内大写意 i。 |
| `keyboard` | 设置 → 快捷键 | 中空键盘外形、仅 5–6 枚大键位与空格键；禁止密集键阵。 |
| `lock` | 设置 → 安全 | 中空 U 锁梁、实体简洁锁身；不放钥匙孔。 |
| `log` | 会话开始/停止日志、日志页 | 参考 `edit` 的厚实单页构图和折角节奏：中空记录页、左侧时间点和两条疏朗记录线；页面轮廓要占主要面积但保持透明内腔，不画成实心文档、剪贴板或自动回复步骤，也不加入铅笔。 |
| `loop` | 循环发送 | 两条闭环箭头，和刷新方向差异明显。 |
| `package` | Y/X/ZMODEM、TFTP 文件传输协议 | 中空或半透明的简洁包裹，使用清晰的大十字封箱带和顶部折线；轮廓要占满光学框，不能用模糊高光、密集折射、实心蓝方块或无线天线。 |
| `paste` | 终端粘贴 | 与 `clipboard` 使用同宽、同视觉重量的中空剪贴板骨架，加一枚占主要面积的向内下箭头；主体可以比旧稿略宽，箭头和夹子要分离，不能与复制同形、不能塞入一张小纸。 |
| `play` | 启动、连接、执行 | 实心圆角右三角，留白足够。 |
| `plus` | 新建会话、添加规则 | 等宽短臂圆角加号；SSH 新通道改用 `connection`。 |
| `refresh` | 刷新目录、刷新端口 | 单一顺时针回转箭头；不与 `loop` 相同。 |
| `robot` | 自动回复 | 简洁中空机器人头、两枚大眼；无密集天线。 |
| `search` | 全局/会话搜索 | 单一放大镜环和短柄。 |
| `send` | 发送栏 → 基础发送模式 | 一个紧凑、近方形的中空终端数据框，带一至两条短文本线，右侧接粗短“送出”箭头或数据尾迹；必须同时读出“数据框 + 发送”，不能拉成长条，不能只剩孤立右箭头、纸飞机或上传托盘。 |
| `settings` | 设置入口 | 六齿大齿轮与中心孔。 |
| `shield` | Local Shell 管理员子会话与“以管理员身份新建”动作 | 对称单体盾牌；严格跟随 `settings`、`plus`、`lock` 的淡青色半透明玻璃主体、亮天蓝描边与白色柔光；肩部、底尖和厚边框都必须圆润，12px 时仍清晰。禁止银色金属、深蓝/紫色、左右双色、中央分割线、锐角、锁、钥匙、星章或文字。 |
| `sidebar-left` | 工具栏左上角会话栏开关 | 与 `sidebar-right` 使用完全相同的近方形主窗格比例（约 1.2:1）、线宽和留白；中空主窗格、左侧独立窄栏、清晰竖分隔，窄栏可用实体冰蓝填充，仅左右镜像；不得画成汉堡菜单、超长条或整块实心面板。 |
| `sidebar-right` | 工具栏右侧栏开关 | 与 `sidebar-left` 使用完全相同的近方形主窗格比例（约 1.2:1）、线宽和留白；中空主窗格、右侧独立窄栏、清晰竖分隔，窄栏可用实体冰蓝填充，仅左右镜像；不得画成超长条或整块实心面板。 |
| `ssh-shell` | 新建会话 → SSH | 放大的简化终端框和粗 `>_` 图形线，终端框占主要面积；不画锁、网络地球、密集窗口控件或缩小的蓝色方块。 |
| `status-cancelled` | TFTP/文件传输已取消 | 占满光学框的粗圆环内斜杠，斜杠与圆环保持清晰负空间；区别失败 `x-circle` 与普通 close，不做显小的细线符号。 |
| `status-skipped` | 跳过状态 | 前行短箭头越过短横；不是双快进细线。 |
| `steps` | 自动回复规则动作数量 | 三个大顺序节点以短线连接，节点和连接线占主要面积；不是日志记录、菜单或密集流程图。 |
| `stop` | 停止、断开 | 实心圆角方块；不加叉。 |
| `stopwatch` | 定时发送 | 中空秒表和顶部按钮，表盘细节极少。 |
| `tag` | 会话名称与标签 | 中空标签和单一大孔。 |
| `transfer-active` | 传输中行、终端传输横幅 | 两条占满横向光学框的清晰水平反向传输箭头，中间留出可辨间隙；不能看成 `loop`、刷新或一枚小右箭头。 |
| `trash` | 删除 | 中空桶身、桶盖和两条粗竖槽。 |
| `upload` | 文件上传、文件传输发送 | 上箭头明确离开托盘；不用于基础数据发送。 |
| `view-grid` | 文件管理器当前列表视图的目标切换 | 2×2 四块大方格；只表示“切换到网格”。 |
| `view-list` | 文件管理器当前网格视图的目标切换 | 三条粗记录线和左侧点；只表示“切换到列表”。 |
| `warning` | 警告 Toast/提示 | 中空圆角三角和大感叹号。 |
| `window-close` | 标题栏关闭窗口 | 极简 X，必须与 `window-minimize`、`window-maximize`、`window-restore` 使用同一笔画厚度、圆角端点、核心尺寸和视觉重量；不复用普通 `close`，不加圆环。 |
| `window-maximize` | 标题栏最大化 | 极简中空方框，外框尺寸、线宽和视觉重量与 `window-minimize`、`window-restore` 同族一致；不画实心方块。 |
| `window-minimize` | 标题栏最小化 | 单一短横线。 |
| `window-restore` | 标题栏还原 | 两个错位中空方框，后框与前框保持清晰间隙；线宽、端点和视觉重量与最大化/最小化同族一致，不画成细小双框。 |
| `x-circle` | 失败、正则错误/不匹配 | 中空圆环内粗 X；不作普通关闭。 |

## 验收与变更规则

1. 新增、删除或重命名图标时，必须同步修改 `Icon.tsx` 的 `PNG_MAP`、本表、`scripts/check-icons.mjs`、真实调用点与双语文档。
2. `antenna` 与 `menu` 已删除：前者曾错误暗示 XMODEM 无线语义，后者曾误表示左侧会话栏。
3. 任何调用若不符合上表，应新增专用键或迁移调用；不得以视觉相似替代业务语义。
4. 资源通过检查后再删除候选目录，避免候选资产进入运行时注册表。
5. 任何新图标必须保存 `npm run prompt:icon -- <key>` 的最终输出作为生成输入；若生成工具不支持多参考图，则不得用该工具产出运行时图标。
