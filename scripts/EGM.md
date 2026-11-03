# 开发脚本契约

## Overview

这些脚本构建当前 checkout 的 Rust/Swift 应用，执行离线检查并启动开发构建。本契约以 origin/main 的文档任务基线为准。

## Interface

- [build](./build)：设置 `MACOSX_DEPLOYMENT_TARGET=14.0`，构建 mi-ffi、调用 `uniffi-bindgen generate`、复制绑定并运行 Swift build，打包 `target/MicroInput.app`。
- [check](./check)：依次运行 Rust 格式检查、workspace 测试、build、Swift 测试与边界脚本。
- [check-boundaries.py](./check-boundaries.py)：`found` / `expected` 核对 UNKNOWN-01 至 UNKNOWN-07；`production_framers` 必须只包含 UnimplementedFramer。
- [run](./run)：启动当前 checkout 已有二进制；`--verify` 使用隔离临时文档目录和 600 秒等待上限。

## Invariants

- `build` 和 `check` 使用 `set -euo pipefail`，前一步失败停止；绑定由所构建 mi-ffi 库生成。
- `check-boundaries.py:production_framers` 是当前占位实现的静态守卫，不证明真实 HID 只读链路已验收。
- `run:app` 不存在时退出并提示先构建；`qa` 只在 `--verify` 存在时启用，使用 `MI_DOCUMENT_DIR` 隔离文档。未隔离 bundle ID、签名或用户偏好。
- `run:p.wait` 超时或键盘中断后先 terminate，5 秒未退出则 kill 并等待；本版本没有独立 launcher/audit 证据文件。

## Side Effects

- `build` 调用 Cargo/Swift 编译器，写 target、Swift 构建目录与生成桥接目录，复制 bundle 并做 ad-hoc codesign。
- `check` 运行测试与构建；`check-boundaries.py` 读取本地 Rust/UNKNOWN 文件。
- `run` 启动应用；QA 创建临时目录，退出时清理该目录。普通启动沿用应用默认文档位置。

## Verification

在仓库根目录运行 `scripts/check`；仅验证静态守卫可用 `python3 scripts/check-boundaries.py`。本次文档任务未构建、启动应用或操作设备。
