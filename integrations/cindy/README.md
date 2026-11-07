# Cindy 零补丁原生适配说明

Micro Input 直接依据 Cindy（`com.xd.cindy`）开源代码与运行期可观测行为进行适配，不再向 Cindy 维护或注入任何第三方 `.patch` 补丁。

## 开源行为契约与键位对齐

1. **原生键盘所有权（`apps/desktop/src/shared/inputDevices.ts`）**：
   - Cindy 原生支持 Work Louder Creator Micro 2 的 `KV_OAI_*` 保护层。
   - 当 `cindy-ownership.sock` 不存在时，`NativeOwnershipController.isUnbridgedDesktopActive` 自动放行本地桌面控制。
2. **原生快捷键（`apps/desktop/src/shared/appShortcuts.ts`）**：
   - 新建任务（`new-chat` / `new-maker`）：`Cmd+N`
   - 搜索（`search` / `find-in-page`）：`Cmd+F`
   - 切换权限/思考模式（`effort` / `cycle-permission-mode`）：`Shift+Tab`
   - 打开终端（`terminal` / `open-terminal`）：`Ctrl+\``
   - 切换第 1..9 个会话：`Cmd+1..9`
3. **侧边栏会话与状态识别（`SessionItem.tsx` & `SidebarRightStatusIndicator.tsx`）**：
   - 会话行：`role="button"`，位于左侧栏（`x < 300`），`AXDOMClassList` 包含 `rounded-full` 与 `h-8`。
   - 状态指示器（`role="img"` / `AXImage` 的 `aria-label`）：
     - Working（运行中）：`运行中` / `Running`
     - Completed（已完成未读）：`已完成 — 点击查看` / `Completed — click to view`
     - Attention（需关注）：`等待回复` / `Awaiting your input` / `出错 — 点击查看` / `Failed — click to view`
