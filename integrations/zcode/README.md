# ZCode 零补丁原生适配说明

Micro Input 直接依据 ZCode（`dev.zcode.app`，`zcode`）开源代码与 macOS Accessibility 树进行适配，不再向 ZCode 维护或注入任何第三方 `.patch` 补丁。

## 开源行为契约与键位对齐

1. **原生快捷键（`packages/shared/src/shortcutCommands.ts`）**：
   - 新建任务（`new-chat` / `newTask`）：`Cmd+N`
   - 命令中心/搜索（`search` / `openCommandCenter`）：`Cmd+K`
   - 打开工作区/项目（`project` / `openWorkspace`）：`Cmd+O`
   - 上一会话（`prev-conversation` / `previousConversation`）：`Cmd+Shift+[`
   - 下一会话（`next-conversation` / `nextConversation`）：`Cmd+Shift+]`
   - 模型菜单（`model` / `openModelMenu`）：`Ctrl+M`
   - 切换思考档位（`effort` / `cycleThoughtLevel`）：`Ctrl+T`
   - 切换终端（`terminal` / `toggleTerminal`）：`Cmd+J`
   - 切换侧栏面板（`browser` / `reveal-tools` / `toggleSidePane`）：`Cmd+Option+B`
   - 发送消息（`submit` / `composerSend`）：聚焦输入框后按 `Return`
2. **侧边栏项目与会话识别（`ProjectNodeContent.tsx` & `TaskListItem.tsx`）**：
   - 项目行：`AXDOMClassList` 含 `group/project-row` 的节点及其切换按钮。
   - 会话/任务行：`AXDOMClassList` 含 `group/task-item`（当前选中项含 `bg-selected`）。
   - 状态指示：
     - Working（运行中）：子节点 `AXDOMClassList` 含 `animate-spin`。
     - Completed（已完成蓝点）：子节点 `AXDOMClassList` 含 `bg-icon-blue`。
