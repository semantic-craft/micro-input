# Micro Input

面向 Work Louder Creator Micro 2 的独立 macOS 配置器与宿主服务。技术方向：**Rust 核心 + SwiftUI 界面**。项目名为工作名。

## 当前开发状态

首轮原生离线工作台：Rust五核心模块 + UniFFI + SwiftUI。支持配置浏览、动作搜索/分类、结构化动作参数、Profile/Layer新建/复制/改名/排序/删除、本地普通宏按下/释放/延时步骤编辑与共享引用管理、四分支 Multi 的本地时序与共享动作引用编辑、本地草稿持久化、结构与动作差异及整次撤销/重做。动作库按名称、分类和目标搜索，收藏作为独立本机偏好保存；动作条目显示目标、前提与执行不可用原因，应用意图通过系统选择器选择目标。主窗和菜单栏共享状态；关闭主窗保留菜单栏，退出结束会话。View菜单可选择深浅色和紧凑窗口；⇧⌘M打开菜单栏面板，⌘E搜索动作，⌘[/⌘]选择控件，⌘D查看差异。

```sh
scripts/check          # Rust/Swift测试（含真实UniFFI合成发现）、桥接生成、开发构建和协议边界检查
scripts/run            # 启动当前checkout的target/MicroInput.app
scripts/run --verify   # 独立QA标识/文档，10分钟自动退出并回收副本
```

需要macOS14+、Rust stable、Swift工具链。构建不加载凭据。仅本地ad-hoc开发签名，未正式发布。

启动载入所选本地草稿，无文件时载入合成配置；点击“查找 / 刷新 USB”后只读枚举 USB HID 描述符并持续监听插拔，不会打开硬件或申请权限。候选经完整 usage pairs 与 OS USB 来源筛选，多候选必须明确选择；0x8297 标为未验型号，固件始终显示未知。插拔事件更新候选，选择不读取或应用设备配置。本地文档默认在用户Application Support/MicroInput/workbench.json；v1浏览不改文件，首次修改先备份原字节再保存v2。设备快照保持只读，点击“从快照新建草稿”才保存独立draft-UUID.json；原工作台保留，所选文档记录带锁/冲突检查，退出重开继续新草稿。参数表单需点击保存，撤销历史仅在当前会话。无法执行动作或写入设备。支持显式USB身份查询，当前303A:8298/固件0.6.3已在应用路径读回；显式身份为受支持的0.6.3后可读取两份限定配置文件，按块校验原始SHA-1并再次list，再一次发布只读快照。查看设备不会改变本地草稿，返回后保留编辑与撤销。其他固件仍拒绝读取，不把演示或导入数据作为设备快照。

旋钮三个逻辑槽位可独立分配本地动作；摇杆可选择保留来源模式或编辑本地逻辑扇区。减少活动扇区或切换模式不会删除旧槽位及绑定，未知物理方向继续标识；0–32 是本机编辑器限制，不是设备容量。保存、撤销/重做和重开沿用本地文档服务。见[控件编辑契约](docs/implementation-plan.md#t10-本地控件编辑契约)。

“导入与导出”支持整文档、当前配置或当前层的自有 JSON 预设，以及原创 Coding 起步预设。导入先预览，再创建独立草稿；原文档保留，本地对象 ID 重映射，历史设备基线不作为当前设备事实。为保留未知原值，导出仍包含完整来源档案，保存前会提示；不宣称兼容厂商格式。见[预设交换契约](docs/implementation-plan.md#t11-自有预设交换契约)。

规格见 [SPEC](docs/spec/SPEC.md)、分阶段验收见 [BUILD_PROMPT](docs/spec/BUILD_PROMPT.md)、[证据](docs/spec/EVIDENCE.md)和[未知项](docs/spec/UNKNOWNS.md)。

## 调研入口

Input 1.1.0 的[业务架构与证据定位](docs/input-architecture.md)、[写入和宿主通知候选协议](docs/protocol-candidates.md)、[独立实现模块契约](docs/module-contracts.md)已基于当前安装包整理。该轮为静态分析，未新增设备操作或运行时插桩验收；原始材料仍保留在 `.scratch/input-study/`。

可直接用于只读客户端实现的结论见 [USB 发现、身份与配置读取协议](docs/protocol-readonly.md)。依据 Input 1.1.0 静态分析及本机 IOKit 枚举；独立IOKit探针曾读回0.6.3；应用身份路径已读回0.6.3；当前0.6.3的配置读取及原生只读浏览已有E-079验收；完整生命周期和物理映射仍待验收。

以下为本机原始调查材料，保存在 Git 忽略的 `.scratch/input-study/`，不随源码提交：

- [官方来源、能力矩阵与公开协议线索](.scratch/input-study/out/research.md)
- [已连接 UI 清单](.scratch/input-study/out/connected-ui-inventory.md)
- [灯光、摇杆、层、AppSense 与参数实验](.scratch/input-study/out/layer-lighting-joystick-followup.md)
- [观察规格](.scratch/input-study/out/spec.json)和[证据记录](.scratch/input-study/out/evidence.jsonl)
- [原任务历史](.scratch/input-study/task-history.md)

旧入口 `~/agent-tasks/worklouder-input-study` 仅保留晋升记录和兼容链接。后续项目事实、代码和进度只在本项目维护。

T05已补取消、会话终止通知及退出时等待资源释放；当前设备两次独立身份查询→关闭→重开已通过。配置读取中物理拔插、睡眠和原有映射行为仍待实际验收，不能据此签收完整生命周期。见[E-082](docs/spec/EVIDENCE.md)。

## 下一步

PR #12/#13 已交付原生离线工作台、结构化动作和本地持久化。当前已有设备发现、身份查询与[#5 配置读取](https://github.com/semantic-craft/micro-input/issues/5)实现；配置原生浏览验收见[E-079](docs/spec/EVIDENCE.md)，[#6 生命周期](https://github.com/semantic-craft/micro-input/issues/6)及未核实物理映射仍待验证。范围继续限定只读，不从核心读回推定全部S1完成。

完整目标与 35 张工单入口为 [#44](https://github.com/semantic-craft/micro-input/issues/44)，技术契约见 [实施设计](docs/implementation-plan.md)，阶段验收见 [BUILD_PROMPT](docs/spec/BUILD_PROMPT.md)。当前按继续开发授权补齐本地功能与验证；设备能力仍遵守各票证据与恢复门槛，不接自动化、不写设备、不执行宿主动作。未完成技术细化的工单使用 needs-info，并在工单内列明缺项。T10 本地控件编辑已接入集成分支并通过离线与原生 QA；T11 自有预设已实现，下一步核对 T09 / T12 在途分支的交付和剩余验收。工单关闭与产品总验收仍以 GitHub 当前状态为准。

原生验收使用 `scripts/run --verify` 输出的 `QA_APP_PATH` 精确选择应用；QA 副本有唯一 bundle ID，避免多个 worktree 的同名应用互相抢占。退出后通过进程状态确认，不再调用可能自动重启应用的 UI 入口。QA 身份不能替代生产签名与权限验收。
