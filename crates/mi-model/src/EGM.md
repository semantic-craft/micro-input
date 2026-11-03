# mi-model 源码契约

## Overview

这里实现合成配置投影、本地动作和 v1 文档；不把演示格式当已验证设备 schema。协议规划见仓库 `docs/implementation-plan.md`，其中 v2 和比较仍为后续目标。

## Interface

- [lib.rs](./lib.rs)：`Source`、`DeviceConfig`、`ActiveProfile::Unknown`、`Profile`、`Layer`、`Joystick`、`Lights`、`Token`、`Binding`、`Macro`、`MacroEvent`、`Act`、`MultiAction`、`ParseError`；`parse_config` 解析合成交换格式，`demo` 载入编译期夹具。
- [draft.rs](./draft.rs)：`ActionKind` 六种意图为 Shortcut/Text/OpenUrl/AgentCommand/FocusSession/SkillReference；`Action::validate`、`actions`、`binding_key`；`Draft::{new,validate,from_bytes,assign,undo}`。
- [storage.rs](./storage.rs)：`DocumentStore::{open,update}` 和公开 `draft`；update 接收操作 Draft 的闭包。

## Invariants

- `lib.rs:parse_config` 要求按键行宽 2/4/4/3、旋钮三槽；摇杆扇区为变长数组。解析返回 Imported、无固件、未知活动 Profile；`demo` 单独标为 Demo。
- `lib.rs:binding` 仅 KC_A/KC_B 识别为 Basic；macro/multi/smart 引用缺失产生 warning，其他 token 保留 Unknown。raw 保存 JSON 值语义，不保证输入字节和空白原样。宏 act 0/1/2 只属合成定义，delay/term 当前只检查 i64 类型。
- `draft.rs:Action::validate` 校验名称与参数长度、修饰键白名单和带 host 的 HTTP(S) URL；不验证工具存在性，不执行意图。
- `draft.rs:Draft::keys/validate` 要求 schema_version=1、Profile ID 文档内唯一、Layer ID 在 Profile 内唯一，ID 为字符串或整数；edits 必须指向有效 logical-N。validate 不自行验证动作 payload。
- `draft.rs:Draft::from_bytes` 对损坏/未来版本拒绝；可解析但未知字段、不能等价往返或无效动作的文档转只读。未知原文件的保全依赖拒绝覆盖，不能声称所有未知字段都保存在 Draft 内。
- `draft.rs:Draft::assign/undo` 拒绝只读；同值赋值不增加 revision；实际编辑保存 edits 前态、清 redo、revision+1。撤销仅覆盖 edits；历史不序列化，重开清空。`binding_key` 使用来源 ID 和 logical-N 的 JSON 元组。
- `storage.rs:DocumentStore::open` 失败保留旧 draft 并阻止 update；文件缺失时使用默认草稿。读写上限 4 MiB。
- `storage.rs:DocumentStore::update` 先克隆，操作成功后以 create-new lock、预期原字节比对、0600 pending、sync_all、rename 保存，最后替换内存。已有 lock/pending 拒写；没有父目录 fsync、迁移或旧版备份。默认未 open 的 store 允许仅内存更新。
- `storage.rs:DocumentStore::update` 本身不调用 Draft::validate，也不直接拒绝 read_only；正常编辑保护来自 assign/undo，任意闭包不自动获得同样保护。

## Side Effects

- `lib.rs:parse_config` 读 UNIX 时钟填 read_at，早于 epoch 回退 0。
- `draft.rs:Draft::assign/undo` 修改内存编辑图与历史。
- `storage.rs:DocumentStore::open/update` 读取文件，创建父目录/lock/pending，替换目标文件；`Lock::drop` 尽力清理 lock，保存失败尽力清理新 pending。无设备或宿主执行调用。

## Verification

仓库根目录：`cargo test --locked -p mi-model`。本次由独立只读 scout 全文核对生产源码，未运行代码测试或设备验收。
