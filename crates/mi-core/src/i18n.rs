use mi_model::ResolvedLocale;

pub const CORE_EN_TABLE: &[(&str, &str)] = &[
    ("已就绪", "Ready"),
    ("就绪", "Ready"),
    ("待授权辅助功能", "Accessibility Permission Needed"),
    ("小键盘已连接", "Keypad Connected"),
    ("硬件布局草稿模式 · 返回后继续管理软件方案", "Hardware Layout Draft · Return to manage software profiles"),
    ("缺少 macOS 辅助功能权限，按键动作已被系统拦截，请打开辅助功能设置授权", "macOS Accessibility permission is required; key actions are blocked until granted in System Settings"),
    ("六层标准键输入已连接", "6-Layer Standard Key Input Connected"),
    ("六层输入尚未连接", "6-layer input not connected"),
    ("原生受保护方案", "Protected Native Profile"),
    ("USB 供电", "USB Powered"),
    ("电量读取中", "Reading Battery"),
    ("usb.host 保持", "usb.host Hold"),
    ("跟随宿主", "Follow Host"),
    ("常醒", "Keep Awake"),
    ("定时休眠", "Auto Sleep"),
    ("待机", "Idle"),
    ("休眠", "Sleep"),
    ("其他软件", "Other Apps"),
    ("通用", "APP"),
    ("未分配", "Unassigned"),
    ("未选择方案", "No Profile Selected"),
    ("常用软件方案", "Software Profiles"),
    ("长按说话上限 120 秒，响应超时 8 秒", "Push-to-talk limit 120s, response timeout 8s"),
    ("Codex · 原生", "Codex · Native"),
    ("缺失的方案", "Missing Profile"),
    ("缺失的方案（原引用保留）", "Missing Profile (Original Reference Kept)"),
    ("设备层未确认", "Device layer unverified"),
    ("六层输入与写入协议尚未验证，当前仅保存本地方案", "6-layer input and write protocol unverified; saved locally only"),
    ("方案不存在，原引用已保留", "Profile missing; original reference preserved"),
    ("方案归属与保存的意图不一致，不能执行", "Profile ownership mismatches saved intent; cannot execute"),
    ("保留 OAI 映射；USB 独占后由 Micro Input 当前方案控制", "Keeps OAI mapping; controlled by active Micro Input profile under USB exclusive mode"),
    (" · 绑定缺失或属于原生应用，按键已暂停", " · Binding missing or native-owned; keys paused"),
    ("检测到原版 Input 正在后台运行，已由方案锁定隔离干扰", "Work Louder Input detected in background; interference isolated by profile lock"),
    ("检测到原版 Input 正在后台抢占层切换，建议开启「锁定当前方案」", "Work Louder Input is competing for layer switching; consider enabling Lock Profile"),
    ("已锁定 · 忽略前台切换与外部干扰", "Locked · Ignores app switches & external changes"),
    ("已暂停软件按键映射", "Software key mapping paused"),
    ("已启用软件按键映射", "Software key mapping enabled"),
    ("六层板载路由", "6-Layer Onboard Routing"),
    ("手动选择方案", "Manual Profile"),
    ("手动选择方案 · 按键映射已启用", "Manual Profile · Key Mapping Active"),
    ("只读保护层 · 保留原生映射 (KV_OAI_*)", "Read-Only Protected Layer · Keeps Native Mapping (KV_OAI_*)"),
    ("保留此层的 OAI 固件映射；USB 独占后由 Micro Input 当前方案处理输入。按键映射、预设覆盖与灯光编辑保持只读保护。", "Preserves OAI firmware mappings on this layer; Micro Input handles input under USB exclusive mode. Key bindings, presets, and lighting stay read-only."),
    ("新建并切换到自定义层", "Create & Switch to Custom Layer"),
    ("自定义层", "Custom Layer"),
    ("默认层", "Default Layer"),
    ("未绑定", "Unbound"),
    ("硬件指针", "HW Mouse"),
    ("硬件滚轮", "HW Scroll"),
    ("8向摇杆", "8-Way Stick"),
    ("USB 启动方案预览", "USB Startup Preview"),
    ("模型选择（直旋模型 · 按下搜会话）", "Model Picker & Search (Turn Model · Press Search)"),
    ("模型选择（短按模型 · 长按搜会话）", "Model Picker & Search (Tap / Hold)"),
    ("旋钮轮询 Working/蓝点", "Dial Cycles Working / Unread"),
    ("旋钮+摇杆双滚轮", "Dial + Stick Dual Scroll"),
    ("旋钮滚动对话", "Dial Scrolls Chat"),
    ("旋钮调整推理档位", "Dial Adjusts Reasoning Effort"),
    ("摇杆鼠标 + 旋钮滚轮", "Stick Mouse + Dial Scroll"),
    ("左上键", "Top Left"),
    ("右上键", "Top Right"),
    ("按键 03", "Key 03"),
    ("按键 04", "Key 04"),
    ("按键 05", "Key 05"),
    ("按键 06", "Key 06"),
    ("按键 07", "Key 07"),
    ("按键 08", "Key 08"),
    ("按键 09", "Key 09"),
    ("按键 10", "Key 10"),
    ("底行左键", "Bottom Left"),
    ("底部宽键（2U）", "Bottom Wide (2U)"),
    ("底行中键", "Bottom Mid"),
    ("底行右键", "Bottom Right"),
    ("旋钮左旋", "Dial CCW"),
    ("旋钮右旋", "Dial CW"),
    ("旋钮按下", "Dial Press"),
    ("旋钮短按 / 长按", "Dial Tap / Hold"),
    ("摇杆（上）", "Stick Up"),
    ("摇杆（左）", "Stick Left"),
    ("摇杆（下）", "Stick Down"),
    ("摇杆（右）", "Stick Right"),
    ("摇杆（硬件鼠标）", "Stick (HW Mouse)"),
    ("摇杆（硬件滚轮）", "Stick (HW Scroll)"),
    ("确认/模型页", "OK/Model"),
    ("模型/搜索", "Model/Search"),
    ("模型选择中：展开 / 调高模型力度", "Model Picker: Expand / Raise Effort"),
    ("模型力度", "Effort"),
    ("硬件鼠标指针（选模型时按右调力度）", "Hardware Mouse Pointer (Right Adjusts Effort)"),
    ("硬件鼠标指针", "Hardware Mouse Pointer"),
    ("鼠标指针", "Mouse Ptr"),
    ("硬件滚轮（选模型时按右调力度）", "Hardware Scroll Wheel (Right Adjusts Effort)"),
    ("空闲就绪", "Idle"),
    ("正在运行", "Running"),
    ("等待确认", "Awaiting Input"),
    ("已完成待查看", "Completed · Unread"),
    ("执行异常", "Error"),
    ("后台运行中", "Background Active"),
    ("语音输入中", "Recording Voice"),
    ("按住说话已就绪", "Push-to-Talk Ready"),
    ("待处理审批/输入", "Needs Approval / Input"),
    ("● 已完成待查看", "● Completed · Unread"),
    (":: 正在运行", ":: Running"),
    ("空闲已读", "Idle · Read"),
    ("Antigravity 后台重连中", "Reconnecting Antigravity Daemon"),
    ("Antigravity 后台断连", "Antigravity Daemon Disconnected"),
    ("等待审批或用户输入", "Waiting for approval or user input"),
    ("子代理等待审批或用户输入", "Subagent waiting for approval or user input"),
    ("已完成未读 · 按键直达", "Completed unread · Press key to jump"),
    ("执行出错，点击查看", "Execution error, click to inspect"),
    ("纯白", "Pure White"),
    ("翡翠绿", "Emerald"),
    ("海洋蓝", "Ocean Blue"),
    ("青碧", "Cyan Teal"),
    ("琥珀金", "Amber Gold"),
    ("柔和 (35%)", "Soft (35%)"),
    ("标准 (65%)", "Standard (65%)"),
    ("明亮 (95%)", "Bright (95%)"),
    ("1 分钟", "1 min"),
    ("3 分钟", "3 min"),
    ("5 分钟", "5 min"),
    ("常亮", "Always On"),
    ("会话键", "Session Key"),
    ("动作键", "Action Key"),
    ("全部", "All"),
    ("四分支 Multi", "4-Branch Multi"),
    ("旋钮长按 (ENC_HOLD)", "Encoder Hold (ENC_HOLD)"),
    ("宿主 CGEvent（逻辑扇区）：即时生效、仅限当前 Mac、不写 Flash；支持为上下左右 4 个摇杆方向分别绑定动作。", "Host CGEvent (Logical Sectors): Takes effect immediately on this Mac without Flash writes; binds separate actions to 4 stick directions."),
    ("硬件 MOUSE 指针：需经配置审阅并应用到 /fs/keymap.json 生效；写入后跨主机脱机生效，不占用宿主摇杆方向槽位。", "Hardware MOUSE Pointer: Requires config review & apply to /fs/keymap.json; works standalone across hosts without using logical stick slots."),
    ("硬件 MOUSE 指针：需固件 0.7.0-rc.16+ 并应用到 /fs/keymap.json 才能生效（当前设备固件暂不支持，建议改用「宿主逻辑扇区」或先升级固件）。", "Hardware MOUSE Pointer: Requires firmware 0.7.0-rc.16+ applied to /fs/keymap.json (unsupported on current firmware; use Host Logical Sectors or update firmware)."),
    ("硬件 SCROLL 滚轮：需经配置审阅并应用到 /fs/keymap.json 生效；写入后跨主机脱机平滑滚动，不占用宿主摇杆方向槽位。", "Hardware SCROLL Wheel: Requires config review & apply to /fs/keymap.json; smooth standalone scrolling across hosts without using logical stick slots."),
    ("硬件 SCROLL 滚轮：需固件 0.7.0-rc.16+ 并应用到 /fs/keymap.json 才能生效（当前设备固件暂不支持，建议改用「宿主逻辑扇区」或先升级固件）。", "Hardware SCROLL Wheel: Requires firmware 0.7.0-rc.16+ applied to /fs/keymap.json (unsupported on current firmware; use Host Logical Sectors or update firmware)."),
    ("保留来源模式：保留固件原始摇杆定义，宿主不覆盖扇区动作（Mac 运行时不触发逻辑槽位）。", "Keep Source Mode: Preserves original firmware joystick definition without host sector overrides."),
    ("请按下快捷键…", "Press shortcut…"),
    ("请先录入快捷键按键", "Record a shortcut key first"),
    ("显式配置下先停听写、聚焦目标输入区、暂存剪贴板粘贴后再回车", "Stops dictation, focuses target input, pastes via staged clipboard, then submits"),
    ("默认仅粘贴不回车（no_newline） · 自动暂存恢复剪贴板", "Pastes without newline by default · Stages and restores clipboard"),
    ("自有本地序列 · 已验证键名可宿主执行", "Local macro sequence · Verified keys execute on host"),
    ("本地四分支 · 不执行 · 固件范围未知", "Local 4-branch Multi · Preview only · Firmware support unverified"),
    ("新建宏", "New Macro"),
    ("新建 Multi", "New Multi"),
    ("未设置", "Unset"),
    ("按键背光 (Backlight)", "Key Backlight"),
    ("底灯环绕 (Underglow)", "Underglow"),
    ("单击 (Tap)", "Tap"),
    ("双击 (Double Tap)", "Double Tap"),
    ("长按 (Hold)", "Hold"),
    ("点击后长按 (Tap + Hold)", "Tap + Hold"),
    ("旋钮按下确认", "Encoder Press to Confirm"),
    ("摇杆回弹确认", "Stick Release to Confirm"),
    ("摇杆选择 · 旋钮按下或快速回弹确认 · 慢回中取消", "Tilt stick to select · Press dial or flick-release to confirm · Return slowly to cancel"),
    ("尚未连接已核验的键盘", "Verified keypad not connected"),
    ("请读取当前设备身份与配置", "Read device identity and config first"),
    ("当前摇杆或旋钮映射不可用于菜单", "Current stick or dial mapping unavailable for wheel menu"),
    ("设备操作期间或映射重验前暂停菜单", "Wheel menu paused during device op or before re-verification"),
    ("后退", "Back"),
    ("前进", "Forward"),
    ("按键速查", "Cheat Sheet"),
    ("Back · 后退", "Back"),
    ("Forward · 前进", "Forward"),
    ("Submit · 提交", "Submit"),
    ("Stop · 停止", "Stop"),
    ("当前配置不可用", "Active profile unavailable"),
    ("应用未安装", "App not installed"),
    ("未设置指令文本", "Prompt text not set"),
    ("没有可用的目标配置", "No target profile available"),
    ("当前配置不是桌面 Agent", "Active profile is not a desktop agent"),
    ("没有可后退的打开历史", "No back navigation history"),
    ("没有可前进的打开历史", "No forward navigation history"),
    ("Creator Micro 2 (303A:8298) · 离线草稿模式", "Creator Micro 2 (303A:8298) · Offline Draft Mode"),
    ("六层操作正在进行", "6-layer operation in progress"),
    ("六层恢复记录尚未处理，请先恢复原设备配置", "Pending 6-layer recovery record; restore original device config first"),
    ("旧六层安装已暂停，请先恢复原设备配置", "Previous 6-layer install paused; restore original device config first"),
    ("隔离工作台不能应用实体六层方案", "Isolated workbench cannot apply physical 6-layer plan"),
    ("实体输入已验证", "Physical input verified"),
    ("等待首次实体输入", "Waiting for first physical input"),
    ("六层已保存，当前输入连接尚未确认", "6-layer plan saved; input connection unverified"),
    ("六层恢复待处理，吞键规则保留", "6-layer recovery pending; key-grab rule retained"),
    ("已解除方案锁定", "Profile unlocked"),
    ("已打开高级设置", "Advanced settings opened"),
    ("已关闭高级设置", "Advanced settings closed"),
    ("已打开主工作台窗口", "Main workbench window opened"),
    ("正在检测小键盘连接…", "Checking keypad connection…"),
    ("未检测到 Creator Micro 2 USB 连接（离线工作台就绪）", "Creator Micro 2 USB not detected (offline workbench ready)"),
    ("小键盘已断开连接（等待重新插入）", "Keypad disconnected (waiting for reconnect)"),
    ("已删除方案", "Profile deleted"),
    ("已恢复默认软件方案", "Default software profiles restored"),
    ("已导出方案预设", "Profile preset exported"),
    ("已导入方案预设", "Profile preset imported"),
    ("已返回常用软件方案", "Returned to software profiles"),
    ("已打开 macOS 辅助功能权限设置", "Opened macOS Accessibility settings"),
    ("已显示当前宿主配置的按键速查", "Showing Cheat Sheet for active host profile"),
    ("没有可打开的历史目标", "No navigation history target available"),
    ("当前处于离线草稿模式 · 设备身份：Creator Micro 2 (303A:8298)", "Offline draft mode · Identity: Creator Micro 2 (303A:8298)"),
    ("USB 专用接口已独占", "USB vendor interface claimed"),
    ("已建立 HID 会话", "HID session connected"),
    ("板载三层或启动层未确认，按键已暂停", "Onboard 3-layer or startup layer unverified; keys paused"),
    ("等待键盘返回第 1 层，其他层按键已暂停", "Waiting for keypad to return to Layer 1; other layers paused"),
    ("板载三层已就绪 · 第 1 层", "Onboard 3-layer ready · Layer 1"),
    ("旧三层绑定缺失或属于原生应用，按键已暂停", "Legacy 3-layer binding missing or native-owned; keys paused"),
    ("输入流有缺口，已停止原目标动作，请重新连接", "Input stream gap detected; stopped active action, please reconnect"),
    ("正在准备六层输入并核验设备配置…", "Preparing 6-layer input and verifying device config…"),
    ("正在恢复原设备配置，确认后再撤销吞键规则…", "Restoring original device config before removing key-grab rule…"),
    ("六层配置已读回，等待首次实体输入验证", "6-layer config read back; waiting for first physical input"),
    ("原设备配置及 Karabiner 设置已恢复", "Original device config and Karabiner settings restored"),
    ("宿主动作清理未确认，输入已暂停", "Host action cleanup unverified; input paused"),
    ("系统睡眠 · 旧会话已失效，正在释放资源", "System sleeping · Releasing previous session"),
    ("系统已唤醒 · 等待旧会话释放", "System woke · Waiting for previous session cleanup"),
    ("清理任务已中断 · 宿主输入保持暂停", "Cleanup interrupted · Host input remains paused"),
    ("系统已唤醒 · 设备发现已刷新，请明确重新连接", "System woke · Device discovery refreshed; reconnect explicitly"),
    ("系统已唤醒 · USB 发现尚未开始", "System woke · USB discovery not started"),
    ("正在切换会话，等待目标回读…", "Switching session, awaiting target readback…"),
    ("已按下 ENC_CLK（当前槽位未绑定动作）", "Pressed ENC_CLK (slot unbound)"),
    ("当前运行版本尚未读取", "Running version not yet read"),
    ("尚未检查官方固件", "Official firmware not yet checked"),
    ("有较新的固件可用", "Newer firmware available"),
    ("当前已是此通道最新版本；再次安装将重刷相同版本", "Already on latest version for this channel; reinstalling reflashes same version"),
    ("所选版本低于当前版本；安装将降级，配置兼容性未确认", "Selected version is older; installing downgrades with unverified config compatibility"),
    ("检查 Creator Micro 2 官方稳定版固件", "Check official stable firmware for Creator Micro 2"),
    ("正在读取官方固件发布…", "Checking official firmware releases…"),
    ("正在下载并校验官方固件…", "Downloading and verifying official firmware…"),
    ("正在查找引导模式设备…", "Scanning for bootloader devices…"),
    ("正在连接所选设备；完整备份后才会写入…", "Connecting to selected device; full backup runs before write…"),
    ("后台操作中断；设备状态尚未确认", "Background operation interrupted; device state unverified"),
    ("下载完成，SHA-256 与官方附件摘要一致", "Download complete; SHA-256 matches official release digest"),
    ("下载完成；官方未提供摘要，仅记录本地 SHA-256", "Download complete; no official digest provided, local SHA-256 recorded"),
    ("尚未发现引导模式串口，可稍后重新查找", "No bootloader serial port found yet; try scanning again"),
    ("请明确选择这把 Creator Micro 2 的串口；Espressif 名称本身不能确认型号", "Select the serial port for this Creator Micro 2 explicitly; Espressif name alone does not confirm model"),
    ("已请求进入引导模式，暂未自动发现串口，请点击「查找串口」确认", "Bootloader requested, port not yet detected; click Scan Ports to verify"),
    ("已进入引导模式并发现以下串口，请核对并手动选择设备串口", "Bootloader active with ports below; verify and select device port"),
    ("正在连接设备…", "Connecting to device…"),
    ("正在保存完整 Flash 备份，请保持连接…", "Saving full Flash backup, keep connected…"),
    ("正在写入固件，请勿断开设备或退出应用…", "Writing firmware, do not disconnect or quit…"),
    ("正在校验写入结果…", "Verifying written firmware…"),
    ("正在重启设备…", "Restarting device…"),
    ("正在更新固件…", "Updating firmware…"),
    ("固件写入校验通过；运行版本与原键位尚待重连核对。完整 Flash 备份已保留。", "Firmware write verified; reconnect to confirm running version and keymap. Full Flash backup retained."),
    ("固件流程已结束。请重新连接设备核对运行版本与键位；新固件的配置兼容性需另行核验。", "Firmware workflow finished. Reconnect to verify version and keymap."),
    ("需连接 0.7.0-rc.16+ 独占固件设备", "Requires 0.7.0-rc.16+ USB exclusive device"),
    ("已更新电源管理设置", "Updated power management settings"),
    ("已开启方案切层联动（仅 RAM 零擦写）", "Enabled profile-to-layer sync (RAM only, zero Flash wear)"),
    ("已关闭方案切层联动", "Disabled profile-to-layer sync"),
    ("已开启 AI 会话状态自动映灯", "Enabled AI session status RGB sync"),
    ("已关闭 AI 会话状态自动映灯", "Disabled AI session status RGB sync"),
    ("备份目录路径缺失", "Backup directory path missing"),
    ("备份写后读回不一致", "Backup readback mismatch"),
    ("需要已连接的 USB 专用独占会话", "Requires a connected USB exclusive session"),
    ("固件版本尚未确认", "Firmware version unverified"),
    ("当前设备固件版本尚未确认，不支持 0.7.0 硬件能力", "Device firmware version unverified; 0.7.0 hardware capabilities unavailable"),
    ("USB 专用桥接当前未连接", "USB vendor bridge is not connected"),
    ("未连接支持 0.7.0 硬件能力的 USB 专用设备", "No 0.7.0-capable USB vendor device connected"),
    ("固件版本尚未确认，请等待连接验证完成", "Firmware version unverified; wait for connection verification"),
    ("固件操作正在进行，请等待完成", "Firmware operation in progress; wait for completion"),
    ("固件更新流程正在占用设备，请结束后再试", "Device reserved for firmware update; finish update first"),
    ("上一条 0.7.0 硬件指令尚未完成", "Previous 0.7.0 hardware command still in progress"),
    ("亮度和速度必须为有限数值", "Brightness and speed must be finite numbers"),
    ("RGB 颜色必须为 6 位十六进制（例如 #33AAFF）", "RGB color must be 6-digit hex (e.g. #33AAFF)"),
    ("RGB 颜色格式无效", "Invalid RGB color format"),
    ("USB 会话已断开", "USB session disconnected"),
    ("固件更新期间设备控制已暂停；请先结束固件更新流程", "Device control paused during firmware update; finish firmware update first"),
    ("请先下载并校验固件", "Download and verify firmware first"),
    ("官方未提供可核验摘要，不能启动自动安装", "No verifiable official digest; automatic install blocked"),
    ("官方未提供可核验摘要，不能自动安装", "No verifiable official digest; automatic install blocked"),
    ("请等待当前设备操作完成", "Wait for the current device operation to finish"),
    ("已暂停设备控制，请查找引导模式串口", "Device control paused; scan for bootloader serial port"),
    ("检测到多台 Creator Micro 2 有线 USB 设备，请仅保留目标键盘后再准备更新", "Multiple wired Creator Micro 2 USB devices detected; keep only the target keypad connected"),
    ("未检测到 Creator Micro 2 的有线 USB 连接或引导串口（蓝牙连接不支持固件备份与刷写，请接入 USB 数据线后重试）", "No wired Creator Micro 2 USB connection or bootloader port detected (Bluetooth cannot back up or flash firmware; connect via USB cable)"),
    ("检测到设备已处于引导模式，请核对并选择对应串口", "Device already in bootloader mode; verify and select its serial port"),
    ("设备控制已暂停，正在请求进入引导模式…", "Device control paused; requesting bootloader mode…"),
    ("原设备连接已变化，未发送引导命令", "Device connection changed; bootloader command not sent"),
    ("请先结束当前设备更新流程", "Finish the current firmware update workflow first"),
    ("隔离验收不会切换实体设备的引导模式", "Isolated QA mode never switches physical devices into bootloader mode"),
    ("请先准备设备更新", "Prepare device update first"),
    ("串口不在本次发现结果中，请重新查找", "Selected port is not in the current scan results; scan again"),
    ("隔离验收不能刷写实体设备", "Isolated QA mode never flashes physical devices"),
    ("当前没有设备更新流程", "No active firmware update workflow"),
    ("请先检查固件更新", "Check for firmware updates first"),
    ("请先暂停设备控制并准备更新", "Pause device control and prepare update first"),
    ("尚未下载固件", "Firmware not downloaded yet"),
    ("请选择这把 Creator Micro 2 对应的引导模式串口", "Select the bootloader serial port for this Creator Micro 2"),
    ("此资产没有通过官方 SHA-256 校验，不能自动安装", "Asset did not pass official SHA-256 verification; automatic install blocked"),
    ("下载后的固件文件已变化，请重新下载", "Downloaded firmware file changed; download again"),
    ("固件不是 Creator Micro 2 使用的 ESP32-S3 镜像", "Firmware is not an ESP32-S3 image for Creator Micro 2"),
    ("固件缺少完整分区表", "Firmware missing complete partition table"),
    ("分区表无法识别，不能确认配置保留范围", "Unrecognized partition table; cannot verify preserved config range"),
    ("数据分区范围无效", "Invalid data partition range"),
    ("未找到配置数据分区，不能确认配置保留范围", "Config data partition not found; cannot verify preserved range"),
    ("新旧固件的数据分区布局不同；需要专门迁移流程，未写入", "Data partition layout differs between firmware versions; migration required, nothing written"),
    ("当前数据分区超出 Flash 备份范围，未写入", "Data partition exceeds Flash backup range; nothing written"),
    ("固件范围溢出", "Firmware address range overflow"),
    ("固件写入范围覆盖现有数据分区，未写入", "Firmware write range overlaps existing data partition; nothing written"),
    ("镜像缺少 Flash 容量声明", "Firmware image missing Flash size header"),
    ("镜像 Flash 容量声明无法识别", "Unrecognized Flash size header in firmware image"),
    ("探测到的 Flash 容量不足以匹配官方镜像，未写入", "Detected Flash capacity smaller than official image; nothing written"),
    ("所选引导端口已经断开或身份变化，请重新选择", "Selected bootloader port disconnected or changed; select again"),
    ("安全下载模式不支持此更新流程，未写入固件", "Secure download mode is unsupported; firmware not written"),
    ("完整 Flash 备份长度不符，未写入", "Full Flash backup size mismatch; nothing written"),
    ("发布状态与固件版本不一致", "Release draft/prerelease state mismatches firmware version"),
    ("发布中没有匹配的 Creator Micro 2 固件文件", "No matching Creator Micro 2 firmware binary in release"),
    ("固件附件身份或上传状态无效", "Invalid firmware asset identity or upload state"),
    ("固件附件摘要格式不支持", "Unsupported firmware asset digest format"),
    ("固件附件摘要无效", "Invalid firmware asset digest"),
    ("固件发布链接不属于官方仓库", "Firmware release URL does not belong to official repository"),
    ("稳定通道返回了预发布固件", "Stable channel returned a prerelease firmware"),
    ("官方仓库暂无可用固件", "No firmware releases available in official repository"),
    ("固件下载大小与发布信息不一致", "Downloaded firmware size mismatches release metadata"),
    ("固件 SHA-256 校验失败", "Firmware SHA-256 verification failed"),
    ("固件附件与所选发布不匹配", "Firmware asset mismatches selected release"),
    ("宏库已达 30 个上限", "Macro library limit of 30 reached"),
    ("未打开宏编辑器", "Macro editor is not open"),
    ("未打开 Multi 编辑器", "Multi editor is not open"),
    ("Multi 动作不存在", "Multi action not found"),
    ("不是有效的 Multi 动作", "Invalid Multi action"),
    ("宏不存在", "Macro action not found"),
    ("不是有效的本地宏", "Invalid local macro"),
    ("请核验当前连接并保存原配置副本后应用", "Verify the active connection and save a backup copy before applying"),
    ("当前设备配置", "Current device config"),
    ("空绑定", "Empty binding"),
    ("已删除 / 不存在", "Deleted / Missing"),
    ("原始绑定（只读，展开查看）", "Raw binding (read-only)"),
    ("结构化内容（展开查看）", "Structured value"),
    ("空列表", "Empty list"),
    ("未知原值", "Unknown raw value"),
    ("共同值", "Common"),
    ("设备", "Device"),
    ("本地", "Local"),
    ("冲突", "Conflict"),
    ("设备接口已变化；请先核对并确认是同一设备", "Device USB interface changed; verify and confirm it is the same keypad"),
    ("设备身份不同；不能接受错误基线", "Different device identity; cannot accept mismatched baseline"),
    ("跨重连设备身份未确认；只允许明确选择整份版本", "Device identity across reconnect unverified; choose an entire version explicitly"),
    ("设备身份已由用户核对；技术身份仍未确认，不自动合并", "Device identity confirmed by user; technical serial unverified, automatic merge disabled"),
    ("未知 raw 改变；双方原值保留，禁止自动合并", "Unknown raw fields changed; automatic merge disabled"),
    ("配置源 ID 无法对照；新增与删除不能猜作同一对象", "Profile source IDs cannot be aligned; additions and deletions cannot be auto-matched"),
    ("层源 ID 无法对照；新增与删除不能猜作同一对象", "Layer source IDs cannot be aligned; additions and deletions cannot be auto-matched"),
    ("控件源 ID 无法对照；新增与删除不能猜作同一对象", "Control source IDs cannot be aligned; additions and deletions cannot be auto-matched"),
    ("缺少有效的设备基线；不能比较", "Missing valid device baseline; cannot compare"),
    ("没有设备基线；请先从核验快照新建草稿", "Missing device baseline; create draft from verified snapshot first"),
    ("基线配置不能显示", "Baseline config cannot be displayed"),
    ("来源 ID 存在歧义，不能自动对齐", "Ambiguous source IDs; cannot auto-align"),
    ("此比较不能由人工确认绕过设备身份差异", "Device identity mismatch cannot be bypassed by manual confirmation"),
    ("草稿已变化；请重新比较", "Draft changed; compare again"),
    ("缺少设备基线", "Missing device baseline"),
    ("设备基线无效", "Invalid device baseline"),
    ("设备快照或身份已变化；请重新读取并比较", "Device snapshot or identity changed; read snapshot and compare again"),
    ("设备身份不同；不能套用基线", "Different device identity; cannot adopt baseline"),
    ("存在冲突；不能合并", "Conflicts present; cannot merge"),
    ("文档修订号已达上限", "Document revision limit reached"),
    ("没有设备基线，拒绝应用", "Missing device baseline; apply rejected"),
    ("设备身份未经核验，拒绝应用", "Device identity unverified; apply rejected"),
    ("设备身份状态未知，拒绝应用", "Unknown device identity state; apply rejected"),
    ("缺少新读取的文件摘要，拒绝应用", "Missing fresh file digest; apply rejected"),
    ("设备身份或固件已变化；请重新比较", "Device identity or firmware changed; compare again"),
    ("设备摘要已变化；请重新比较", "Device file digest changed; compare again"),
    ("当前会话或固件未核验", "Active session or firmware unverified"),
    ("没有键位或灯光变化", "No key binding or lighting changes"),
    ("只有 DeviceKey 普通设备按键可准备；宿主快捷键不可转换", "Only DeviceKey hardware bindings can be staged; host shortcuts cannot be converted"),
    ("先保存并核验原始副本", "Save and verify the backup copy first"),
    ("文档或修订已变化；请重新预览", "Document or revision changed; preview again"),
    ("会话快照已变化；请重新读取并预览", "Session snapshot changed; read snapshot and preview again"),
    ("原生摇杆 MOUSE/SCROLL 模式需要固件 0.7.0-rc.16 或更高版本", "Native joystick MOUSE/SCROLL modes require firmware 0.7.0-rc.16 or newer"),
    ("MS_* 鼠标硬件令牌需要固件 0.7.0-rc.16 或更高版本", "MS_* hardware mouse tokens require firmware 0.7.0-rc.16 or newer"),
    ("基线原字节已变化；请重新比较", "Baseline bytes changed; compare again"),
    ("存在未绑定的动作；不能应用", "Unbound actions exist; cannot apply"),
    ("结构、模式或其他字段变化不在配置应用范围内", "Structural, mode, or unsupported field changes cannot be applied"),
    ("来源引用或结构变化不在配置应用范围内", "Source reference or structural changes cannot be applied"),
    ("完整现状 keymap；可能不可解析", "Full current keymap; may be unparseable"),
    ("关联原副本；保留现状 smart_actions", "Original backup keymap; preserves current smart_actions"),
    ("名称应为 1–256 字节，且不能全为空白", "Name must be 1–256 bytes and not blank"),
    ("本地文档版本或身份不合法", "Invalid local document version or identity"),
    ("文档对象 UUID 缺失或重复", "Missing or duplicate document object UUID"),
    ("配置顺序引用不完整或重复", "Incomplete or duplicate profile order references"),
    ("层归属引用不完整或重复", "Incomplete or duplicate layer ownership references"),
    ("控件顺序引用不完整或重复", "Incomplete or duplicate control order references"),
    ("逻辑控件身份无效或重复", "Invalid or duplicate logical control identity"),
    ("来源引用指向不存在的对象", "Source reference points to a missing object"),
    ("跨层合并键索引必须在 0..13 范围内且互不相同", "Cross-layer merged key indices must be distinct and within 0..13"),
    ("层不属于所选配置", "Layer does not belong to the selected profile"),
    ("受保护宿主层（原生 OAI）不支持复制层", "Protected host layer (Native OAI) cannot be duplicated"),
    ("逻辑控件顺序无法显示", "Logical control order cannot be displayed"),
    ("层缺少基础逻辑控件", "Layer is missing base logical controls"),
    ("目标排序位置不存在", "Target sort position does not exist"),
    ("未打开重连比较窗口", "Reconnect comparison modal is not open"),
    ("未打开配置应用预检窗口", "Apply preparation modal is not open"),
    ("重试清理", "Retry Cleanup"),
    ("已完成宿主动作清理", "Host action cleanup completed"),
    ("动作读回未确认", "Action readback unconfirmed"),
    ("已执行", "Executed"),
    ("未执行", "Not executed"),
    ("原目标释放未确认", "Original target release unconfirmed"),
    ("原目标听写停止未确认", "Original dictation stop unconfirmed"),
    ("宿主动作清理未确认", "Host action cleanup unconfirmed"),
    ("旧会话清理未确认", "Previous session cleanup unconfirmed"),
    (
        "上次听写所在窗口或对话已切换，请先确认原录音已停止",
        "Previous dictation window or chat changed; confirm recording stopped",
    ),
    (
        "目标窗口或会话已变化，请回到原窗口后重试",
        "Target window or session changed; return to the original window and retry",
    ),
    (
        "听写状态回读未确认，请检查原窗口是否仍在录音",
        "Dictation state unconfirmed; check whether recording is still active",
    ),
    (
        "无法确定目标分屏，请点击目标会话区域",
        "Unable to determine target pane; click inside the target session",
    ),
    ("未找到唯一可编辑输入框", "No unique editable input field found"),
    (
        "未找到可用的听写按钮，请先切到对话输入界面",
        "Dictation control unavailable; switch to an active chat input",
    ),
    ("当前已在听写中", "Dictation is already recording"),
    ("听写正在结束处理中，请稍候", "Dictation is still finishing; please wait"),
    (
        "苹果 Fn 听写键释放未确认，请点击「重试清理」",
        "Apple Fn key release unconfirmed; click Retry Cleanup",
    ),
    (
        "动作回读未确认，请检查目标应用状态",
        "Action readback unconfirmed; check target app state",
    ),
    (
        "目标应用当前无法执行该操作",
        "Target app cannot perform this action right now",
    ),
    (
        "当前目标应用暂不支持该动作",
        "Current target app does not support this agent action",
    ),
    (
        "当前目标应用暂不支持托管听写",
        "Current target app does not support managed dictation",
    ),
    (
        "当前目标应用不支持此指令",
        "Current target app does not support this command",
    ),
    (
        "当前无法执行会话切换",
        "Session navigation is currently unavailable",
    ),
    (
        "当前目标应用不支持 Subagent 选择",
        "Subagent picker is unavailable in the current app",
    ),
    (
        "按键输入已失效，未执行动作",
        "Input event expired; action not executed",
    ),
    (
        "粘贴内容为空，未执行粘贴",
        "Paste text is empty; nothing pasted",
    ),
    (
        "实时语音状态回读超时，请检查目标窗口",
        "Live voice readback timed out; check target window",
    ),
    (
        "会话切换已取消或失效",
        "Session navigation cancelled or expired",
    ),
    ("已确认目标会话", "Target session confirmed"),
    (
        "目标会话当前不可用",
        "Target session is currently unavailable",
    ),
    (
        "会话列表已更新，请重新按键切换",
        "Session list changed; press again to switch",
    ),
    (
        "正在切换其他会话，请稍候",
        "Another session switch is in progress; please wait",
    ),
    (
        "目标会话已失效或暂不支持切换",
        "Target session is stale or unsupported",
    ),
    (
        "会话切换输入已失效，请重试",
        "Session navigation input expired; retry",
    ),
    (
        "会话切换状态未确认，请检查目标窗口",
        "Session switch unconfirmed; check target window",
    ),
    (
        "隔离模式下不执行实体会话切换",
        "Session navigation is disabled in isolated mode",
    ),
    (
        "未能读取会话列表，请确认目标应用已打开",
        "Session list unavailable; check that target app is open",
    ),
    (
        "目标应用窗口或工作区已变化",
        "Target app window or workspace changed",
    ),
    (
        "暂不支持切换到此目标会话",
        "Switching to this session target is unsupported",
    ),
    ("原听写停止未确认", "Previous dictation stop unconfirmed"),
    (
        "旧设备操作清理未确认",
        "Previous device operation cleanup unconfirmed",
    ),
    ("清理任务已中断", "Cleanup task interrupted"),
    ("宿主动作清理失败", "Host action cleanup failed"),
    ("设备监视清理失败", "Device monitor cleanup failed"),
    ("会话未确认", "Session switch unconfirmed"),
    ("已切换会话", "Switched session"),
    ("会话未切换", "Session not switched"),
    (
        "正在启动 30 秒硬件灯光预览…",
        "Starting 30s hardware lighting preview…",
    ),
    (
        "硬件灯光预览已开启（仅临时 lights.preview，不写入闪存）",
        "Hardware lighting preview active (temporary lights.preview only, no Flash write)",
    ),
    (
        "硬件灯光预览已停止（未自动恢复设备运行态灯效）",
        "Hardware lighting preview stopped (runtime lighting not auto-restored)",
    ),
    (
        "30 秒硬件灯光预览已自动停止（未自动恢复设备运行态灯效）",
        "30s hardware lighting preview auto-stopped (runtime lighting not auto-restored)",
    ),
    (
        "灯光预览正在启动，请稍候",
        "Lighting preview is starting; please wait",
    ),
    ("灯光预览已在进行中", "Lighting preview is already active"),
    (
        "上一轮灯光预览正在停止，请稍候重试",
        "Previous lighting preview is stopping; try again shortly",
    ),
    (
        "当前图层尚未配置双区灯效，请先初始化灯效后再预览",
        "This layer has no dual-zone lighting configured; initialize lighting first",
    ),
    (
        "六层操作正在进行，请稍后重试灯光预览",
        "6-layer operation in progress; retry lighting preview shortly",
    ),
    (
        "六层运行代次已失效，无法启动灯光预览",
        "6-layer runtime generation retired; cannot start lighting preview",
    ),
    (
        "当前已有灯光预览正在进行",
        "A lighting preview is already in progress",
    ),
    (
        "当前 USB 会话请求配额即将耗尽，请稍后重试预览",
        "Current USB session request budget is nearly exhausted; retry preview shortly",
    ),
    (
        "尚未连接已核验的 USB 专用独占键盘，无法启动硬件灯光预览",
        "No verified USB exclusive keypad connected; cannot start hardware lighting preview",
    ),
    (
        "正在核验 USB 键盘会话，请稍后再试",
        "Verifying USB keypad session; try again shortly",
    ),
    (
        "设备预览通道已关闭，无法启动灯光预览",
        "Device preview channel closed; cannot start lighting preview",
    ),
    (
        "当前设备连接不支持原生灯光预览（需 macOS USB 专用独占通道）",
        "Current device connection does not support native lighting preview (requires macOS USB exclusive vendor link)",
    ),
    (
        "当前设备固件版本不支持原生灯光预览",
        "Current device firmware does not support native lighting preview",
    ),
    (
        "设备连接已断开，灯光预览已停止（未自动恢复原灯效）",
        "Device disconnected; lighting preview stopped (original lighting not auto-restored)",
    ),
    (
        "设备预览连接异常中断，已停止预览（未自动恢复原灯效）",
        "Preview connection interrupted; preview stopped without retry (original lighting not auto-restored)",
    ),
    ("灯光预览状态不可用", "Lighting preview status unavailable"),
    ("当前未打开可归档的会话", "No active conversation to archive"),
    (
        "当前会话正在运行，无法直接归档",
        "Current conversation is running and cannot be archived",
    ),
    (
        "未找到当前会话的归档按钮，请先展开左侧栏",
        "Archive button not found for the active conversation; expand the left sidebar first",
    ),
    (
        "会话归档回读未确认，请检查左侧会话栏",
        "Conversation archive readback unconfirmed; check the left sidebar",
    ),
    (
        "侧栏会话切换回读未确认，请检查目标窗口",
        "Sidebar conversation switch readback unconfirmed; check the target window",
    ),
    (
        "未找到左侧对话导航栏中的会话列表",
        "No conversations found in the left sidebar",
    ),
];

pub fn en_lookup(zh: &str) -> Option<&'static str> {
    for (k, v) in CORE_EN_TABLE {
        if *k == zh {
            return Some(v);
        }
    }
    mi_model::i18n::en_lookup(zh)
}

pub fn zh_reverse_lookup(en: &str) -> Option<&'static str> {
    for (k, v) in CORE_EN_TABLE {
        if *v == en {
            return Some(k);
        }
    }
    None
}

pub fn tr(zh: &str, locale: ResolvedLocale) -> &str {
    match locale {
        ResolvedLocale::ZhHans => zh,
        ResolvedLocale::En => en_lookup(zh).unwrap_or(zh),
    }
}

fn localize_managed_error_segment(segment: &str, locale: ResolvedLocale) -> String {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut rest = trimmed;
    let mut saw_prefix = false;
    let mut saw_unconfirmed = false;
    loop {
        if let Some(next) = rest.strip_prefix("unconfirmed:") {
            saw_prefix = true;
            saw_unconfirmed = true;
            rest = next;
            continue;
        }
        if let Some(next) = rest.strip_prefix("blocked:") {
            saw_prefix = true;
            rest = next;
            continue;
        }
        break;
    }
    let code = rest.trim();
    let known_zh = match code {
        "uncleared_dictation_target_changed" | "dictation_target_changed" => {
            Some("上次听写所在窗口或对话已切换，请先确认原录音已停止")
        }
        "target_window_changed"
        | "target_identity_changed"
        | "target_scope_changed"
        | "target_pid_mismatch"
        | "target_window_lost" => Some("目标窗口或会话已变化，请回到原窗口后重试"),
        "dictation_readback" => Some("听写状态回读未确认，请检查原窗口是否仍在录音"),
        "ambiguous_or_missing_focused_pane" => Some("无法确定目标分屏，请点击目标会话区域"),
        "ambiguous_or_missing_input" => Some("未找到唯一可编辑输入框"),
        "ambiguous_or_missing_dictation_control" | "dictation_control_unavailable" => {
            Some("未找到可用的听写按钮，请先切到对话输入界面")
        }
        "dictation_already_recording" => Some("当前已在听写中"),
        "dictation_finishing" => Some("听写正在结束处理中，请稍候"),
        "accessibility_untrusted" => {
            Some("缺少 macOS 辅助功能权限，按键动作已被系统拦截，请打开辅助功能设置授权")
        }
        "unconfirmed_apple_fn_release" | "apple_fn_cleanup_unconfirmed" => {
            Some("苹果 Fn 听写键释放未确认，请点击「重试清理」")
        }
        "unsupported_agent_adapter" => Some("当前目标应用暂不支持该动作"),
        "unsupported_dictation_adapter" => Some("当前目标应用暂不支持托管听写"),
        "unsupported_agent_command" => Some("当前目标应用不支持此指令"),
        "session_navigation_unavailable" => Some("当前无法执行会话切换"),
        "subagent_picker_unavailable" => Some("当前目标应用不支持 Subagent 选择"),
        "input_retired" => Some("按键输入已失效，未执行动作"),
        "empty_paste_text" => Some("粘贴内容为空，未执行粘贴"),
        "live_voice_readback_timeout" => Some("实时语音状态回读超时，请检查目标窗口"),
        "session_navigation_retired" => Some("会话切换已取消或失效"),
        "session_selection_confirmed" => Some("已确认目标会话"),
        "session_target_unavailable" => Some("目标会话当前不可用"),
        "session_slots_changed" => Some("会话列表已更新，请重新按键切换"),
        "session_navigation_busy" => Some("正在切换其他会话，请稍候"),
        "session_target_stale_or_unsupported" => Some("目标会话已失效或暂不支持切换"),
        "session_input_retired" => Some("会话切换输入已失效，请重试"),
        "session_worker_ended" => Some("会话切换状态未确认，请检查目标窗口"),
        "isolated_navigation_unavailable" => Some("隔离模式下不执行实体会话切换"),
        "session_catalog_unavailable" => Some("未能读取会话列表，请确认目标应用已打开"),
        code if code.starts_with("session_catalog_unavailable:") => {
            Some("未能读取会话列表，请确认目标应用已打开")
        }
        "session_instance_or_workspace_changed" => Some("目标应用窗口或工作区已变化"),
        "session_navigation_unsupported" => Some("暂不支持切换到此目标会话"),
        "no_active_conversation" => Some("当前未打开可归档的会话"),
        "conversation_running_cannot_archive" => Some("当前会话正在运行，无法直接归档"),
        "archive_control_unavailable" => Some("未找到当前会话的归档按钮，请先展开左侧栏"),
        "archive_conversation_readback" => Some("会话归档回读未确认，请检查左侧会话栏"),
        "sidebar_conversation_step_readback" => Some("侧栏会话切换回读未确认，请检查目标窗口"),
        "sidebar_conversations_unavailable" => Some("未找到左侧对话导航栏中的会话列表"),
        _ => None,
    };
    if let Some(zh) = known_zh {
        return tr(zh, locale).to_string();
    }
    if !saw_prefix {
        return tr(trimmed, locale).to_string();
    }
    let zh = if saw_unconfirmed {
        "动作回读未确认，请检查目标应用状态"
    } else {
        "目标应用当前无法执行该操作"
    };
    tr(zh, locale).to_string()
}

pub fn localize_managed_error_detail(raw: &str, locale: ResolvedLocale) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if locale == ResolvedLocale::En {
        if let Some(en) = en_lookup(trimmed) {
            return en.to_string();
        }
    }
    let joiner = match locale {
        ResolvedLocale::ZhHans => "；",
        ResolvedLocale::En => "; ",
    };
    if trimmed.contains("; ") {
        return trimmed
            .split("; ")
            .map(|seg| localize_managed_error_segment(seg, locale))
            .filter(|seg| !seg.is_empty())
            .collect::<Vec<_>>()
            .join(joiner);
    }
    if locale == ResolvedLocale::En && trimmed.contains('；') {
        return trimmed
            .split('；')
            .map(|seg| localize_managed_error_segment(seg, locale))
            .filter(|seg| !seg.is_empty())
            .collect::<Vec<_>>()
            .join(joiner);
    }
    localize_managed_error_segment(trimmed, locale)
}

pub fn relocalize_status_message(msg: &str, next_locale: ResolvedLocale) -> String {
    match next_locale {
        ResolvedLocale::ZhHans => {
            if let Some(detail) = msg.strip_prefix("会话未确认 · ") {
                return format!(
                    "会话未确认 · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::ZhHans)
                );
            }
            if let Some(detail) = msg.strip_prefix("已切换会话 · ") {
                return format!(
                    "已切换会话 · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::ZhHans)
                );
            }
            if let Some(detail) = msg.strip_prefix("会话未切换 · ") {
                return format!(
                    "会话未切换 · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::ZhHans)
                );
            }
            msg.to_string()
        }
        ResolvedLocale::En => {
            if let Some(en) = en_lookup(msg) {
                return en.to_string();
            }
            if let Some(rest) = msg
                .strip_prefix("已锁定方案「")
                .and_then(|s| s.strip_suffix('」'))
            {
                let name = mi_model::i18n::localize_profile_name(rest, ResolvedLocale::En);
                return format!("Locked profile \"{name}\"");
            }
            if let Some(rest) = msg
                .strip_prefix("已新建方案「")
                .and_then(|s| s.strip_suffix('」'))
            {
                let name = mi_model::i18n::localize_profile_name(rest, ResolvedLocale::En);
                return format!("Created profile \"{name}\"");
            }
            if let Some(rest) = msg
                .strip_prefix("已重命名为「")
                .and_then(|s| s.strip_suffix('」'))
            {
                let name = mi_model::i18n::localize_profile_name(rest, ResolvedLocale::En);
                return format!("Renamed to \"{name}\"");
            }
            if let Some(rest) = msg
                .strip_prefix("已复制方案「")
                .and_then(|s| s.strip_suffix('」'))
            {
                let name = mi_model::i18n::localize_profile_name(rest, ResolvedLocale::En);
                return format!("Duplicated profile \"{name}\"");
            }
            if let Some(path) = msg.strip_prefix("已导出诊断报告：") {
                return format!("Exported diagnostic report: {path}");
            }
            if let Some(sha1) = msg.strip_prefix("设备快照已核验 · keymap sha1=") {
                return format!("Device snapshot verified · keymap sha1={sha1}");
            }
            if let Some(rest) = msg
                .strip_prefix("设备身份已读取：")
                .and_then(|s| s.strip_suffix(" · 只读门禁生效"))
            {
                return format!("Device identity read: {rest} · Read-only guard active");
            }
            if let Some(rest) = msg.strip_prefix("已检测到 ") {
                return format!("Detected {rest}");
            }
            if let Some(rest) = msg.strip_prefix("已连接 ") {
                let localized = rest
                    .replace("USB 专用接口已独占", "USB vendor interface claimed")
                    .replace("已建立 HID 会话", "HID session connected")
                    .replace("自动验收通过", "auto-verified")
                    .replace("固件 ", "firmware ");
                return format!("Connected {localized}");
            }
            if let Some(layer) = msg
                .strip_prefix("板载三层已就绪 · 第 ")
                .and_then(|s| s.strip_suffix(" 层"))
            {
                return format!("Onboard 3-layer ready · Layer {layer}");
            }
            if let Some(key) = msg
                .strip_prefix("已按下 ")
                .and_then(|s| s.strip_suffix("（当前槽位未绑定动作）"))
            {
                return format!("Pressed {key} (slot unbound)");
            }
            if let Some(key) = msg
                .strip_prefix("已按下 ")
                .and_then(|s| s.strip_suffix("（已暂停软件按键映射）"))
            {
                return format!("Pressed {key} (software key mapping paused)");
            }
            if let Some(detail) = msg.strip_prefix("动作读回未确认 · ") {
                return format!(
                    "Action readback unconfirmed · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(detail) = msg.strip_prefix("已执行 · ") {
                return format!(
                    "Executed · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(detail) = msg.strip_prefix("未执行 · ") {
                return format!(
                    "Not executed · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(detail) = msg.strip_prefix("原目标释放未确认 · ") {
                return format!(
                    "Original target release unconfirmed · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(detail) = msg.strip_prefix("原目标听写停止未确认 · ") {
                return format!(
                    "Original dictation stop unconfirmed · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(detail) = msg.strip_prefix("宿主动作清理未确认 · ") {
                return format!(
                    "Host action cleanup unconfirmed · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(detail) = msg
                .strip_prefix("旧会话清理未确认 · ")
                .and_then(|s| s.strip_suffix("；宿主输入保持暂停"))
            {
                return format!(
                    "Previous session cleanup unconfirmed · {}; host input remains paused",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some((key, detail)) = msg
                .strip_prefix("已触发 ")
                .and_then(|s| s.split_once(" · "))
            {
                return format!(
                    "Triggered {key} · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some((key, detail)) = msg
                .strip_prefix("已释放 ")
                .and_then(|s| s.split_once(" · "))
            {
                return format!(
                    "Released {key} · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some((key, detail)) = msg
                .strip_prefix("未执行 ")
                .and_then(|s| s.split_once(" · "))
            {
                return format!(
                    "Not executed {key} · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(key) = msg
                .strip_prefix("正在按住语音 (")
                .and_then(|s| s.strip_suffix(")…"))
            {
                return format!("Holding push-to-talk ({key})…");
            }
            if let Some(detail) = msg.strip_prefix("听写未启动 · ") {
                return format!(
                    "Dictation not started · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(detail) = msg.strip_prefix("会话未切换 · ") {
                return format!(
                    "Session not switched · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(detail) = msg.strip_prefix("会话未确认 · ") {
                return format!(
                    "Session switch unconfirmed · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(detail) = msg.strip_prefix("已切换会话 · ") {
                return format!(
                    "Switched session · {}",
                    localize_managed_error_detail(detail, ResolvedLocale::En)
                );
            }
            if let Some(err) = msg
                .strip_prefix("打开辅助功能设置失败 (")
                .and_then(|s| s.strip_suffix(')'))
            {
                return format!("Failed to open Accessibility settings ({err})");
            }
            if let Some(target) = msg
                .strip_prefix("已弹出模型选择页 · 左右旋转选模型，短按确认 (")
                .or_else(|| msg.strip_prefix("已弹出模型选择页 · 左右旋转选模型，按右选模型力度 ("))
                .and_then(|s| s.strip_suffix(')'))
            {
                return format!(
                    "Model picker opened · Turn dial to select, tap to confirm ({target})"
                );
            }
            if let Some(target) = msg
                .strip_prefix("已打开搜索会话 · 上下旋转选择会话，短按确认 (")
                .and_then(|s| s.strip_suffix(')'))
            {
                return format!(
                    "Search conversations opened · Turn dial to select, tap to confirm ({target})"
                );
            }
            if let Some(target) = msg
                .strip_prefix("已打开 Subagent 选择 · 上下旋转选择，短按打开 (")
                .and_then(|s| s.strip_suffix(')'))
            {
                return format!(
                    "Subagent picker opened · Turn dial to select, tap to open ({target})"
                );
            }
            let fw = localize_firmware_message(msg, ResolvedLocale::En);
            if fw != msg {
                return fw;
            }
            let preview = localize_lighting_preview_message(msg, ResolvedLocale::En);
            if preview != msg {
                return preview;
            }
            let v070 = localize_v070_status_message(msg, ResolvedLocale::En);
            if v070 != msg {
                return v070;
            }
            let managed = localize_managed_error_detail(msg, ResolvedLocale::En);
            if managed != msg {
                return managed;
            }
            msg.to_string()
        }
    }
}

pub fn localize_lighting_preview_message(msg: &str, locale: ResolvedLocale) -> String {
    if locale == ResolvedLocale::ZhHans {
        return msg.to_string();
    }
    if let Some(en) = en_lookup(msg) {
        return en.to_string();
    }
    if let Some(err) = msg.strip_prefix("当前灯光配置无效，无法启动硬件预览：") {
        return format!("Invalid lighting configuration; cannot start hardware preview: {err}");
    }
    if let Some(err) = msg.strip_prefix("硬件灯光预览启动失败：") {
        return format!("Failed to start hardware lighting preview: {err}");
    }
    if let Some(err) = msg.strip_prefix("硬件灯光预览异常停止（未自动恢复原灯效）：")
    {
        return format!(
            "Hardware lighting preview stopped on error (original lighting not auto-restored): {err}"
        );
    }
    if let Some(input) = msg.strip_prefix("颜色格式无效（需为 #RRGGBB）：") {
        return format!("Invalid color format (expected #RRGGBB): {input}");
    }
    if let Some(input) = msg.strip_prefix("颜色十六进制无效：") {
        return format!("Invalid hex color: {input}");
    }
    msg.to_string()
}

pub fn localize_v070_status_message(msg: &str, locale: ResolvedLocale) -> String {
    if locale == ResolvedLocale::ZhHans {
        return msg.to_string();
    }
    if let Some(en) = en_lookup(msg) {
        return en.to_string();
    }
    if let Some(ver) = msg
        .strip_prefix("当前固件 ")
        .and_then(|s| s.strip_suffix(" 低于 0.7.0-rc.16，不支持硬件能力接口"))
    {
        return format!(
            "Firmware {ver} is older than 0.7.0-rc.16; hardware capability API unsupported"
        );
    }
    if let Some(layer) = msg
        .strip_prefix("板载层索引 ")
        .and_then(|s| s.strip_suffix(" 超出有效范围（仅支持第 1–6 层）"))
    {
        return format!("Onboard layer index {layer} out of range (only Layers 1–6 supported)");
    }
    if let Some(other) = msg.strip_prefix("不支持的摇杆硬件模式：") {
        return format!("Unsupported joystick hardware mode: {other}");
    }
    if let Some(ver) = msg
        .strip_prefix("已识别 ")
        .and_then(|s| s.strip_suffix(" 硬件能力，等待同步"))
    {
        return format!("Detected {ver} hardware capabilities, awaiting sync");
    }
    if let Some(ver) = msg
        .strip_prefix("0.7.0 硬件能力已同步（")
        .and_then(|s| s.strip_suffix('）'))
    {
        return format!("0.7.0 hardware capabilities synced ({ver})");
    }
    if let Some(rest) = msg
        .strip_prefix("板载层已同步至 P")
        .and_then(|s| s.strip_suffix(" 层"))
    {
        if let Some((p, l)) = rest.split_once(" · 第 ") {
            return format!("Onboard layer synced to P{p} · Layer {l}");
        }
    }
    if let Some(layer) = msg
        .strip_prefix("已切换当前板载层至第 ")
        .and_then(|s| s.strip_suffix(" 层（不写入闪存）"))
    {
        return format!("Switched onboard layer to Layer {layer} (RAM only)");
    }
    if let Some(rest) = msg
        .strip_prefix("方案切层联动已切至 P")
        .and_then(|s| s.strip_suffix(" 层（RAM 零擦写）"))
    {
        if let Some((p, l)) = rest.split_once(" · 第 ") {
            return format!("Profile sync switched to P{p} · Layer {l} (RAM only)");
        }
    }
    if let Some(mode) = msg.strip_prefix("已同步板载摇杆硬件模式为") {
        let mode_en = match mode {
            "硬件鼠标（MOUSE）" => "Hardware Mouse (MOUSE)",
            "硬件滚轮（SCROLL）" => "Hardware Scroll (SCROLL)",
            "原生通道（VENDOR）" => "Vendor Channel (VENDOR)",
            "四向扇区（RADIAL）" => "4-Way Sectors (RADIAL)",
            other => other,
        };
        return format!("Synced onboard joystick mode to {mode_en}");
    }
    if let Some(preset) = msg
        .strip_prefix("已更新硬件鼠标参数（")
        .and_then(|s| s.strip_suffix('）'))
    {
        return format!("Updated hardware mouse parameters ({preset})");
    }
    if let Some(dz) = msg
        .strip_prefix("已更新摇杆防漂移死区（")
        .and_then(|s| s.strip_suffix('）'))
    {
        return format!("Updated joystick deadzone ({dz})");
    }
    if let Some(name) = msg.strip_prefix("设备名称已更新为 ") {
        return format!("Device name updated to {name}");
    }
    if let Some(sa) = msg
        .strip_prefix("已更新 SA #")
        .and_then(|s| s.strip_suffix(" 独立灯效"))
    {
        return format!("Updated SA #{sa} per-key RGB");
    }
    if let Some(sa) = msg
        .strip_prefix("已恢复 SA #")
        .and_then(|s| s.strip_suffix(" 默认背景灯效"))
    {
        return format!("Restored SA #{sa} default background RGB");
    }
    msg.to_string()
}

pub fn localize_firmware_message(msg: &str, locale: ResolvedLocale) -> String {
    if locale == ResolvedLocale::ZhHans {
        return msg.to_string();
    }
    if let Some(en) = en_lookup(msg) {
        return en.to_string();
    }
    if let Some((err_part, tail)) = msg.split_once("；备份与操作记录：") {
        let err_en = localize_firmware_message(err_part, locale);
        if let Some((dir, save_err)) = tail.split_once("；记录保存失败：") {
            return format!(
                "{err_en}; backup and operation log: {dir}; failed to save log: {save_err}"
            );
        }
        return format!("{err_en}; backup and operation log: {tail}");
    }
    if let Some(ver) = msg.strip_prefix("官方预发布版：") {
        return format!("Official prerelease: {ver}");
    }
    if let Some(ver) = msg.strip_prefix("官方稳定版：") {
        return format!("Official stable release: {ver}");
    }
    if let Some(ver) = msg.strip_prefix("无法识别固件版本：") {
        return format!("Unrecognized firmware version: {ver}");
    }
    if let Some(err) = msg.strip_prefix("固件发布信息解析失败：") {
        return format!("Failed to parse firmware release metadata: {err}");
    }
    if let Some(err) = msg.strip_prefix("无法检查固件发布：") {
        return format!("Failed to check firmware releases: {err}");
    }
    if let Some(err) = msg.strip_prefix("固件请求失败：") {
        return format!("Firmware request failed: {err}");
    }
    if let Some(err) = msg.strip_prefix("固件目录解析失败：") {
        return format!("Failed to parse firmware catalog: {err}");
    }
    if let Some(err) = msg.strip_prefix("无法保存固件：") {
        return format!("Failed to save firmware: {err}");
    }
    if let Some(err) = msg.strip_prefix("无法保存完整固件：") {
        return format!("Failed to save complete firmware: {err}");
    }
    if let Some(err) = msg.strip_prefix("不能阻止更新期间休眠：") {
        return format!("Failed to prevent sleep during update: {err}");
    }
    if let Some(rest) = msg
        .strip_prefix("引导模式请求结果未确认：")
        .and_then(|s| s.strip_suffix("。请查找串口确认设备状态；不会自动重发。"))
    {
        return format!(
            "Bootloader request unconfirmed: {rest}. Scan ports to check device state; will not auto-retry."
        );
    }
    msg.to_string()
}

pub fn localize_macro_reference_entry(entry: &str, locale: ResolvedLocale) -> String {
    if locale == ResolvedLocale::ZhHans {
        return entry.to_string();
    }
    if entry == "未绑定" {
        return "Unbound".to_string();
    }
    if let Some((left, control)) = entry.rsplit_once(" · ") {
        if let Some((profile, layer)) = left.split_once(" / ") {
            return format!(
                "{} / {} · {control}",
                mi_model::i18n::localize_profile_name(profile, locale),
                mi_model::i18n::localize_layer_name(layer, locale)
            );
        }
    }
    tr(entry, locale).to_string()
}

fn localize_comparison_entity_en(entity: &str) -> String {
    if entity == "文档" {
        return "Document".to_string();
    }
    if let Some(p) = entity.strip_prefix("配置 ") {
        return format!(
            "Profile {}",
            mi_model::i18n::localize_profile_name(p, ResolvedLocale::En)
        );
    }
    if let Some(l) = entity.strip_prefix("层 ") {
        return format!(
            "Layer {}",
            mi_model::i18n::localize_layer_name(l, ResolvedLocale::En)
        );
    }
    if let Some(a) = entity.strip_prefix("动作 ") {
        return format!(
            "Action {}",
            mi_model::i18n::localize_action_label(a, ResolvedLocale::En)
        );
    }
    if let Some((layer, ctrl)) = entity.split_once(" / ") {
        return format!(
            "{} / {ctrl}",
            mi_model::i18n::localize_layer_name(layer, ResolvedLocale::En)
        );
    }
    en_lookup(entity).unwrap_or(entity).to_string()
}

fn localize_comparison_label_en(label: &str) -> String {
    if let Some(en) = en_lookup(label) {
        return en.to_string();
    }
    if let Some((context, field)) = label.rsplit_once(" · ") {
        let context_en = localize_comparison_entity_en(context);
        let field_en = match field {
            "名称" => "Name",
            "配置顺序" => "Profile order",
            "层顺序" => "Layer order",
            "控件顺序" => "Control order",
            "对象或绑定" => "Object or binding",
            other => en_lookup(other).unwrap_or(other),
        };
        return format!("{context_en} · {field_en}");
    }
    localize_comparison_entity_en(label)
}

fn localize_comparison_summary_en(summary: &str) -> String {
    if let Some(en) = en_lookup(summary) {
        return en.to_string();
    }
    if let Some(act) = summary.strip_prefix("本地动作：") {
        return format!(
            "Local action: {}",
            mi_model::i18n::localize_action_label(act, ResolvedLocale::En)
        );
    }
    if let Some(token) = summary.strip_prefix("按键 ") {
        return format!("Key {token}");
    }
    if let Some(count) = summary.strip_suffix(" 项（展开查看）") {
        return format!("{count} item(s)");
    }
    if summary.contains(" → ") {
        return summary
            .split(" → ")
            .map(localize_comparison_entity_en)
            .collect::<Vec<_>>()
            .join(" → ");
    }
    let act = mi_model::i18n::localize_action_label(summary, ResolvedLocale::En);
    if act != summary {
        return act;
    }
    let prof = mi_model::i18n::localize_profile_name(summary, ResolvedLocale::En);
    if prof != summary {
        return prof;
    }
    let layer = mi_model::i18n::localize_layer_name(summary, ResolvedLocale::En);
    if layer != summary {
        return layer;
    }
    localize_comparison_entity_en(summary)
}

fn localize_comparison_conflict_en(conflict: &str) -> String {
    if let Some(en) = en_lookup(conflict) {
        return en.to_string();
    }
    if let Some(rest) = conflict.strip_prefix("未知 raw 绑定变化：") {
        return format!(
            "Unknown raw binding change: {}",
            localize_comparison_label_en(rest)
        );
    }
    if let Some(label) = conflict.strip_suffix("：本地与设备变化不同") {
        return format!(
            "{}: local and device changes differ",
            localize_comparison_label_en(label)
        );
    }
    if let Some(err) = conflict.strip_prefix("引用校验：") {
        return format!("Reference check: {}", tr(err, ResolvedLocale::En));
    }
    conflict.to_string()
}

pub fn localize_comparison_modal(
    modal: &mut crate::editors::ReconnectComparisonModalProjection,
    locale: ResolvedLocale,
) {
    if locale == ResolvedLocale::ZhHans {
        return;
    }
    for row in &mut modal.rows {
        row.result_badge = match row.result_badge.as_str() {
            "共同值" => "Common".to_string(),
            "设备" => "Device".to_string(),
            "本地" => "Local".to_string(),
            "冲突" => "Conflict".to_string(),
            other => tr(other, locale).to_string(),
        };
        row.label = localize_comparison_label_en(&row.label);
        row.base_summary = localize_comparison_summary_en(&row.base_summary);
        row.local_summary = localize_comparison_summary_en(&row.local_summary);
        row.device_summary = localize_comparison_summary_en(&row.device_summary);
    }
    for conflict in &mut modal.conflicts {
        *conflict = localize_comparison_conflict_en(conflict);
    }
    if let Some(err) = &mut modal.error_message {
        *err = tr(err, locale).to_string();
    }
}

fn localize_apply_summary_en(summary: &str) -> String {
    if let Some(en) = en_lookup(summary) {
        return en.to_string();
    }
    if let Some((keys, rest)) = summary.split_once(" 个键位、") {
        if let Some(lights) = rest.strip_suffix(" 个灯光字段") {
            if let Some((joysticks, lights_only)) = lights.split_once(" 个摇杆模式、") {
                return format!(
                    "{keys} key(s), {joysticks} joystick mode(s), {lights_only} lighting field(s)"
                );
            }
            return format!("{keys} key(s), {lights} lighting field(s)");
        }
    }
    summary.to_string()
}

fn localize_apply_cell_en(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        if let Ok(inner) = serde_json::from_str::<String>(raw) {
            let localized = localize_comparison_summary_en(&inner);
            if let Ok(quoted) = serde_json::to_string(&localized) {
                return quoted;
            }
        }
    }
    localize_comparison_summary_en(raw)
}

pub fn localize_apply_modal(
    modal: &mut crate::editors::ApplyPreparationModalProjection,
    locale: ResolvedLocale,
) {
    if locale == ResolvedLocale::ZhHans {
        return;
    }
    modal.unavailable_reason = tr(&modal.unavailable_reason, locale).to_string();
    modal.before_summary = localize_apply_summary_en(&modal.before_summary);
    modal.after_summary = localize_apply_summary_en(&modal.after_summary);
    for row in &mut modal.changes {
        row.before = localize_apply_cell_en(&row.before);
        row.after = localize_apply_cell_en(&row.after);
        row.location_label = localize_comparison_label_en(&row.location_label);
    }
    if let Some(err) = &mut modal.error_message {
        *err = tr(err, locale).to_string();
    }
}

pub fn concise_keycap_caption_en(raw_label: &str) -> String {
    let trimmed = raw_label.trim();
    if trimmed.is_empty() || trimmed == "未绑定" || trimmed == "Unbound" {
        return "Unbound".to_string();
    }
    let localized = mi_model::i18n::localize_action_label(trimmed, mi_model::ResolvedLocale::En);
    let base = localized
        .split_once(" (")
        .map(|(l, _)| l.trim())
        .unwrap_or(localized.as_str());
    if base == "Built-in Browser · Layout" {
        return "Browser".to_string();
    }
    let segment = if let Some((left, right)) = base.split_once(" · ") {
        let left_trim = left.trim();
        let right_trim = right.trim();
        if left_trim.starts_with("Session ") || left_trim.starts_with("Slot ") {
            left_trim
        } else if !right_trim.is_empty() {
            right_trim
        } else {
            left_trim
        }
    } else {
        base
    };
    match segment {
        "Scroll Chat Up" => return "Scroll Up".to_string(),
        "Scroll Chat Down" => return "Scroll Dn".to_string(),
        "Scroll to Bottom" | "Bottom & Focus Input" => return "To Bottom".to_string(),
        "Lower Reasoning Effort" => return "Effort -".to_string(),
        "Raise Reasoning Effort" => return "Effort +".to_string(),
        "Toggle Fast Mode" => return "Fast Mode".to_string(),
        "New Session in Project" => return "New Sess".to_string(),
        "New Conversation" => return "New Chat".to_string(),
        "Gemini Live Voice" | "Live Voice" => return "Live Voice".to_string(),
        "Push-to-Talk Voice" | "Push to Talk" => return "Hold Talk".to_string(),
        "Stop Dictation & Submit" => return "Stop+Send".to_string(),
        "Approve Pending Action" => return "Approve".to_string(),
        "Reject / Interrupt" => return "Reject".to_string(),
        "Next Unread Completed" => return "Next Done".to_string(),
        "Previous Unread Completed" => return "Prev Done".to_string(),
        "Cycle Working / Attention" => return "Cycle Work".to_string(),
        "Focus Active Input" => return "Focus In".to_string(),
        "Search Conversations" => return "Search".to_string(),
        "Select Subagent" => return "Subagent".to_string(),
        "Open Model Picker" => return "Models".to_string(),
        "Previous Model / Effort" => return "Prev Model".to_string(),
        "Next Model / Effort" => return "Next Model".to_string(),
        "Confirm Selected Model" => return "Pick Model".to_string(),
        "Expand / Raise Model Effort" | "Select / Raise Model Effort" => {
            return "Effort +".to_string()
        }
        "Back to Models / Lower Effort" | "Lower Model Effort" => return "Effort -".to_string(),
        "Cancel Model Picker" => return "Cancel".to_string(),
        "Mouse Pointer Up" => return "Ptr Up".to_string(),
        "Mouse Pointer Down" => return "Ptr Down".to_string(),
        "Mouse Pointer Left" => return "Ptr Left".to_string(),
        "Mouse Pointer Right" => return "Ptr Right".to_string(),
        "Mouse Left Click" => return "Left Click".to_string(),
        "Mouse Right Click" => return "Right Clk".to_string(),
        "Mouse Middle Click" => return "Mid Click".to_string(),
        "Mouse Wheel Up" => return "Wheel Up".to_string(),
        "Mouse Wheel Down" => return "Wheel Down".to_string(),
        "Mouse Wheel Left" => return "Wheel Left".to_string(),
        "Mouse Wheel Right" => return "Wheel Rgt".to_string(),
        "Cycle Mouse Speed" | "Cycle Pointer Speed" => return "Ptr Speed".to_string(),
        "Built-in Browser" | "Open Built-in Browser" | "Open Browser" => {
            return "Browser".to_string()
        }
        "Toggle Sidebar" => return "Sidebar".to_string(),
        "Command Palette" => return "Palette".to_string(),
        "Switch Workspace" | "Project Workspace Switcher" => return "Workspace".to_string(),
        "Open Side Question" => return "Side Q".to_string(),
        "Open Right Panel" => return "Open Panel".to_string(),
        "Close Right Panel" => return "Close Panel".to_string(),
        _ => {}
    }
    if let Some(num) = segment.strip_prefix("Session ") {
        let digits: String = num.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return format!("Sess {digits}");
        }
    }
    if let Some(num) = segment.strip_prefix("Slot ") {
        let digits: String = num.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return format!("Slot {digits}");
        }
    }
    if segment.chars().count() <= 10 {
        return segment.to_string();
    }
    let words: Vec<&str> = segment.split_whitespace().collect();
    if words.len() >= 2 {
        let two = format!("{} {}", words[0], words[1]);
        if two.chars().count() <= 10 {
            return two;
        }
    }
    if let Some(first) = words.first() {
        if first.chars().count() <= 10 {
            return (*first).to_string();
        }
    }
    segment.chars().take(10).collect()
}

pub fn localize_session_title(title: &str, locale: ResolvedLocale) -> String {
    if locale == ResolvedLocale::ZhHans {
        return title.to_string();
    }
    match title {
        "Antigravity 后台重连中" | "Antigravity 后台断连" => tr(title, locale).to_string(),
        _ => title.to_string(),
    }
}

pub fn localize_session_status_detail(detail: &str, locale: ResolvedLocale) -> String {
    if locale == ResolvedLocale::ZhHans {
        return detail.to_string();
    }
    let translated = tr(detail, locale);
    if translated != detail {
        return translated.to_string();
    }
    if let Some(rest) = detail.strip_prefix("模型自动重试（第 ") {
        if let Some((attempt, after)) = rest.split_once(" 次）：") {
            return format!("Model retrying (attempt {attempt}): {after}");
        }
    }
    if let Some(rest) = detail.strip_prefix("等待模型响应已 ") {
        if let Some(dur) = rest.strip_suffix("（超 60s 无进展）") {
            return format!("Waiting for model response for {dur} (>60s stalled)");
        }
    }
    if let Some(rest) = detail.strip_prefix("正在重连 Antigravity 后台服务（") {
        if let Some(secs) = rest.strip_suffix("s）") {
            return format!("Reconnecting Antigravity daemon ({secs}s)");
        }
    }
    if let Some(rest) = detail.strip_prefix("Antigravity 后台守护进程无响应已 ") {
        if let Some(secs) = rest.strip_suffix('s') {
            return format!("Antigravity daemon unresponsive for {secs}s");
        }
    }
    if let Some(reason) = detail.strip_prefix("API 报错：") {
        return format!("API error: {reason}");
    }
    detail.to_string()
}
