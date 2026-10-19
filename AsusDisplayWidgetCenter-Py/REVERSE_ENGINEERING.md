# DisplayWidgetCenter 逆向分析与 Python 复刻报告

分析对象：`C:\Program Files (x86)\ASUS\DisplayWidgetCenter`
主程序：`DisplayWidgetCenter.exe` **1.5.1.3**（14,619,272 字节，.NET Framework 4.7.2 / WPF）
分析日期：2026-10-19
分析环境：Windows 11 26100，ASUS **XG27UCGR**，NVIDIA RTX 5080
工具：ILSpy CLI 11.1.0（`ilspycmd`）、Python 3.14 + ctypes、原程序运行日志

---

## 1. 结论摘要

* 该应用的核心是 **VESA DDC/CI（MCCS）** 控制：通过 `dxva2.dll` 拿到每个物理显示器的
  `hPhysicalMonitor`，再用 `GetVCPFeatureAndVCPFeatureReply` / `SetVCPFeature`
  读写 VCP 命令；用 `gdi32!GetCapabilitiesStringLength` +
  `dxva2!CapabilitiesRequestAndCapabilitiesReply` 读取 MCCS 能力字符串。
* 所有“ASUS 专有功能”（GameVisual、滤蓝光、Shadow Boost、OverDrive、准星、OSD 遥控等）
  都有明确的 VCP 编号，绝大多数是标准 MCCS 命令加 ASUS 自定义取值，**不需要原版 DLL**。
* 机型系列（Gaming / MainStream / Portable / ProArt）由能力字符串里的 `model(...)` 前缀判断；
  同一功能在不同系列下使用不同的模式表、色温表和写入编码。
* 已在本机 XG27UCGR 上完成真机读写验证，Python 复刻版可以独立控制亮度、对比度、音量、
  滤蓝光、伽马、色温、GameVisual 模式、OSD 遥控、准星、计时器、FPS、对齐、静音、ASCR 等。
* 只有在线服务、AI 推理、HID 灯条/眼镜、固件升级、ProArt 校准报告等外围功能未复刻
  （见第 10 节），它们依赖服务器、私有硬件或原生 DLL，与显示器 DDC 控制无关。

---

## 2. 安装目录与模块构成

目录下共 75 个文件，可分为五类（观测）：

### 2.1 主程序与托管依赖

| 文件 | 作用 |
| --- | --- |
| `DisplayWidgetCenter.exe` (14.6 MB) | WPF 主程序，版本 1.5.1.3 |
| `DisplayWidgetCenter.exe.config` | .NET 4.7.2，大量程序集绑定重定向 |
| `CommunityToolkit.Mvvm.dll` 8.4 | MVVM 基础设施 |
| `DisplayIpc.dll` 1.0.0.0 | 命名管道 IPC：`MonitorCommandMessage`、`EventMessage`、`UserConfigMessage` 等 |
| `H.Pipes.dll` 15.0 / `NamedPipeWrapper.NetCore` | 管道封装 |
| `HidLibrary.dll` 3.2.49 / `ScreenLightBarHid.dll` | HID 设备（灯条等） |
| `Nefarius.ViGEm.Client.dll` | 虚拟手柄（Gamepad 功能） |
| `SharpDX.DirectInput/XInput` | 手柄输入 |
| `ScottPlot` / `PdfSharp` | 校准报告图表与 PDF |
| `MdXaml` / `AvalonEdit` | Markdown 显示 |
| 本地化：`cs-CZ … zh-TW\DisplayWidgetCenter.resources.dll` | 22 种语言的 WPF 资源 |

### 2.2 原生 DLL（非托管，ILSpy 无法反编译）

| 文件 | 观测到的导出/字符串 | 推断作用 |
| --- | --- | --- |
| `AsusDisplayMonitorInfo.dll` (285 KB) | `Monitor::getCurrentSetting`、`getPossibleSettings`、`getEdidRawData`、`getDDCHandler`、`getModelName`、`getSerialNumber`、`Win32MonitorHal` | 显示器 EDID/DDC 的 C++ 封装 |
| `AsusDisplayMath_x64/x86.dll` | 无 CLR 头 | 颜色/伽马数学运算 |
| `AsusDisplayTracker.dll` | 无 CLR 头 | 显示状态跟踪 |
| `ASUSLogHelper.dll` | 无 CLR 头 | 日志 |
| `EncryptoCore.dll` | 无 CLR 头 | 日志/配置加密（`EncryptedLogs.log` 2.4 MB） |
| `AirVisionGpuResolver.exe` | 无 CLR 头 | 独显/核显绑定解析 |

### 2.3 配置与资源

* `AppConfig\DisplayModeCapability_{Gaming,MainStream,Portable,ProArt,Esport}`：
  每个 GameVisual 模式支持哪些滑杆/开关（`Brightness`、`Contrast`、`Saturation` …）。
* `runtimes\win-{x64,x86,arm64}`：WebView2、SQLite 等本地运行时。
* `zh-CN\DisplayWidgetCenter.resources.dll`：中文界面字符串。

### 2.4 日志与用户数据

* `%ProgramData%\ASUS\DisplayWidgetCenter\log\AppEventLogs.log`：
  明文统计日志，可直接看到 `ColorTemp,5,6500K`、`GameVisual,Racing`、`Input Source,DisplayPort 1`。
* `%ProgramData%\ASUS\DisplayWidgetCenter\UserConfig\CapabilityData.json`：
  按型号+序列号缓存 MCCS 能力字符串（原版自己的缓存机制）。
* `%ProgramData%\ASUS\DisplayWidgetCenter\HotKey\hotkey.json`：快捷键配置。

---

## 3. 托管代码结构

ILSpy 以 project 模式反编译主程序，得到 **341 个 `.cs` 文件、约 3.28 MB 源码**
（WPF 的 XAML 页面、ViewModel、P/Invoke、业务逻辑都在其中）。

关键类：

| 类 | 规模 | 作用 |
| --- | --- | --- |
| `VCPAPI` | 2,234 行 | 所有 VCP 读写方法、模式/色温/输入源映射 |
| `DisplayManager` | 3,377 行 | 枚举显示器、读能力字符串、解析支持项、状态同步 |
| `GameVisualPage` | 16,181 行 | 显示与色彩主页面（模式、滑杆、开关） |
| `MainWindow` / `TabPage` | 90 KB / 55 KB | 主窗口与左侧导航 |
| `CapabilityConfigManager` | 4,206 行 | 把能力字符串翻译成“是否支持某功能” |
| `GamePlusPage` / `EyeCarePage` / `OSDControlPage` | 1,088 / 883 / 2,412 行 | 游戏辅助、护眼、OSD |
| `ImportExportManager` | 570 行 | 配置导入/导出及其 VCP 白名单 |
| `HotKeyClass` / `KeyBoardHook` | 1,094 / 270 行 | 全局快捷键（低级键盘钩子） |

---

## 4. DDC/CI 调用链（观测）

`DDCMethods.cs` 中的 P/Invoke（原文件 18–56 行）：

```csharp
[DllImport("dxva2.dll")] GetNumberOfPhysicalMonitorsFromHMONITOR
[DllImport("dxva2.dll")] GetPhysicalMonitorsFromHMONITOR
[DllImport("dxva2.dll")] GetVCPFeatureAndVCPFeatureReply(hMonitor, code, pvct, ref cur, ref max)
[DllImport("dxva2.dll")] SetVCPFeature(hMonitor, code, value)
[DllImport("dxva2.dll")] GetCapabilitiesStringLength(hMonitor, ref len)
[DllImport("dxva2.dll")] CapabilitiesRequestAndCapabilitiesReply(hMonitor, sb, len)
```

`VCPAPI` 的并发与节流（`VCPAPI.cs` 68、76–159 行）：

```csharp
private static int Set_Time = 20;          // 每次调用前 sleep 20 ms
private static object _lock = new object(); // 全局串行
...
Thread.Sleep(Set_Time);
flag = DDCMethods.SetVCPFeature(hMonitor, dwVCPCode, dwNewValue);
```

`DisplayManager.GeCapability`（约 1380–1503 行）会：

1. 先查 `CapabilityData.json` 缓存；
2. 缓存未命中或固件号变化时，最多重试 20 次读取能力字符串；
3. 用 `GetModelNameFormCapability` 从 `model(...)` 中取型号；
4. 解析失败且型号为 ProArt / PG49WCD / PG34WCDM / PG32UCDM / XG27ACDNG 时使用内置字符串兜底。

能力字符串解析（`DisplayManager.NewCapabilityParserMethod` / `ParseString`，1505–1544 行）：
先截取 `vcp(` 到 `mswhql` 之间的内容，再用正则
`(\w+)(?:\(([^)]+)\))?` 展开为 `{ "10": [], "14": ["03","04",...], ... }`。

Python 复刻的 `vcp.parse_capabilities()` 使用同样的“括号深度扫描 + 正则”策略，
并保留 `13(0x0D)` 这类十六进制值的完整列表。

---

## 5. VCP 命令映射（从 `VCPAPI.cs` 逐方法提取）

| 方法 | VCP | 语义 / 取值 |
| --- | --- | --- |
| `SetRestoreFactoryDefaults` | `0x04` | 写 1：恢复全部出厂 |
| `SetRestoreColorFactoryDefaults` | `0x08` | 写 1：恢复颜色出厂 |
| `SetCaliCtrlOControlMode` | `0x0F` | 校准控制 |
| `SetBrightness` | `0x10` | 0–100 |
| `SetContrast` | `0x12` | 0–100 |
| `SetColorPreset` / 色温 | `0x14` | 3=4000K, 4=5000K, 5=6500K, 6=7500K, 7=8200K, 8=9300K, 9=10000K, 11=User, 14=5500K, 15=P3-Theater, 17=M Model |
| `SetRed/Green/BlueGain` | `0x16/0x18/0x1A` | 0–100（高 16 位可带回读默认值） |
| `SetColorWeakness` | `0x49` | 色弱模式 |
| `SetRedSaturation` … | `0x59–0x5E` | 红/黄/绿/青/蓝/洋红六轴饱和度 |
| `SetInputSource` | `0x60` | 15=DP1, 16=DP2, 17=HDMI1, 18=HDMI2, 26=USB-C1 … |
| `SetAudioVolume` | `0x62` | 0–100 |
| `SetBacklightLevelWhite` | `0x6B` | ProArt 背光 |
| `SetRed/Green/BlueOffset` | `0x6C/0x6E/0x70` | RGB 偏移 |
| `SetGamma` | `0x72` | ASUS 16 位值：0x5000=1.8, 0x6400=2.0, 0x7800=2.2, 0x8C00=2.4, 0xA000=2.6 |
| `SetSharpness` | `0x87` | 0–100 |
| `SetSaturation` / `SetHue` | `0x8A` / `0x90` | 0–100 |
| `SetOSDLanguage` | `0xCC` | 1=繁中, 2=EN, 13=简中, 39=ID … |
| `SetPowerMode` | `0xD6` | 1=开, 4=待机, 5=关 |
| `SetDisplayMode` | `0xDC`（ProArt 为 `0xE3`） | GameVisual 模式索引 |
| `SetOverDrive` | `0xE0` | 0–20（机型而定） |
| `SetPowerSaving` | `0xE1`（旧机型 `0xEE`） | 0/1 |
| `SetHDRCurve` | `0xE2`（旧 `0xF8`） | HDR 曲线/提醒 |
| `SetCrosshair` | `0xE3` | 旧式 1–6；新式 7–12 |
| `SetGamePlusTimer` | `0xE4` | 0=关, 1=30, 2=40, 3=50, 4=60, 5=90 分钟 |
| `SetShadowBoost` | `0xE5` | 0=关, 1–3 级, 4=动态 |
| `SetBlueLightFilter` | `0xE6` | 0–4（5 档） |
| `SetDisplayAlignment` | `0xE7` | 0/1 |
| `SetGamePlusPosition` | `0xE8` | 1–4 |
| `SetFPSCounter` | `0xEA` | 0=关, 1=数字, 2=柱状图 |
| `SetEzOSD` | `0xEB` | 0=关闭, 1=显示, 2=上, 3=下, 4=右, 5=左, 6=确认, 7=返回, 8=输入选择, 9=QuickFit, 10/11=快捷键, 12=自校准 |
| `SetReset` | `0xEC` | 重置 |
| `SetRestReminder` | `0xED` | 0=关, 1–12（5–60 分钟） |
| `SetOLEDReminder` | `0xF8` | OLED 提醒 |
| `SetShortcuts` | `0xFB` | OSD 快捷键 |
| `SetAudioMute`（位域） | `0xFC` bit 3 | 1=静音 |
| `SetVRR/SetASCR/SetSniper/OLED…` | `0xFD` | bit0 VRR、bit1 ASCR、bit2 Sniper、bit3 OLED Saver、bit7 ClearPixel、bit8 FrameRateBoost、bit9 AI Visual、bit11–14 OLED/Ambient |
| `GetVCPVersion` | `0xEF` | max 高字节=版本、低字节=修订 |
| `GetFWVersion` | `0xFF` | max=固件号，current 低字节=Panel ID |

> 注：CSV 表中的十进制在 `VCPAPI.cs` 中以 `16u`、`220u` 等写法出现；
> 上表已统一为十六进制。`IsASUSVendor` 还列出了 ASUS 私有段
> （0x55–0x58、0x66–0x69、0x77–0x7A 等），本复刻未使用。

### 5.1 模式写入的特殊编码

`SetDisplayMode`（`VCPAPI.cs` 446–478 行）：

```csharp
if (productLine == ProductLine.ProArt) {
    if (value 不是 3840/3841/3842/4096/4097/4098/4607/4863/4867/4868/5120/5121/5122
        && value < 256)
        value = (value << 8) | 0xFF;
    return SetVCPFeatureInternal(hMonitor, 227u, value);   // 0xE3
}
if (Gaming && isSRGBCalibration && value == 3) value = 10;
return SetVCPFeatureInternal(hMonitor, 220u, value);       // 0xDC
```

### 5.2 GameVisual 模式表

* **Gaming**：1 Cinema、2 Scenery、3 sRGB、4 User、5 Racing、6 RTS/RPG、7 FPS、8 MOBA、10 sRGB Cal。
* **MainStream / Portable**：1 Theater、2 Scenery、3 sRGB、4 Standard、5 Game、6 NightView、7 Reading、8 Darkroom。
* **ProArt**：1 Native、5 sRGB、8 AdobeRGB、9 Rec.2020、10 DCI-P3、11 DICOM、14 Rec.709、
  22/23/30 User1–3、31 Display P3、32 M Model P3，以及 HDR PQ/HLG/Dolby 组合值
  （3840–3842、4096–4098、4352/4607、4608/4863、4867/4868、5120–5122）。

### 5.3 色温表

* Gaming（非 VG）：4000/5000/6500/7500/8200/9300/10000K + User。
* Gaming VG 与 MainStream：Cool(8)、Normal(6)、Warm(5)、User(11)。
* ProArt：5000、5500、6500、7500、9300、P3-Theater(15)、M Model(17)。

---

## 6. 机型系列识别（移植 `Utils.GetProductLine`）

```csharp
if (model 含 "_D" 或 "gcd01") return Gaming;
if (model 含 "R1")           return Glasses;
if (model 含 "BenQ"/"Generic" 或长度 < 5) return NONE;
if (model 含 "ALB")          return LightBar;
prefix = 前两个字符
MB/MQ -> Portable
VX/VP/VY/VZ/VU/VA/VT/BE -> MainStream
PA/PQ -> ProArt
VG/PG/XG 或含 "_D" -> Gaming
```

能力字符串中的型号会先去掉 `ASUS` 前缀和空格（`GetModelNameFormCapability`）。
本机 XG27UCGR → `xg` → Gaming，与实测一致。

---

## 7. 能力探测与“支持项”判断

原版不是简单地“有这个 VCP 就启用”，而是把能力字符串展开为字典后，
由 `CapabilityConfigManager` 按功能逐项判断。例如：

* **ASCR**（`GetASCRConfig`，951–987 行）：要求 `vcp` 里存在 `fd`，
  且其第一个值（单字节取 `[0]`，双字节取 `[1]`）的 bit1 为 1。
* **静音**（`GetAudioMuteConfig`，998–1034 行）：要求 `fc` 存在且 bit3 为 1。
* **省电**（`GetPowerSavingConfig`）：只要求 `e1` 存在。
* **滤镜/OD/GamePlus** 等也各有对应子键或型号判断。

本复刻做了等价但更宽松的处理：
`code ∈ caps.vcp` 或实测 `max > 0` 即视为支持；UI 仍会在写入失败时给出错误状态。

---

## 8. 特殊行为与坑

### 8.1 伽马回读

设置时用 16 位 `0x5000 + n*0x1400`，回读时原版取高字节
（`GameVisualPage.xaml.cs` 3351–3398 行）：

```csharp
currentLocal = (currentLocal & 0xFF00) >> 8;
80 -> 1.8, 100 -> 2.0, 120 -> 2.2, 140 -> 2.4, 160 -> 2.6
```

### 8.2 准星新旧类型

`CheckCrosshairTypeFromDictionary` 判断 `e3` 是否含 `07` 或 `0C`：

* 新式：只显示 Off + 7 蓝点、8 绿点、9 蓝色目标环、10 绿色目标环、11 蓝色目标十字、12 绿色目标十字。
* 旧式：Off + 1 红点、2 绿点、3 红环、4 绿环、5 红十字、6 绿十字。

本机能力字符串为 `E3(00 07 08 09 0A 0B 0C 0D 0E 0F)`，属于新式。
实测写入 1 会被固件钳制为 7，**这也验证了该判断的必要性**。

### 8.3 模式切换后的联动

`GameVisualPage` 在切换模式后会重新读取滤镜、色温、伽马等值，
因为显示器会按模式恢复自己的预设；复刻版在写模式后延迟 350 ms 刷新。

### 8.4 配置文件白名单

`ImportExportManager` 中 Gaming 的导出 VCP 白名单（14–20 行）：

```
220, 226, 20, 230, 16, 18, 22, 24, 26, 89, 90, 91, 92, 93, 94, 95,
98, 114, 135, 138, 144, 197, 204, 224, 225, 229, 238, 248, 249, 252,
227, 234, 253
```

复刻版的 `PROFILE_CODES` 取其中与 UI 相关的子集，并额外包含 0x59–0x5E 六轴饱和度。

---

## 9. 真机验证（ASUS XG27UCGR）

能力字符串摘要（`python cli.py probe`）：

```
model(XG27UCGR)  mccs_ver(2.0)  60 个 VCP
VCP 版本 2.18    固件 1.2D (panel 0x10)
VCP 10 亮度 = 77/100      VCP 12 对比度 = 80/100
VCP 14 色温 = 5 (6500K)   VCP 60 输入 = 15 (DisplayPort 1)
VCP 62 音量 = 50/100      VCP E6 滤蓝光 = 0/4
VCP 72 伽马 = 0x7800 (2.2) VCP E0 OverDrive = 19/20
VCP FC = 0x0811           VCP FD = 0x0000
```

`python cli.py test-write` 的实测结果（全部 True，且均恢复）：

| 项目 | 写入 | 回读 | 恢复 |
| --- | --- | --- | --- |
| 亮度 | 77→78 | 78 | 77 |
| 对比度 | 80→81 | 81 | 80 |
| 音量 | 50→51 | 51 | 50 |
| 滤蓝光 | 0→1 | 1 | 0 |
| OverDrive | 19→20 | 20 | 19 |
| 伽马 | 0x7800→0x8C00 | 0x8C00 | 0x7800 |
| GameVisual | 4→7 | 7 | 4 |
| 色温 | 5→6 | 6 | 5 |

额外验证：

| 项目 | 结果 |
| --- | --- |
| ASCR（0xFD bit1） | 0x0000 → 0x0002 → 0x0000 |
| 静音（0xFC bit3） | 0x0811 → 0x0819 → 0x0811 |
| OSD 语言 0xCC | 13 → 2 → 13 |
| 准星 0xE3 | 0 → 7 → 0（写 1 被钳制为 7，确认新式准星） |
| 计时器 0xE4 | 0 → 1 → 0 |
| FPS 0xEA | 0 → 1 → 0 |
| 对齐 0xE7 | 0 → 1 → 0 |

启动耗时：能力字符串冷读约 **3.85 s**，用缓存后整机探测约 **1.41 s**
（14 个常用 VCP + 第二块无 DDC 虚拟屏）。复刻版据此实现了两阶段探测 + 能力缓存。

---

## 10. Python 复刻架构与取舍

### 10.1 架构

```
tkinter App
  ├─ MonitorManager ── MonitorState ── ddc.MonitorHandle
  │                       ├─ caps: CapabilityInfo
  │                       ├─ values: {VCP: VcpValue}
  │                       └─ probe() / probe_remaining() / write()
  ├─ ThreadPoolExecutor(1)  ← 所有 DDC 调用串行
  ├─ queue.Queue            ← worker → UI 线程回调
  ├─ HotkeyManager(thread)  ← RegisterHotKey + GetMessage
  └─ store                  ← settings/profiles/caps cache/autostart
```

* `ddc.py` 只做 ctypes 与句柄生命周期。
* `controller.py` 负责状态、探测、配置捕获/应用。
* `app.py` 的页面只读写 `MonitorState`，不直接碰 Win32。
* UI 线程永不调用 DDC；`run()` 把任务投递到单线程池，回调经队列回到 Tk。
* 两阶段探测：`FAST_CODES`（14 个）先出界面，`probe_remaining()` 后台补齐。
* 能力字符串按 `设备名|描述` 缓存 7 天，避免每次冷读 3–4 秒。

### 10.2 从原版移植的规则

* `Utils.GetProductLine` 的完整分支；
* Gaming / MainStream / Portable / ProArt 四套 GameVisual 模式表；
* Gaming 普通/VG、ProArt 三套色温表；
* 伽马 16 位值与高字节回读；
* ProArt 模式 `(value << 8) | 0xFF` 编码；
* 准星新旧类型判断；
* ASCR/静音位域；
* 配置文件 VCP 白名单；
* 20 ms 级 DDC 节流与全局锁（本实现取 32 ms，更保守）。

### 10.3 未复刻的功能及原因

| 功能 | 原因 |
| --- | --- |
| AI Game Visual / AI Advisor | 依赖本地 ONNX 模型、云端推理与 `DisplayWidget.AIGameVisual.dll`，不属于 DDC 控制 |
| App Sync | 依赖进程/窗口检测与后台服务 |
| Aura Light Bar / R1 眼镜 | 依赖 HID 协议与私有 `ScreenLightBarHid.dll` |
| 固件升级 | 依赖 ASUS 服务器、加密包与私有协议，风险高 |
| ProArt 校准报告 | 依赖 `PdfSharp`、色度计与专有校准流程 |
| PIP/PBP、KVM、OLED 专项 | 机型相关，VCP 语义未在本机验证 |
| 账号、云事件、自动更新 | 纯在线服务 |
| ICC 安装 | 调用 `ICCProfileInstaller.exe`，与显示控制无关 |

### 10.4 已知限制

* 非 DDC 显示器会显示“不支持 DDC/CI”，这是预期行为。
* 部分 VCP 在 HDR/sRGB 模式下被固件锁定，写入可能返回 True 但值不变；复刻版会以回读刷新 UI。
* 能力字符串缓存按设备名，不按 EDID 序列号；换同接口的另一台显示器时，
  “重新扫描”会强制重读（`rescan(force_caps=True)`）。
* 未实现原版的低级键盘钩子，改用 `RegisterHotKey`，不能组合鼠标侧键/媒体键。
* 所有界面尺寸按 96 DPI 设计，进程保持 DPI-unaware 交给 Windows 位图缩放；
  在 4K/200% 下会略糊，但布局稳定。

---

## 11. 复现步骤

```bat
:: 1. 反编译主程序
ilspycmd "C:\Program Files (x86)\ASUS\DisplayWidgetCenter\DisplayWidgetCenter.exe" -p -o C:\RE\DWC_exe

:: 2. 查看 VCP 映射
findstr /n "SetVCPFeatureInternal(hMonitor" C:\RE\DWC_exe\DisplayWidgetCenter\VCPAPI.cs

:: 3. 真机探测
python cli.py probe
python cli.py dump

:: 4. 安全写入验证（自动恢复）
python cli.py test-write

:: 5. 启动复刻 UI
python main.py
```

---

## 12. 证据类型说明

* **观测**：反编译源码（341 个文件）、P/Invoke 签名、原版日志、真机 VCP 读写结果。
* **推断**：机型系列与模式的对应关系、原生 DLL 的作用、部分私有 VCP 的语义；
  其中机型/模式/色温/伽马/准星/位域均已用真机写入验证。
* **未知**：`0xE9`、`0xF2`、`0xFB` 低位、`0xFD` 高位在 XG27UCGR 上的确切语义；
  `AsusDisplayMonitorInfo.dll` 内部实现；AI/云服务协议；不同固件的边界行为。

本报告和复刻代码仅用于互操作、学习与自有硬件控制；ASUS、ROG、ProArt 等名称与
原程序版权归 ASUSTeK Computer Inc. 所有。
