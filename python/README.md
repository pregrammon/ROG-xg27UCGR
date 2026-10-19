# Python/tkinter 复刻版

对 `C:\Program Files (x86)\ASUS\DisplayWidgetCenter\DisplayWidgetCenter.exe`
（v1.5.1.3，SHA-256 `d9d6d043ff493e528a551e8b0ae9a0b31b60077ea2df66cce8a1da640882790f`）
做静态逆向后的最小可用实现：**亮度滑块 + 4K/1K 双模切换**。

只依赖 Python 标准库（tkinter、ctypes），不需要 .NET / WinUI / Windows App Runtime。

## 逆向证据（IL）

| 位置 | 证据 |
| --- | --- |
| `DisplayWidgetCenter.VCPAPI.SetVCPFeatureInternal` | 转发给 `DDCMethods.SetVCPFeature`（dxva2 / monitorconfigapi 的 DDC/CI 写） |
| `DisplayWidgetCenter.VCPAPI.SetFrameRateBoost` | `ldc.i4 253`（VCP `0xFD`），分支里 `ldc.i4 2048`（`0x800`）/ `ldc.i4 256`（`0x100`）作为位掩码 |
| `DisplayWidgetCenter.VCPAPI.SetToggleSettingsItem` | 对 `0xFD` 做 **read-modify-write**：`cur \| mask`（开）/ `cur & ~mask`（关） |
| `DisplayWidgetCenter.VCPAPI.GetVCPVersion` | 读 VCP `0xEF` 低字节作为版本，用于选择新/旧位掩码 |
| `DisplayWidgetCenter.DDCMethods` | `SetVCPFeature` / `GetVCPFeatureAndVCPFeatureReply`，即标准 DDC/CI |

仓库根目录的 C# 版（`Platform/RogScreenDualMode.cs`）已在 XG27UCGR 实机验证：
`0xFD = 0x128` → 1K 485Hz，`0xFD = 0x001` → 原码可读，本脚本沿用这组已验证值。

## 功能

- 亮度：标准 VCP `0x10` / `GetMonitorBrightness` / `SetMonitorBrightness`，0–100 映射到显示器真实范围。
- 双模：写 VCP `0xFD`，`0x128` ↔ `0x001`；切换后自动轮询分辨率（最多 12 秒），并重新打开物理句柄。
- 模式判断：读 Windows 当前分辨率（`3840×2160` / `1920×1080`），不依赖 VCP 回读（该显示器读 VCP 不稳定）。
- GUI：tkinter，后台线程做 DDC 调用，`queue` 回主线程，滑块 150ms 防抖。

## 用法

```powershell
python python\display_widget_center.py             # 启动 GUI
python python\display_widget_center.py --probe     # 只读探针，不写显示器
python python\display_widget_center.py --switch 1k # 命令行切到 1K（会短暂黑屏）
python python\display_widget_center.py --switch 4k # 命令行切回 4K
python python\display_widget_center.py --selftest  # 建窗 1.2s 自动关闭，验证 tkinter
```

## 实测（本机，DP 连接 XG27UCGR）

```
[OK] DLL            : dxva2.dll
[OK] 物理显示器     : 'Generic PnP Monitor'
[OK] 亮度范围       : 0..100
[OK] 当前亮度       : 77%
[OK] 当前分辨率     : 3840x2160 (4k)
[OK] VCP 0xFD 读取 : ok=True cur=0x0 max=0x103
[OK] VCP 0x10 读取 : ok=True cur=77 max=100
```

## 已知限制

- 只实现亮度 + 双模；DisplayWidgetCenter 的 GameVisual、HDR、色彩、固件升级等未复刻。
- 双模写入会触发面板重新握手（黑屏数秒），**默认不自动测试**；请用 GUI 或 `--switch` 自行验证。
- VCP `0xFD` 的回读值在该显示器上不可靠（`max=0x103` 明显异常），所以状态以分辨率判断。
- 仅针对 XG27UCGR 逆向，其他型号的 `0xFD` 位定义不保证一致。
