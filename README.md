# Micro Input

面向 Work Louder Creator Micro 2 的独立 macOS 配置器与宿主服务。技术方向：**Rust 核心 + SwiftUI 界面**。项目名为工作名。

## 当前开发状态

首轮原生离线工作台：Rust五核心模块 + UniFFI + SwiftUI。支持合成配置浏览、动作搜索/分类、结构化动作参数、本地草稿持久化、差异及撤销/重做。主窗和菜单栏共享状态；关闭主窗保留菜单栏，退出结束会话。View菜单可选择深浅色和紧凑窗口；⇧⌘M打开菜单栏面板，⌘E搜索动作，⌘[/⌘]选择控件，⌘D查看差异。

```sh
scripts/check          # Rust/Swift测试（含真实UniFFI合成发现）、桥接生成、开发构建和协议边界检查
scripts/run            # 启动当前checkout的target/MicroInput.app
scripts/run --verify   # 验收实例10分钟自动退出
```

需要macOS14+、Rust stable、Swift工具链。构建不加载凭据。仅本地ad-hoc开发签名，未正式发布。

启动仅载入合成配置；点击“查找 / 刷新 USB”后只读枚举 USB HID 描述符并持续监听插拔，不会打开硬件或申请权限。候选经完整 usage pairs 与 OS USB 来源筛选，多候选必须明确选择；0x8297 标为未验型号，固件始终显示未知。插拔事件更新候选，选择不读取或应用设备配置。草稿保存在用户Application Support/MicroInput/workbench.json；退出重开恢复。参数表单需点击保存，撤销历史仅在当前会话。无法执行动作或写入设备。支持显式USB身份查询，当前303A:8298/固件0.6.3已在应用路径读回；显式身份为受支持的0.6.3后可读取两份限定配置文件，按块校验原始SHA-1并再次list，再一次发布只读快照。查看设备不会改变本地草稿，返回后保留编辑与撤销。其他固件仍拒绝读取，不把演示或导入数据作为设备快照。

规格见 [SPEC](docs/spec/SPEC.md)、分阶段验收见 [BUILD_PROMPT](docs/spec/BUILD_PROMPT.md)、[证据](docs/spec/EVIDENCE.md)和[未知项](docs/spec/UNKNOWNS.md)。

## 调研入口

可直接用于只读客户端实现的结论见 [USB 发现、身份与配置读取协议](docs/protocol-readonly.md)。依据 Input 1.1.0 静态分析及本机 IOKit 枚举；独立IOKit探针曾读回0.6.3；应用身份路径已读回0.6.3；当前0.6.3的配置读取及原生只读浏览已有E-079验收；完整生命周期和物理映射仍待验收。

以下为本机原始调查材料，保存在 Git 忽略的 `.scratch/input-study/`，不随源码提交：

- [官方来源、能力矩阵与公开协议线索](.scratch/input-study/out/research.md)
- [已连接 UI 清单](.scratch/input-study/out/connected-ui-inventory.md)
- [灯光、摇杆、层、AppSense 与参数实验](.scratch/input-study/out/layer-lighting-joystick-followup.md)
- [观察规格](.scratch/input-study/out/spec.json)和[证据记录](.scratch/input-study/out/evidence.jsonl)
- [原任务历史](.scratch/input-study/task-history.md)

旧入口 `~/agent-tasks/worklouder-input-study` 仅保留晋升记录和兼容链接。后续项目事实、代码和进度只在本项目维护。

## 下一步

PR #12/#13 已交付原生离线工作台、结构化动作和本地持久化。当前已有设备发现、身份查询与[#5 配置读取](https://github.com/semantic-craft/micro-input/issues/5)实现；配置原生浏览验收见[E-079](docs/spec/EVIDENCE.md)，[#6 生命周期](https://github.com/semantic-craft/micro-input/issues/6)及未核实物理映射仍待验证。范围继续限定只读，不从核心读回推定全部S1完成。

完整目标与 35 张工单入口为 [#44](https://github.com/semantic-craft/micro-input/issues/44)，技术契约见 [实施设计](docs/implementation-plan.md)，阶段验收见 [BUILD_PROMPT](docs/spec/BUILD_PROMPT.md)。规划已批准不等于未来阶段获准执行；当前不接自动化、不写设备、不执行宿主动作。未完成技术细化的工单使用 needs-info，并在工单内列明缺项。
