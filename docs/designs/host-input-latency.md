# 托管按键动作的 AX 等待

按键已经进入执行线程后，Micro Input 会多次遍历目标应用的辅助功能树。一次聚焦需要捕获、选择、执行前校验和执行后确认，旧实现四步均会完整扫描。真实安装版采样已观察到这些调用，因而重复发现控件是已确认的等待来源。连续按键还会等待前一个动作完成。

采样包含用户按下的不同动作，不能用总采样时间除以按键次数。首次空闲采样没有证明任何动作耗时。5ms 空队列轮询和 Swift120ms 回执刷新不是这次 AX 等待的解释。独立 lsappinfo 查询7次成功，中位数4.64ms，只代表进程查询。

## 与既有项目的对照

| 项目 | 已核实实现 | 本次采用的判断 |
| --- | --- | --- |
| [Paseo 输入消费](https://github.com/mattt/paseo-creator-micro/blob/aca834c3094a64d2d2db001fb1c89f9c4d36e584/Sources/PaseoMicroCore/KeypadSession.swift#L136) | 输入事件消费、状态轮询、灯光刷新分别运行。 | 本项目已有独立执行线程，保留结构，先处理动作内部的重复工作。 |
| [Karabiner to](https://karabiner-elements.pqrs.org/docs/json/complex-modifications-manipulator-definition/to/) | 普通映射区分按下和释放。 | 本项目使用普通 to.send_user_command，没有 to_after_key_up；不改宽键和按住语义。 |
| [Hammerspoon AX](https://www.hammerspoon.org/docs/hs.axuielement.html) | 可直接读取已取得元素的属性，属性写入需要读回才能确认，失效元素需要处理。 | 对已选输入框进行实时校验与焦点读回，完整发现仍负责候选歧义。 |

没有在相同目标、负载和语义下运行这些项目，不能给出项目间速度排名。Paseo 直接投递快捷键不等于确认指定分屏输入框。

## 变更边界

复用 ManagedWindowIdentity 与控件ID。原生模块持有元素，沿最近一次完整选择扫描得到的路径检查每段实际子节点关系，跨进程、跨窗口、跨分屏、隐藏或已移除元素均拒绝。完整捕获和选择读取保留。仅聚焦动作与焦点读回使用定点读取，其余控件动作保留现有检查。没有跨动作缓存、设备映射写入或 Cindy 依赖。

当前界面的 AXScrollArea 在反查 AXParent 时出现父子列表不互为逆关系。实测记录为 depth28、children1、matchedfalse。因此校验依据扫描实际走过的 AXChildren 路径，而不是重新拼接 AXParent 路径。失效路径仍然拒绝，不转向另一个控件。

## 测量与验证

2026-11-10，在 Antigravity 2.31.0 同一窗口交替运行基线与候选，各排除一次预热，再各运行7次。计时范围是 `focus_agent_input` 进入到同一目标焦点读回成功，不包含测试进程启动。

| 构建 | 基线中位数及范围 | 候选中位数及范围 | 成功数 |
| --- | --- | --- | --- |
| debug，与日常构建配置一致 | 483.704ms，476.045–511.791ms | 291.669ms，277.348–308.113ms | 每侧7/7 |
| release | 486.850ms，469.269–493.753ms | 284.311ms，275.542–294.437ms | 每侧7/7 |

日常构建配置的这项操作减少39.7%。release 也有一致方向，说明结果不只是调试构建开销。剩余完整捕获和候选选择扫描仍会花时间，不承诺即时响应。原始对照和构建摘要见[测量回执](../verification/host-input-latency.json)。

这些是重复聚焦样本，目标可能已经聚焦。另一次真实转换由 CUA 独立确认，从 HTML content 移到 Message input，没有输入文字。候选独立采样中，执行前校验和焦点确认进入定点路径，完整扫描仍用于捕获和候选选择；采样运行未计入上述数据。

第一版基于反向父链的候选失败，已放弃。修正后的首轮debug对照双方各出现1次失败，未保留具体错误码，该轮不用于主性能结论，数据仍保留。随后debug与release各完成上述全成功对照。这些结果不证明所有动态界面下都无拒绝。硬件开关到接收器的时间没有时间戳，本轮不能声称完整物理延迟。

研究参考为 docs/agents/research-context.md、IA-09/10 与模块契约。采用宿主动作独立于设备协议及前台目标身份的约束。没有用原 Input 的静态轮询周期解释当前托管层的等待。

`MI_AUTO_HOST_READY=0 RUST_TEST_THREADS=1 scripts/verify-offline` 全量通过，包含静态检查、Rust/Swift 测试、原生构建与八个隔离 QA 场景。合成验收不替代实体按键。
