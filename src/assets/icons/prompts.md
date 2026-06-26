# TauTerm Icon Generation Prompts / 图标生成提示词

> **开发者**: 图标组件用法详见 `src/components/common/Icon.tsx` 源码注释。
> 三级渲染体系：Tier 1 (PNG mask-image, 29个) / Tier 2 (CSS 状态圆点, 4个) / Tier 3 (内联 SVG, 11个含 plus)。
> 尺寸预设: xs=12 / sm=14 / md=18 / lg=24 / xl=36 / 2xl=48（像素）。
> **规则**: PNG 图标最小 `sm` (14px)，SVG 图标可在 `xs` (12px) 使用。
> `color` prop 为 PNG 切换 mask-image 模式（主题适配），省略则渲染 `<img>` 保留原始玻璃质感。

## 通用技术规范 / General Technical Specs

所有图标遵循以下统一规范：

| 参数 | 值 |
|------|-----|
| 画布尺寸 | **256×256 像素** |
| 图形颜色 | **半透明霜白玻璃**，带明亮高光与深灰折射阴影 |
| 背景 | **纯黑 #000000**（方便后期用屏幕混合模式过滤，或用抠图工具提取半透明通道） |
| 格式 | **PNG-24** |
| 图形区域 | 居中，四周保留 **12%** 边距（约 30px） |
| 描边等效 | 在 256px 画布上约 **16-20px** 粗（缩小到 18px UI 显示时视觉等同 2.5-3px） |

## 风格关键词 / Style Keywords

> **中文**：纯黑背景，3D UI 图标，极致的液态玻璃质感，磨砂半透明材质，高级玻璃拟态（Glassmorphism），内部带有柔和的体积光与折射，边缘有明亮锐利的纯白高光（Specular highlight），圆润胶囊描边风格，有机流体形态，像水滴一样平滑，无任何尖锐棱角，8k 分辨率，Octane Render 渲染级别的通透感。
>
> **English**: Pure black background, 3D UI icon, ultimate liquid glass texture, translucent frosted glass material, premium glassmorphism, soft volumetric light and refraction inside, bright sharp pure white specular highlights on the edges, rounded capsule stroke style, organic fluid form, smooth like water droplets, no sharp corners anywhere, 8k resolution, Octane Render level transparency and depth.

## 形态要求 / Shape Requirements

1. **圆润胶囊描边** — 所有线条端点为半圆形（round cap），转角为大圆角（round join），如液态玻璃管弯曲
2. **有机流体曲线** — 避免僵硬几何直线，图形带轻微自然弧度
3. **视觉重量一致** — 同尺寸下视觉密度相似
4. **无尖锐棱角** — 任何转角至少 3-4px 等效圆角
5. **3D 玻璃雕塑** — 图形为半透明液态玻璃材质渲染，带明亮边缘高光与内部折射，背景纯黑 #000000（便于后期抠图提取半透明通道，或使用屏幕混合模式叠加到 UI 上）

---

## 图标提示词 / Icon Prompts

以下 29 个图标均为 TauTerm 液态玻璃主题独立设计的 UI 功能图标。每个图标的设计从**功能语义**出发，以 TauTerm 的视觉语言（圆润胶囊描边、有机流体曲线、磨砂玻璃质感）为统一表达，而非对 emoji 或其他图标集的模仿。设计方案优先考虑：在 UI 中 18×18px 的可辨识性、与液态玻璃主题的视觉融合度、以及图标集的整体风格一致性。

The following 29 icons are purpose-built UI functional icons for TauTerm's liquid glass theme. Each icon design starts from its **functional semantics**, expressed through TauTerm's visual language (rounded capsule strokes, organic fluid curves, frosted glass texture) — not as imitations of emoji or other icon sets. Design priorities: recognizability at 18×18px in UI, visual cohesion with the liquid glass theme, and stylistic consistency across the entire icon set.

### 1. logo — 应用 Logo + 运行图标

> **重要：此图标同时作为 App 运行图标使用**（窗口图标、任务栏图标、Favicon）。
> 需要在缩小到 16×16 和 32×32 时仍清晰可辨。建议造型简洁有力，避免过于复杂的细节。

**中文提示词：**
一个科技终端应用"TauTerm"的精美 App 图标，256×256像素。背景为纯黑色（Pure black）。图标的底托是一块带有明显厚度与磨砂质感的半透明液态玻璃片（如一块悬浮的毛玻璃或磨砂亚克力板，不要完全透明，需要展现出细腻的磨砂颗粒、内部折射以及实体材质的体积感与光泽）。主体设计融合希腊字母 τ (tau) 与终端提示符 >_ 为一个有机整体。τ 的左侧为粗壮垂直胶囊形竖干，右侧弧线向下自然弯曲。右下方是终端风格的大于号 >，以两个圆润折角构成。底部是一条粗壮的圆角水平下划线 _，提供稳定底座。所有线条端点半圆形，转角大圆角，无任何锐角。核心图案材质为流光彩色的液态玻璃，色彩与光效完全还原经典的四色流光渐变（顶部为珊瑚红/粉红色，向左平滑过渡到亮黄与橙色，向下过渡到鲜艳的翠绿色，向右过渡到明亮的纯蓝色，中心呈现出四色自然交融的柔和漫反射效果）。在纯黑背景的衬托下，背后的磨砂液态玻璃片与前方绚丽的发光主体产生强烈的互动，通过毛玻璃的漫反射将前方的四色流光温柔地晕染开来，既保留了实体的细腻质感和边缘倒角的微光，又展现出极致的高级感、AI 智能感与未来科技感。造型紧凑有力，确保缩小到 16×16 时仍可辨识。

**English prompt:**
A premium App icon for a tech terminal application "TauTerm", 256×256 pixels. Set against a pure black background (#000000). The icon is backed by a translucent liquid glass sheet with visible thickness and a frosted matte texture (like a floating piece of frosted glass or frosted acrylic pane; it should NOT be completely transparent, clearly showing the fine frosted grain, inner volume, and physical material texture). The main design fuses the Greek letter τ (tau) with terminal prompt >_ into one organic whole. The τ has a thick vertical capsule stem on the left with a naturally curving right arc. A terminal-style greater-than symbol > made of two rounded bends sits at lower right. A thick rounded horizontal underscore _ forms the stable base. All strokes use round caps and round joins, no sharp edges anywhere. The core inner symbol is made of flowing colorful liquid glass, featuring the exact iconic dynamic glowing gradients of a four-color blend (a seamless, fluid transition with coral red/pink at the top, bright yellow and orange on the left, vivid emerald green at the bottom, and bright pure blue on the right, with a soft diffused blend of all four colors in the center). Set against the pure black background, the frosted liquid glass pane beautifully diffuses the vibrant four-color neon glow from the symbol in front of it. The frosted texture captures and scatters the light, emphasizing the pane's physical presence, glossy chamfered edges, and creating an incredibly premium, futuristic AI sci-fi vibe. Compact and bold form, must remain recognizable at 16×16 for taskbar/favicon use.

**使用位置：** `src/assets/icons/logo.png`（UI 中引用）、`/favicon.png`（浏览器标签页图标，由 Vite 构建时自动从 logo.png 生成，参见 `vite.config.ts` 中的 `favicon-from-logo` 插件）、`src-tauri/icons/icon.png`（窗口/任务栏图标）

**技术参数：** 256×256, 四色流光渐变液态玻璃 + 磨砂玻璃底托, pure black bg, PNG-24

---

### 2. zmodem — ZModem 协议

**中文提示词：**
一个 3D 快速文件传输图标，纯黑背景，256×256像素。设计为一个带有速度感弧线轨迹的圆角菱形（象征快速传输）。整体以液态玻璃材质渲染，是一个具有磨砂半透明质感、明亮边缘高光与内部灰白光线折射的 3D 玻璃雕塑。整体造型柔软有机，如水滴般饱满，所有转角大幅圆角处理。高级玻璃拟态 UI 设计，高度精细的 3D 渲染。

**English prompt:**
A 3D fast file transfer icon, pure black background, 256x256. Design as a rounded diamond shape with speed arc trails. Rendered entirely in liquid glass material. The icon is a 3D glass sculpture with frosted translucency, bright edge highlights, and internal grey/white light refractions. Overall shape is soft and organic, plump like a water droplet, all corners heavily rounded. Premium glassmorphism UI design, highly detailed 3D render.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 3. plug — 串口连接 / 插件

**中文提示词：**
一个液态玻璃风格的电插头图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。插头造型极简几何化但保持圆润 — 插脚为两个圆角矩形从圆润的主体中伸出，主体右侧连接一段弧线电缆。所有线条端点圆润，转角大圆角。风格参考：磨砂玻璃质感、液态胶囊描边。

**English prompt:**
A liquid glass style electric plug icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. Plug shape is minimal geometric but soft — two rounded prongs extending from a rounded body, with an arc cable on the right side. All stroke ends rounded, all corners heavily filleted. Style reference: frosted glass texture, liquid pill strokes.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 4. pin — 位置/端口

**中文提示词：**
一个液态玻璃风格的定位图钉图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。图钉顶部为饱满的圆角水滴形/椭圆，底部收窄为柔和的尖端。整体造型如一颗液态金属水滴，无任何直线段，全部为有机曲线。风格参考：磨砂玻璃质感、液态胶囊描边、水滴形态。

**English prompt:**
A liquid glass style location pin icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. The pin has a plump rounded teardrop/oval top that tapers to a soft point at the bottom. Overall shape like a liquid metal droplet, no straight lines, all organic curves. Style reference: frosted glass texture, liquid pill strokes, droplet form.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 5. tag — 标签/名称

**中文提示词：**
一个液态玻璃风格的标签图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。标签主体为圆角矩形，左侧有一个小圆孔（挂绳孔），整体向右倾斜约10度，营造自然摆放感。所有转角大圆角，线条粗壮均匀。风格参考：磨砂玻璃质感、液态胶囊描边。

**English prompt:**
A liquid glass style tag/label icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. The tag body is a rounded rectangle with a small circular hole on the left (string hole), tilted about 10 degrees to the right for a natural resting feel. All corners heavily rounded, strokes thick and even. Style reference: frosted glass texture, liquid pill strokes.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 6. settings — 设置齿轮

**中文提示词：**
一个液态玻璃风格的齿轮设置图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。齿轮中心为实心小圆，外围均匀分布6-8个圆角齿状凸起。齿形短而圆润（不像机械齿轮尖锐），每个齿的顶端和根部均为大圆角。整体如一个柔软的太阳/花朵齿轮混合造型。风格参考：磨砂玻璃质感、液态胶囊描边。

**English prompt:**
A liquid glass style gear/settings icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. The gear has a small solid circle center with 6-8 evenly distributed rounded teeth around it. Teeth are short and plump (not sharp like mechanical gears), each tooth tip and root heavily rounded. Overall looks like a soft sun/flower gear hybrid. Style reference: frosted glass texture, liquid pill strokes.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 7. palette — 外观/主题

**中文提示词：**
一个液态玻璃风格的调色板图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。调色板主体为圆角椭圆/盾形，左上角有一个小拇指孔（圆角矩形镂空）。调色板边缘均匀分布3-4个小圆点（颜料点）。所有形状圆润柔软，无尖锐边缘。风格参考：磨砂玻璃质感、液态胶囊描边。

**English prompt:**
A liquid glass style artist palette icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. The palette body is a rounded oval/shield shape with a small thumb hole (rounded rectangle cutout) in the upper left. 3-4 small rounded dots (paint spots) evenly distributed along the edge. All shapes soft and pillowy, no sharp edges. Style reference: frosted glass texture, liquid pill strokes.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 8. globe — 网络/语言

**中文提示词：**
一个液态玻璃风格的地球/网络图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。正圆形为主体，内部有3条水平弧线（纬度线，如玻璃管弯曲）和1条垂直弧线（经度线），交叉处自然融合。所有线条粗壮圆润，无锐角交叉。风格参考：磨砂玻璃质感、液态胶囊描边、简洁地球仪。

**English prompt:**
A liquid glass style globe/network icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A perfect circle body with 3 horizontal arcs (latitude lines, curved like glass tubes) and 1 vertical arc (longitude line) inside, intersections blend naturally. All strokes thick and rounded, no sharp intersections. Style reference: frosted glass texture, liquid pill strokes, minimal globe.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 9. font — 编码/文本

**中文提示词：**
一个液态玻璃风格的文字/字体图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。设计为 3 条长度递减的圆角水平胶囊（象征文本行），间距均匀，底部对齐。备选方案：设计为大写字母"A"的变体 — 两条斜线在下半部分以流畅的圆弧连接，中间横线也是一个柔和的弧线。字母形态保持几何感但转角全部圆润。风格参考：磨砂玻璃质感、液态胶囊描边、排版图标。

**English prompt:**
A liquid glass style text/font icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. Design as 3 rounded horizontal capsules of decreasing length (representing text lines), evenly spaced, bottom-aligned. Alternative: a stylized uppercase "A" — two diagonal strokes connected by a smooth arc at the bottom, the crossbar is a gentle arc. Letterform keeps geometric feel but all corners are rounded. Style reference: frosted glass texture, liquid pill strokes, typography icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 10. info — 信息提示

**中文提示词：**
一个液态玻璃风格的信息图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。正圆形外框（描边），内部为小写字母"i"的柔软变体 — 上方为一个圆润的小圆点，下方为一根垂直胶囊形竖线。圆形外框和内部元素全部使用粗描边和圆角。风格参考：磨砂玻璃质感、液态胶囊描边、信息提示符号。

**English prompt:**
A liquid glass style info icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A perfect circle outline (stroked), inside is a soft variant of lowercase "i" — a small rounded dot above and a vertical pill-shaped bar below. Both the circle and internal elements use thick strokes and rounded terminals. Style reference: frosted glass texture, liquid pill strokes, info symbol.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 11. search — 搜索

**中文提示词：**
一个液态玻璃风格的搜索/放大镜图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。正圆形镜头（描边），右下角连接一个45°的圆润手柄。圆形与手柄连接处自然过渡，手柄末端为饱满的半圆形。镜头内无额外细节，保持简洁。风格参考：磨砂玻璃质感、液态胶囊描边。

**English prompt:**
A liquid glass style search/magnifying glass icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A perfect circle lens (stroked) with a 45° rounded handle extending from the lower right. The circle-to-handle junction transitions naturally, handle ends in a plump half-circle. No extra detail inside the lens, keep it clean. Style reference: frosted glass texture, liquid pill strokes.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 12. upload — 发送/上传

**中文提示词：**
一个液态玻璃风格的上传/发送图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。圆角矩形外框（描边），内部中心是一个向上指的粗壮圆角箭头。箭头杆为垂直胶囊形，箭头顶部为圆润的V形/人字形。整体对称、有力、积极向上。风格参考：磨砂玻璃质感、液态胶囊描边、发送符号。

**English prompt:**
A liquid glass style upload/send icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A rounded rectangle outline (stroked) with a thick upward-pointing rounded arrow at center. The arrow shaft is a vertical capsule shape, the arrowhead is a rounded chevron/V at top. Overall symmetric, bold, upward energy. Style reference: frosted glass texture, liquid pill strokes, send symbol.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 13. download — 接收/下载

**中文提示词：**
一个液态玻璃风格的下载/接收图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。与 upload 图标对称 — 圆角矩形外框（描边），内部中心是一个向下指的粗壮圆角箭头。箭头杆为垂直胶囊形，箭头底部为圆润的倒V形。风格参考：磨砂玻璃质感、液态胶囊描边、接收符号。

**English prompt:**
A liquid glass style download/receive icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. Symmetric to the upload icon — a rounded rectangle outline (stroked) with a thick downward-pointing rounded arrow at center. Arrow shaft is a vertical capsule, arrowhead is a rounded inverted chevron at bottom. Style reference: frosted glass texture, liquid pill strokes, receive symbol.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 14. package — YModem 协议/数据包

**中文提示词：**
一个液态玻璃风格的包裹/数据包图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。3D等距视角的圆角立方体（所有棱角大圆角），或者2D正视图的圆角方形盒子，顶部有一个柔和的弧线盖子。所有面使用粗描边和圆角连接。风格参考：磨砂玻璃质感、液态胶囊描边、包裹图标。

**English prompt:**
A liquid glass style package/archive icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A 3D isometric rounded cube (all edges heavily rounded), or a 2D front-view rounded square box with a soft arc lid on top. All faces use thick strokes with rounded connections. Style reference: frosted glass texture, liquid pill strokes, package icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 15. antenna — XModem 协议/通信

**中文提示词：**
一个液态玻璃风格的天线/卫星通信图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。中心为一个圆润的竖直塔杆（垂直胶囊形），顶部展开为3-4条向上弯曲的弧形信号波（如同心圆弧段，越远越宽）。信号波线条粗壮，弧度优美，与塔杆自然融合。风格参考：磨砂玻璃质感、液态胶囊描边、通信信号图标。

**English prompt:**
A liquid glass style antenna/satellite communication icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A rounded vertical pole (vertical capsule) at center, with 3-4 upward-curving arc signal waves expanding from the top (concentric arc segments, wider as they go out). Signal wave strokes are thick with elegant curves, blending naturally with the pole. Style reference: frosted glass texture, liquid pill strokes, signal icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 16. trash — 删除

**中文提示词：**
一个液态玻璃风格的垃圾桶/删除图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。圆角梯形/桶形主体（上宽下略窄），顶部有一条略宽的圆角横条（桶盖/把手）。桶身内部可选2-3条垂直圆角细线（象征纹理）。所有元素粗描边、大圆角。风格参考：磨砂玻璃质感、液态胶囊描边、删除图标。

**English prompt:**
A liquid glass style trash can/delete icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A rounded trapezoid/bucket body (wider at top, slightly narrower at bottom), with a slightly wider rounded horizontal bar at top (lid/handle). Optionally 2-3 vertical rounded thin lines inside the body (texture). All elements thick strokes, heavily rounded. Style reference: frosted glass texture, liquid pill strokes, delete icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 17. stop — 停止/断开

**中文提示词：**
一个液态玻璃风格的停止/方块图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。圆角正方形（四角大圆角），内部填充为实心（与 play 的三角空心形成对比）。四角弧度对称一致，整体饱满有力。风格参考：磨砂玻璃质感、液态胶囊描边、停止按钮。

**English prompt:**
A liquid glass style stop/square icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A rounded square (all four corners heavily rounded), filled solid (contrasting with the play triangle outline). Corner radii symmetric and consistent, overall plump and bold. Style reference: frosted glass texture, liquid pill strokes, stop button.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 18. play — 播放/连接

**中文提示词：**
一个液态玻璃风格的播放/三角图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。向右指向的圆角三角形（实心填充），三个顶点全部为大圆角。三角形的三条边带轻微向外弧度（非直线），营造有机流体感。风格参考：磨砂玻璃质感、液态胶囊描边、播放按钮。

**English prompt:**
A liquid glass style play/triangle icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A right-pointing rounded triangle (solid fill), all three vertices heavily rounded. The three sides have subtle outward curvature (not straight lines), creating an organic fluid feel. Style reference: frosted glass texture, liquid pill strokes, play button.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 19. construction — 建设中/未实现

**中文提示词：**
一个液态玻璃风格的施工/建设中图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。菱形/钻石形外框（圆角），内部为扳手与锤子的简化交叉造型 — 扳手为弧线+圆角头部，锤子为圆角矩形+饱满圆形锤头。所有元素粗描边、大圆角。风格参考：磨砂玻璃质感、液态胶囊描边、建设中符号。

**English prompt:**
A liquid glass style construction/under-development icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A diamond/rhombus outline (rounded corners), inside is a simplified crossed wrench and hammer — wrench is an arc with rounded head, hammer is a rounded rectangle handle with a plump circular head. All elements thick strokes, heavily rounded. Style reference: frosted glass texture, liquid pill strokes, construction symbol.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 20. folder — 文件夹/本地文件

**中文提示词：**
一个液态玻璃风格的文件夹图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。传统的文件夹造型 — 主体为大圆角矩形，顶部有一个小圆角矩形标签凸起。文件夹正面和标签之间的折线使用柔和的弧线过渡而非直角。所有边角大圆角。风格参考：磨砂玻璃质感、液态胶囊描边、文件管理图标。

**English prompt:**
A liquid glass style folder icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. Classic folder shape — body is a large rounded rectangle with a small rounded rectangular tab protruding from the top. The fold line between the front face and tab uses a soft arc transition instead of a sharp angle. All edges heavily rounded. Style reference: frosted glass texture, liquid pill strokes, file manager icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 21. chart — 仪表盘/图表

**中文提示词：**
一个液态玻璃风格的柱状图/仪表盘图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。3根高度递增的垂直圆角柱形（胶囊形），底部对齐在一条圆角横线上。柱子间距均匀，每根柱子顶部为饱满的半圆形。风格参考：磨砂玻璃质感、液态胶囊描边、数据可视化图标。

**English prompt:**
A liquid glass style bar chart/dashboard icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. 3 vertical rounded bars (capsule shapes) of increasing height, aligned at the bottom on a rounded horizontal baseline. Bars are evenly spaced, each bar top is a plump half-circle. Style reference: frosted glass texture, liquid pill strokes, data visualization icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 22. warning — 警告/注意

**中文提示词：**
一个液态玻璃风格的警告三角图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。圆角等边三角形（描边），三个顶点全部为大圆角。内部中心为一个大圆角感叹号 — 上方竖线为垂直胶囊形，下方圆点为饱满的小圆形。三角形和感叹号均使用粗描边。风格参考：磨砂玻璃质感、液态胶囊描边、警告符号。

**English prompt:**
A liquid glass style warning triangle icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A rounded equilateral triangle (stroked), all three vertices heavily rounded. Inside at center is a large rounded exclamation mark — upper bar is a vertical capsule, lower dot is a plump small circle. Both triangle and exclamation use thick strokes. Style reference: frosted glass texture, liquid pill strokes, warning symbol.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 23. stopwatch — 计时/运行时间

**中文提示词：**
一个液态玻璃风格的秒表/计时器图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。正圆形表盘（描边），内部有钟面刻度 — 顶部12点位置有两个小圆点，中心有一个小圆点。一根从中心指向右上约2点钟方向的粗圆角指针。表盘顶部有一个圆角小按钮/表冠。风格参考：磨砂玻璃质感、液态胶囊描边、计时器图标。

**English prompt:**
A liquid glass style stopwatch/timer icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A perfect circle dial (stroked) with clock face markings inside — two small dots at the 12 o'clock position, a small dot at center. One thick rounded hand pointing from center to about 2 o'clock. A small rounded button/crown on top of the dial. Style reference: frosted glass texture, liquid pill strokes, timer icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 24. check-circle — 完成/成功

**中文提示词：**
一个液态玻璃风格的圆形完成/成功图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。正圆形外框（粗描边），内部为一个大圆角勾选标记 — 左侧短斜线和右侧长斜线以流畅的圆弧连接，勾选末端为饱满的半圆形。圆形和勾选标记线条粗壮均匀。风格参考：磨砂玻璃质感、液态胶囊描边、成功确认图标。

**English prompt:**
A liquid glass style circle check/success icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A perfect circle outline (thick stroke) with a large rounded checkmark inside — a short left diagonal and a long right diagonal connected by a smooth arc, the checkmark tip ends in a plump half-circle. Both circle and checkmark strokes are thick and even. Style reference: frosted glass texture, liquid pill strokes, success confirmation icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 25. cross-circle — 失败/错误

**中文提示词：**
一个液态玻璃风格的圆形失败/错误图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。正圆形外框（粗描边），内部为两条45°交叉的圆角粗线（X形），每条线的四个端点均为饱满的半圆形。交叉处自然融合。圆形和X线线条粗壮均匀。风格参考：磨砂玻璃质感、液态胶囊描边、错误图标。

**English prompt:**
A liquid glass style circle cross/error icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A perfect circle outline (thick stroke) with two 45° crossed rounded thick lines inside (X shape), all four endpoints of each line are plump half-circles. The intersection blends naturally. Both circle and X strokes are thick and even. Style reference: frosted glass texture, liquid pill strokes, error icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 26. skip — 跳过

**中文提示词：**
一个液态玻璃风格的跳过/快进图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。两个向右指向的并列圆角三角形（实心填充），后跟一条垂直圆角竖线。三角形顶点全部大圆角，竖线两端为半圆形。整体节奏感强，象征"跳过"。风格参考：磨砂玻璃质感、液态胶囊描边、媒体控制图标。

**English prompt:**
A liquid glass style skip/fast-forward icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. Two side-by-side right-pointing rounded triangles (solid fill) followed by a vertical rounded bar. All triangle vertices heavily rounded, bar ends are half-circles. Strong rhythmic feel, symbolizing "skip". Style reference: frosted glass texture, liquid pill strokes, media control icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 27. hourglass — 等待/处理中

**中文提示词：**
一个液态玻璃风格的沙漏/等待图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。上下两个圆角三角形（或梯形）在中间以细窄的通道相连，形成经典的沙漏造型。所有顶点和转角大圆角处理。内部中空（轮廓描边）。风格参考：磨砂玻璃质感、液态胶囊描边、等待状态图标。

**English prompt:**
A liquid glass style hourglass/waiting icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. Two rounded triangles (or trapezoids) connected by a narrow channel in the middle, forming the classic hourglass shape. All vertices and corners heavily rounded. Hollow inside (outline strokes). Style reference: frosted glass texture, liquid pill strokes, waiting state icon.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 28. transfer-progress — 传输进行中

**中文提示词：**
一个液态玻璃风格的传输/同步进行中图标，3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。正圆形外框，内部不放置居中单一箭头，而是两个沿圆形路径弯曲的粗壮圆角箭头，呈首尾追逐的环形排列 — 每个箭头由弧线箭身和圆润箭头组成，线条粗壮均匀，端点全部为饱满的半圆形。箭头环绕圆心约 270°，营造连续运动/旋转的视觉动感。风格参考：磨砂玻璃质感、液态胶囊描边、同步/加载动画图标、传输进行中状态指示器。

**English prompt:**
A liquid glass style transfer/sync in-progress icon, 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. A perfect circle outline. Inside — instead of a single centered arrow — two thick rounded arrows curved along a circular path, arranged head-to-tail in a chasing ring. Each arrow has a curved shaft and a rounded arrowhead, strokes thick and even, all terminals are plump half-circles. The arrows wrap approximately 270° around the center, conveying a sense of continuous motion / rotation. Style reference: frosted glass texture, liquid pill strokes, sync/loading animation icon, transfer in-progress status indicator.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

### 29. check-plain — 选中/已完成标记

**中文提示词：**
一个液态玻璃风格的纯勾选标记图标（无外框圆圈），3D 玻璃雕刻，纯黑背景，磨砂半透明液态玻璃材质，明亮边缘高光与折射，256×256像素。与 check-circle 的勾选标记造型一致 — 左侧短斜线和右侧长斜线以流畅的圆弧连接，勾选末端为饱满的半圆形。线条粗壮均匀，造型自信有力。风格参考：磨砂玻璃质感、液态胶囊描边、选中确认符号。

**English prompt:**
A liquid glass style plain checkmark icon (no outer circle), 3D glass sculpture on pure black background, rendered in frosted liquid glass with bright edge highlights and refractions, 256×256 pixels. Same checkmark shape as check-circle — a short left diagonal and a long right diagonal connected by a smooth arc, the tip ends in a plump half-circle. Thick even strokes, confident and bold form. Style reference: frosted glass texture, liquid pill strokes, selection confirmation symbol.

**技术参数：** 256×256, frosted glass, black bg, PNG-24

---

## 生成后检查清单 / Post-Generation Checklist

### 图标质量

- [ ] 所有 29 个图标风格统一（描边粗细、圆角程度、视觉密度）
- [ ] 每个图标在缩小到 18×18 时仍然清晰可辨
- [ ] 图标为半透明液态玻璃材质渲染，边缘高光锐利明亮，内部折射自然通透（非纯白剪影，非杂色噪点）
- [ ] 图标在暗色背景上清晰可见（半透明玻璃需要暗色底色衬托）；纯黑背景便于后期抠图或使用屏幕混合模式叠加到 UI
- [ ] 文件名与上述图标名一致（logo.png, zmodem.png, ...）

### 部署

1. 将所有 29 个 PNG 图标放入 `src/assets/icons/` 目录，覆盖占位文件
2. 运行部署脚本，一键将 logo 同步到 App 图标位置：

```bash
node scripts/apply-logo.mjs
```

该脚本自动完成：
- 复制 `logo.png` → `src-tauri/icons/icon.png`（窗口/任务栏图标）
- 生成 `icon.ico`（Windows 编译所需的 ICO 格式）

3. 浏览器标签页图标（Favicon）无需单独部署：
   - `index.html` 通过 `<link rel="icon" href="/favicon.png" />` 声明
   - Vite 开发服务器中间件将 `/favicon.png` 实时映射到 `src/assets/icons/logo.png`
   - 生产构建时 `vite.config.ts` 中的 `closeBundle` 钩子自动复制 `logo.png` → `dist/favicon.png`
   - **不再需要** `public/` 目录

4. 运行 `npm run tauri dev` 验证所有图标正常显示
