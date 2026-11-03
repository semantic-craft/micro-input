# SwiftUI 工作台组件

## Overview

四个手写 Swift 文件组成离线工作台。以下为模块内接口，不是公开 Swift 库 API；本地模型和持久化由 Rust 提供，Swift 只投影与导航。

## Interface

- [MicroInputApp.swift](./MicroInputApp.swift)：`MicroInputApp`、`WorkbenchHost`、`AppLifetime`、`StatusMenu`、`StatusPanel`；主窗、菜单面板、外观与尺寸命令。
- [Model.swift](./Model.swift)：`JSONValue`、`Snapshot`、`Profile`、`Layer`、`Joystick`、`DraftState`、`CatalogAction` 和 `bindingKey`；JSON 投影、未知值/引用与参数摘要。
- [Store.swift](./Store.swift)：`Backend` 仅声明 loadDemo/listDevices/connect/readConfig/disconnect/inputMonitoringStatus；`Store` 为 MainActor ObservableObject；`Events` 接 DeviceEvents 回调。
- `Store::{loadWorkbench,loadDemo,saveAction,undoDraft,selectProfile,selectLayer,refresh,connect,suspend}` 协调文档与候选连接；文档操作使用 `draftCore` 将 backend 转为具体 MicroInput。
- [Workbench.swift](./Workbench.swift)：`Workbench`、`ActionParameters`；Profile/Layer 导航、逻辑控件、分类搜索、编辑参数与差异。

## Invariants

- `MicroInputApp.init` 创建一个后端和 Store；窗口与菜单共享状态。`AppLifetime` 在最后窗口关闭后保留进程。外观是 @State，没有持久化。
- `Store.documentURL` 优先初始化 URL，其次 MI_DOCUMENT_DIR/workbench.json，再用 Application Support/MicroInput/workbench.json。`loadWorkbench` 打开文档后 loadDemo/加载目录；任意 Backend mock 不自动拥有文档能力。
- `Store.selectProfile/selectLayer` 没有后端调用；`saveAction/undoDraft` 要求可编辑文档、非 busy、真实桥接实例，返回 JSON 解码成功后才更新 Swift 草稿。
- `Store.loadDemo/connect` 用 generation 拒收过期完成；设备移除/suspend 增加 generation。但 Events.failed/changed 不带订阅代次，不能宣称保护所有回调。
- `Store.connect` 仍在连接后自动 readConfig；失败保留旧快照并标 stale。这是内部现有方法，Workbench 没有连接/刷新入口；独立身份/配置操作仍为设计目标。
- `JSONValue.bindingLabel` 保留未知值；`Snapshot.referenceDetail` 显示缺失引用。`bindingKey` 序列化来源 Profile/Layer ID 与 logical-N；编码失败返回空字符串，不保证任意数据可编码。
- `Workbench.bindings/title` 按 keys、encoders、sectors 排列；按键行宽 2/4/4/3，旋钮槽位 13–15、摇杆从16起。界面标明物理宽键和方向待核对，不把逻辑图当实机映射。
- `Workbench.filteredActions` 搜索 name/category/detail；没有收藏或参数内容搜索。`ActionParameters` 先编辑 @State，保存才调用 Store；slotKey/revision 改变会重建表单。
- `Workbench.diffSheet` 比较 edits 与当前快照可匹配的槽位，不是完整结构图 diff。所有动作仅是本地意图，没有宿主执行路径。

## Side Effects

- `StatusMenu` 创建 NSStatusItem 和复用的 NSPanel，激活应用、打开工作台；退出调用 NSApp.terminate。窗口命令修改 keyWindow 尺寸。
- `Store` 通过 Rust 文档方法读写本地草稿；refresh/connect/suspend 可委托枚举、连接、读取或断开，但当前工作台启动只调用 loadWorkbench。
- `Events` 把非隔离回调投递 MainActor；`Workbench/ActionParameters` 修改选择、焦点、sheet 并启动编辑 Task。Model.swift 只有内存编解码/格式化。

## Verification

仓库根目录：`scripts/check`；若单独运行 Swift 测试，先 `scripts/build` 生成匹配静态库/绑定，再 `swift test --package-path app/MicroInput`。本次完整阅读四个生产文件和包配置，未启动应用、构建或进行实机验收。
