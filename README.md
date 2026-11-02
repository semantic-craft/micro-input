# Micro Input

面向 Work Louder Creator Micro 2 的独立 macOS 配置器与宿主服务。技术方向：**Rust 核心 + SwiftUI 界面**。项目名为工作名。

## 当前开发状态

首轮原生离线工作台：Rust五核心模块 + UniFFI + SwiftUI。支持合成配置浏览、动作搜索/分类、内存草稿、差异及撤销/重做。主窗和菜单栏共享状态；关闭主窗保留菜单栏，退出结束会话。

```sh
scripts/check          # Rust/Swift测试、桥接生成、开发构建和协议边界检查
scripts/run            # 启动当前checkout的target/MicroInput.app
scripts/run --verify   # 验收实例10分钟自动退出
```

需要macOS14+、Rust stable、Swift工具链。构建不加载凭据。仅本地ad-hoc开发签名，未正式发布。

启动只载入合成数据，不枚举/打开硬件或申请权限。草稿退出重置，无法执行动作或写入设备。真实读取仍受UNKNOWN接口阻断，不把演示数据作为设备快照。

规格见 [SPEC](docs/spec/SPEC.md)、分阶段验收见 [BUILD_PROMPT](docs/spec/BUILD_PROMPT.md)、[证据](docs/spec/EVIDENCE.md)和[未知项](docs/spec/UNKNOWNS.md)。

## 调研入口

可直接用于只读客户端实现的结论见 [USB 发现、身份与配置读取协议](docs/protocol-readonly.md)。依据 Input 1.1.0 静态分析及本机 IOKit 枚举；尚未完成独立 RPC 收发验收。

以下为本机原始调查材料，保存在 Git 忽略的 `.scratch/input-study/`，不随源码提交：

- [官方来源、能力矩阵与公开协议线索](.scratch/input-study/out/research.md)
- [已连接 UI 清单](.scratch/input-study/out/connected-ui-inventory.md)
- [灯光、摇杆、层、AppSense 与参数实验](.scratch/input-study/out/layer-lighting-joystick-followup.md)
- [观察规格](.scratch/input-study/out/spec.json)和[证据记录](.scratch/input-study/out/evidence.jsonl)
- [原任务历史](.scratch/input-study/task-history.md)

旧入口 `~/agent-tasks/worklouder-input-study` 仅保留晋升记录和兼容链接。后续项目事实、代码和进度只在本项目维护。

## 下一步

按S1a验收当前原生工作台；后续S2补持久化、参数编辑和完整Profile管理。S1真实设备读取必须先满足后端/schema证据门槛，硬件验收未完成。自动化和设备写入未授权。
