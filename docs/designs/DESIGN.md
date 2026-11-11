# Micro Input Design System (`DESIGN.md`)

面向人类维护者与 Coding Agent 的统一界面规范。设计语言对齐 [`openai/apps-sdk-ui`](https://github.com/openai/apps-sdk-ui)、[`shadcn-ui/ui`](https://github.com/shadcn-ui/ui) 与 [`VoltAgent/awesome-design-md`](https://github.com/VoltAgent/awesome-design-md) 的单色中性体系。任何修改 `app/MicroInput/Sources/MicroInputUI/*.swift` 的任务均须遵守本文件与 `scripts/check-boundaries.py` 的 `R-C06`、`R-C07`、`R-C09` 门禁。

## 1. 三大宏观原则

1. **清晰的视觉层级 (Visual Hierarchy)**
   - 每个区域或弹窗最多允许 **1 个** `Primary` 近黑实心主按钮；常规动作用 `Secondary` 白底细边胶囊按钮；重置、取消、折叠等辅助动作一律退居 `Ghost` 无框悬停态。
   - 严禁同一屏幕出现功能完全重复的按钮。
2. **极低视觉噪音与高信噪比 (Low Visual Noise)**
   - **单色外壳铁律**：所有按钮、胶囊下拉触发器、弹出菜单卡片、开关轨道、滑块圆点统一使用黑白灰中性色阶。
   - **彩色只归真实信号**：颜色仅用于硬件连接状态字（如已连接/已授权绿字）、实时会话状态圆点（进行中绿点、已完成蓝点、异常红点）与键帽背光预览。严禁在常规按钮、复选框或下拉箭头上使用系统亮蓝或高饱和紫色块。
   - 彻底禁用 `.controlSize(.mini)`、`.buttonStyle(.borderedProminent)`、`.buttonStyle(.bordered)` 以及 macOS 默认双箭头 `Picker` 下拉条。
3. **心智模型与空间映射对齐 (Mental Model & Spatial Mapping)**
   - **表单亲密性与对齐**：关联设置项必须收进 `SettingsCard` 圆角卡片，行内左放标题与副标题，右侧控件统一贴右边缘对齐，不再紧贴文字末尾参差排列。
   - **硬件 1:1 空间映射**：工作台键盘画布与「按键速查」浮层必须严格按 Creator Micro 真实 4×4 物理拓扑排列（第 0 行旋钮 + 双键 + 摇杆，第 1–2 行各四键，第 3 行状态灯 + 1U/2U 底键 + 右下键），禁止排成失去空间坐标的普通表格。

## 2. Design Tokens (`UITheme`)

| Token | 浅色模式 (Light) | 深色模式 (Dark) | 用途 |
| --- | --- | --- | --- |
| `pageBackground` | `#FAFAFA` | `#18181A` | 页面画布底层背景 |
| `surface` | `#FFFFFF` | `#222225` | `SettingsCard`、胶囊触发器、浮动下拉卡片表面 |
| `subtleSurface` | `#F4F4F5` | `#2A2A2E` | 侧栏底色、次级内嵌块 |
| `controlHoverFill` | `#F1F1F3` | `#323236` | 胶囊下拉悬停/展开态、菜单行悬停底色 |
| `controlPrimaryFill` | `#171717` | `#F4F4F5` | `Primary` 按钮、开启态开关轨道、滑块圆点 |
| `controlPrimaryText` | `#FFFFFF` | `#141416` | `Primary` 按钮文字 |
| `hairlineBorder` | `rgba(0,0,0,0.08)` | `rgba(255,255,255,0.10)` | 卡片、胶囊按钮、下拉浮层 `1pt` 描边 |
| `rowDivider` | `rgba(0,0,0,0.05)` | `rgba(255,255,255,0.07)` | `SettingsCard` 行间内缩分割线 |
| `textPrimary` | `#171717` | `#F4F4F5` | 正文、行标题、触发器当前值 |
| `textSecondary` | `#71717A` | `#A1A1AA` | 行副标题、下拉选项说明、滑块百分比 |
| `statusSuccess` | `#16A34A` | `#4ADE80` | 已连接、权限已就绪等真实正状态信号 |

- **字号体系**：页面大标题 `26pt Semibold`；区段标题 `15pt Semibold`；卡片行标题与选项主标题 `14pt Medium`；副标题、胶囊按钮文字 `13pt Regular/Medium`；等宽快捷键徽章 `12pt Monospaced`。
- **圆角体系**：`SettingsCard` 与浮动下拉菜单卡片 `16pt` 连续圆角；胶囊下拉触发器与常规胶囊按钮 `Capsule()`；弹出菜单内部选项高亮行 `10pt` 连续圆角；菜单栏弹窗动作行 `8pt` 连续圆角。

## 3. 标准组件契约

1. **`SettingsCard` & `SettingsRow`**：外框 `16pt` 连续圆角 + `1pt hairlineBorder`；行内横向边距 `20pt`、纵向边距 `14pt`；行间分割线左右各内缩 `20pt`。
2. **`SelectDropdown<Value>` + `SelectOption<Value>`**：
   - 闭合态：`Capsule()` 白底 + `1pt hairlineBorder`，悬停或展开时切换为 `controlHoverFill`；文字 `13pt` 居左，右侧 `10pt chevron.down` 单箭头，自适应紧凑宽度，绝不拉伸占满整行。
   - 展开态：锚定在触发器正下方（右边缘对齐，自动贴合屏幕可视边界），使用无三角箭头的 `16pt` 圆角纯白浮层卡片 + 柔和环境阴影（`opacity 0.10, radius 18, y 8`）。同时支持单行纯文本选项与「主标题 + 灰色副标题」双行语义选项；点击外部或按 `ESC` 立即关闭。
3. **`SurfaceButtonStyle`**：
   - `.primary`：`controlPrimaryFill` 实心胶囊/圆角按钮。
   - `.secondary`：`surface` 白底 + `1pt hairlineBorder` 胶囊按钮，悬停变 `controlHoverFill`。
   - `.ghost`：静止无边框、次级灰字，悬停浮现 `controlHoverFill` 圆角底并加深字色。
4. **`NeutralSwitchToggleStyle`**：`40 × 24pt` 胶囊轨道，关闭态 `#E4E4E7` 浅灰底配 `20pt` 白色圆滑块，开启态 `#171717` 近黑底配白色圆滑块。
5. **`NeutralTrackSlider`**：`3pt` 细胶囊轨道配 `18pt` 纯黑实心圆滑块，右侧固定宽度等宽数字百分比。
6. **`PopoverActionRow`**：菜单栏弹窗底部动作统一使用整行悬停条目（左侧线性 SF Symbol + 标题 + 右侧等宽快捷键提示），禁止堆砌厚按钮。

## 4. 自动化可验证门禁

- **静态门禁 (`R-C09`)**：`scripts/check-boundaries.py` 扫描 `app/MicroInput/Sources/MicroInputUI/*.swift`，禁止出现 `.controlSize(.mini)`、`.buttonStyle(.borderedProminent)`、`.buttonStyle(.bordered)` 及非 `.segmented` 的裸 `Picker(`，并强制要求所有 `.buttonStyle(.plain)` / `.onTapGesture` 附带 `.contentShape(Rectangle())` (`R-C07`) 与 `.focusEffectDisabled()`（消除侧栏粗蓝框）。
- **几何与快照门禁 (`DesignSystemVerificationTests`)**：在 `swift test` 中实测校验胶囊下拉触发器紧凑宽度（严禁拉成全宽长条）、多行 `SettingsRow` 右侧控件对齐、单双行下拉菜单完整包裹不截断、820pt 最小窗宽顶栏零裁切、以及速查浮层 4×4 空间映射完整性，并支持 `MI_LAYOUT_OUTPUT` 导出 PNG 视觉回执。
