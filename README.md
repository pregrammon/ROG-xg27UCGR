# ROG-xg27UCGR（绝神27二代 屏幕桌面控制）

针对 **ASUS ROG Strix XG27UCGR（绝神27二代）** 的 Windows 桌面挂件：
把显示器的 **DDC/CI 亮度调节** 和 **4K / 1K 双模切换** 放在一个常驻桌面的小控件里，
不用再开 ASUS DisplayWidget Center。

仓库里有两套各自独立、功能对等的实现：

| 实现 | 位置 | 技术栈 | 说明 |
| --- | --- | --- | --- |
| C# 版 | 仓库根目录 | .NET 10 + WinUI 3（Windows App SDK） | 主实现，XAML 界面 |
| Rust 版 | `rust/` | eframe / egui 0.29 | 轻量移植版，单个 exe 可直接运行 |

两套实现共用同一份配置文件（`%APPDATA%\RogControl\settings.json`），
界面行为、托盘菜单、位置记忆方式都保持一致。

---

## 功能

- **亮度调节**：通过 DDC/CI（物理显示器 API 的 `SetMonitorBrightness`，即 VCP 0x10）
  调节主显示器亮度，滑块 0–100%，右侧实时显示百分比。
- **双模切换**：一键在 `4K@162Hz` 与 `1920x1080@485Hz` 之间切换（见下方原理）。
- **无边框透明挂件**：窗口无标题栏、背景全透明，只有红色描边和控件内容；
  可拖动、支持八方向缩放（边缘/四角），标题栏图钉按钮可**锁定位置**。
- **桌面层挂载**：窗口被挂到 Explorer 桌面图标层（`SHELLDLL_DefView`）下，
  浮在壁纸之上、所有应用窗口之下 —— 打开任何软件都会盖住它，点击也不会抢焦点。
- **托盘菜单**：窗口透明 / 高斯模糊（两者互斥）/ 开机自启动 / 退出。
- **单实例**：重复启动只会把已运行的窗口拉到前台，不创建第二个进程。
- **位置与尺寸记忆**：保存到 `%APPDATA%\RogControl\settings.json`；
  除绝对坐标外还额外记录**相对主屏分辨率的比例**，
  因此 4K ↔ 1K 切换（分辨率变化）后控件仍停在相同的视觉位置、按比例缩放。
- **隐藏于 Alt+Tab**，带失败状态提示（`StatusText`），失败时可通过托盘菜单重试。

## 双模切换原理

切换不是通过 Windows 显示设置完成的，而是直接向显示器写入 DDC/CI 的**厂商私有 VCP 0xFD**
（一个位掩码寄存器）。命令通过逆向 ASUS DisplayWidget Center 得到：
`SetFrameRateBoost → SetToggleSettingsItem` 会写 VCP `0xFD`，
其中 **bit 8（0x100）= FrameRateBoost（双模开关）**，
**bit 0（0x001）= VRR** —— 两者互斥，必须同时处理。

| 目标模式 | 写入 VCP 0xFD | 结果 |
| --- | --- | --- |
| 1K（像素合并） | `0x128`（296） | 面板黑屏重新握手，EDID 更新，系统变为 `1920x1080 @ 485Hz` |
| 4K（恢复） | `0x001`（1） | 回到 `3840x2160 @ 162Hz`，VRR 恢复 |

写入本身完全不触碰 Windows 显示栈：面板自己完成 EDID 切换，操作系统随后自动跟随。
切换瞬间的黑屏是重新协商链路导致的，属正常现象。

当前模式判断用的是 **Windows 当前分辨率**（3840x2160 / 1920x1080），
而不是回读 VCP —— 该显示器的 VCP 读取经常失败并返回垃圾值，但分辨率在重握手后总是稳定的。

## 目录结构

```
.
├─ App.xaml / App.xaml.cs               # C# 版入口，单实例逻辑
├─ MainWindow.xaml / .xaml.cs           # 主窗口：无边框透明、拖动/缩放、托盘、状态持久化
├─ Controls/
│   └─ ROGScreenWidgetContent.xaml(.cs)  # 亮度滑块 + 双模开关控件
├─ Platform/
│   ├─ RogScreenNativeMethods.cs        # monitorconfigapi.dll / dxva2.dll 的 P/Invoke
│   ├─ RogScreenDualMode.cs             # VCP 0xFD 双模切换
│   └─ Win32DesktopLayer.cs             # 挂载到 Explorer 桌面层
├─ ResizeThumb.cs                       # 自定义缩放热区
├─ app.manifest                         # 应用清单（DPI 感知等）
├─ RogControl.csproj                    # C# 版工程文件
├─ installer/
│   └─ RogControl.iss                   # Inno Setup 6 打包脚本
└─ rust/                                # Rust 移植版
    ├─ .cargo/config.toml               # +crt-static，静态链接 MSVC 运行库
    ├─ Cargo.toml / Cargo.lock
    └─ src/
        ├─ main.rs                      # 入口：单实例 + 无边框透明窗口
        ├─ app.rs                       # 主界面、拖动/缩放、状态持久化
        ├─ ddc.rs                       # DDC/CI 亮度（monitorconfigapi.dll / dxva2.dll）
        ├─ dual_mode.rs                 # VCP 0xFD 双模切换
        ├─ win32.rs                     # 窗口样式、桌面层挂载等 Win32 细节
        ├─ tray.rs                      # 托盘图标与菜单（红色 "R" 图标由字体实时生成）
        └─ config.rs                    # settings.json 读写
```

## 环境要求

- Windows 10 2004（build 19041）及以上 / Windows 11，**x64**
- 显示器通过 **DisplayPort** 连接（双模切换依赖 DP 链路重协商）
- **C# 版**：.NET 10 SDK；运行需要 .NET 10 桌面运行时 + Windows App Runtime
- **Rust 版**：Rust stable（MSVC 工具链，`x86_64-pc-windows-msvc`）；运行时无需额外依赖
- **打包安装程序**：Inno Setup 6

## 编译与运行

### C# 版（WinUI 3）

```powershell
# 调试运行（产物：bin\x64\Debug\net10.0-windows10.0.22621.0\RogControl.exe）
dotnet build -c Debug -p:Platform=x64

# Release 发布（输出到 bin\x64\Release\<tfm>\win-x64\publish）
dotnet publish RogControl.csproj -c Release -p:Platform=x64
```

> **注意**：`dotnet publish` 不会为 WinUI 3 非打包应用复制编译后的 XAML（`.xbf`）文件，
> 直接用发布的 exe 会黑屏并崩溃（stowed exception `0xc000027b`）。
> 工程文件里的 `CopyXbfToPublish` target 就是为了补这一步，改发布流程时别删掉。
>
> Release 配置刻意**不**内嵌 WinAppSDK 运行时（`WindowsAppSDKSelfContained=false`），
> 目标机器需要预先安装 Windows App Runtime，这样发布体积更小。

### Rust 版

```powershell
cd rust
cargo run --release          # 直接运行
cargo build --release        # 产物：rust\target\release\rogcontrol.exe
```

`rust/.cargo/config.toml` 启用了 `+crt-static`，把 MSVC C 运行库静态链接进 exe，
因此产物只依赖 Windows 自带的 DLL，真正做到"解压即用"。

### 打包安装程序

先完成上面的 Release 发布，再执行：

```powershell
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" installer\RogControl.iss
```

输出为 `artifacts\RogControl-Setup.exe`（LZMA2 压缩，简体中文向导，可选开机自启动）。
安装程序会检测目标机器的 .NET 10 桌面运行时并提示缺失。

## 配置文件

`%APPDATA%\RogControl\settings.json`

| 字段 | 含义 |
| --- | --- |
| `x` / `y` / `w` / `h` | 控件绝对位置与尺寸 |
| `fx` / `fy` / `fw` / `fh` | 相对主屏分辨率的比例（0.0–1.0），分辨率变化后据此还原 |
| `ppp_per_px` | 记录时每像素点数 ÷ 屏宽，用于在 4K/1K 缩放变化后保持逻辑尺寸不变 |
| `transparent` / `blur` | 背景模式（两者互斥） |
| `pinned` | 是否锁定位置 |

Rust 版还会把诊断信息写入 `%APPDATA%\RogControl\debug.log`
（因为 GUI 子系统下没有控制台）。

## 已知限制

- 双模切换的 VCP 位掩码是针对 **XG27UCGR** 逆向得到的，其他型号不保证可用。
- 该显示器 VCP **读取**经常失败，因此模式判断依赖分辨率而非回读 VCP。
- 亮度/双模切换在切模式后会重新建立物理显示器句柄（按需重连）。
- 若托盘不可用，控件本身仍可正常使用（仅托盘菜单不可用）。
- Rust 版托盘图标依赖 `C:\Windows\Fonts` 下的 Arial / 黑体字体来实时生成。

## 免责声明

本项目与 ASUSTeK 无任何关联，也未获得其授权或认可。
所有显示器控制命令均为对公开 DDC/CI 接口与 ASUS DisplayWidget Center 行为的互操作分析结果。
写入厂商私有 VCP 存在风险，请自行评估后使用，作者不对显示器损坏等后果负责。

## 许可证

尚未指定许可证。如需他人可自由使用/修改，建议补充一个（例如 MIT）。
