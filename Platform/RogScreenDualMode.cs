using System;
using System.IO;
using System.Runtime.InteropServices;

namespace RogControl.Platform;

/// <summary>
/// Switches the ROG dual-mode panel between 4K @ ~162 Hz and FHD @ 485 Hz
/// (ROG Strix XG27UCGR "绝神二代") over the monitor's DDC/CI vendor VCP
/// channel.
///
/// The exact command was recovered by reverse-engineering ASUS DisplayWidget
/// Center: its SetFrameRateBoost → SetToggleSettingsItem writes VCP 0xFD, a
/// bit-mask register. Bit 8 (0x100) is FrameRateBoost (the dual-mode switch)
/// and bit 0 (0x001) is VRR, which must be cleared at the same time because
/// the two are mutually exclusive. Verified on a DP-connected XG27UCGR:
///   - write 0xFD = 0x128 (296) → black-screen link re-handshake, the panel
///     switches to the pixel-binned 1K state, EDID updates and Windows shows
///     1920x1080 @ 485 Hz;
///   - write 0xFD = 0x001 (1) → back to 4K @ 162 Hz with VRR restored.
/// The write itself never touches the Windows display stack; the panel does
/// the EDID swap and the OS follows automatically.
///
/// Current-state detection uses the Windows display mode (resolution) rather
/// than a VCP read: this monitor's VCP reads frequently return failure with
/// garbage out-parameters, but the resolution always settles on
/// 3840x2160 (4K) / 1920x1080 (1K) after the panel re-handshake.
/// </summary>
internal static class RogScreenDualMode
{
    // VCP 0xFD bit-mask register on XG27UCGR (captured from DisplayWidget Center).
    private const byte VcpDualMode = 0xFD;
    private const uint Mode1K_Value = 0x128; // FrameRateBoost on (0x100) + aux bits, VRR off
    private const uint Mode4K_Value = 0x001; // VRR on, FrameRateBoost off

    private const int ENUM_CURRENT_SETTINGS = -1;
    private const uint Width4K = 3840;
    private const uint Height4K = 2160;
    private const uint Width1K = 1920;
    private const uint Height1K = 1080;

    [DllImport("user32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
    private static extern bool EnumDisplaySettings(
        string? lpszDeviceName, int iModeNum, ref DEVMODE lpDevMode);

    internal enum DualMode { Unknown, Mode4K, Mode1K }

    /// <summary>Reports which panel state the monitor is currently in by
    /// reading the current Windows resolution (the panel switch always lands
    /// on 3840x2160 for 4K / 1920x1080 for 1K). Returns Unknown when the
    /// display mode cannot be read or is neither preset.</summary>
    internal static DualMode GetCurrentMode()
    {
        var mode = new DEVMODE();
        if (!EnumDisplaySettings(null, ENUM_CURRENT_SETTINGS, ref mode))
        {
            return DualMode.Unknown;
        }

        if (mode.dmPelsWidth == Width4K && mode.dmPelsHeight == Height4K)
        {
            return DualMode.Mode4K;
        }

        if (mode.dmPelsWidth == Width1K && mode.dmPelsHeight == Height1K)
        {
            return DualMode.Mode1K;
        }

        return DualMode.Unknown;
    }

    /// <summary>Switches the panel to the requested dual-mode state. Returns an
    /// empty string on success, or a human-readable reason on failure.</summary>
    internal static string SwitchTo(DualMode target)
    {
        if (target is not (DualMode.Mode4K or DualMode.Mode1K))
        {
            return "无效的切换目标";
        }

        if (!TryGetPhysicalMonitor(out IntPtr physical))
        {
            return "未找到可用的物理显示器（DDC/CI 未开启？）";
        }

        try
        {
            uint value = target == DualMode.Mode1K ? Mode1K_Value : Mode4K_Value;
            if (!SetVCPFeature(physical, VcpDualMode, value))
            {
                return "显示器拒绝了双模切换命令（DDC/CI 被禁用？）";
            }

            // The panel re-handshakes the link (brief black screen) and then
            // the OS follows with the new EDID; give it a few seconds so the
            // follow-up state read (current resolution) sees the settled mode.
            System.Threading.Thread.Sleep(3000);
            return string.Empty;
        }
        finally
        {
            DestroyPhysicalMonitor(physical);
        }
    }

    // ---- DDC/CI plumbing (same safe pattern as MonitorBrightnessController) ----

    private static readonly int s_nativeMonitorStructSize =
        RogScreenNativeMethods.NativePhysicalMonitorStructSize;

    /// <summary>Which API DLL is being used: "monitorconfigapi.dll" or "dxva2.dll".</summary>
    private static string? ResolveLibrary()
    {
        string systemDirectory = Environment.SystemDirectory;
        if (File.Exists(Path.Combine(systemDirectory, "monitorconfigapi.dll")))
        {
            return "monitorconfigapi.dll";
        }

        if (File.Exists(Path.Combine(systemDirectory, "dxva2.dll")))
        {
            return "dxva2.dll";
        }

        return null;
    }

    private static bool TryGetPhysicalMonitor(out IntPtr physical)
    {
        physical = IntPtr.Zero;

        string? library = ResolveLibrary();
        if (library is null)
        {
            return false;
        }

        IntPtr desktop = RogScreenNativeMethods.GetDesktopWindow();
        IntPtr hMonitor = RogScreenNativeMethods.MonitorFromWindow(
            desktop, RogScreenNativeMethods.MONITOR_DEFAULTTOPRIMARY);
        if (hMonitor == IntPtr.Zero)
        {
            return false;
        }

        if (!GetNumberOfPhysicalMonitors(library, hMonitor, out uint count) || count == 0)
        {
            return false;
        }

        IntPtr buffer = IntPtr.Zero;
        try
        {
            buffer = Marshal.AllocHGlobal(s_nativeMonitorStructSize * (int)count);
            if (!GetPhysicalMonitors(library, hMonitor, count, buffer))
            {
                return false;
            }

            physical = Marshal.ReadIntPtr(buffer); // first monitor's handle

            // Free every handle after the first one we keep.
            for (int i = 1; i < count; i++)
            {
                IntPtr handle = Marshal.ReadIntPtr(buffer + (i * s_nativeMonitorStructSize));
                DestroyPhysicalMonitors(library, 1, BufferHandle(handle));
            }

            return physical != IntPtr.Zero;
        }
        finally
        {
            if (buffer != IntPtr.Zero)
            {
                Marshal.FreeHGlobal(buffer);
            }
        }
    }

    private static IntPtr BufferHandle(IntPtr hPhysicalMonitor)
    {
        IntPtr buffer = Marshal.AllocHGlobal(s_nativeMonitorStructSize);
        try
        {
            Marshal.WriteIntPtr(buffer, hPhysicalMonitor);
            return buffer;
        }
        catch
        {
            Marshal.FreeHGlobal(buffer);
            throw;
        }
    }

    private static void DestroyPhysicalMonitor(IntPtr physical)
    {
        if (physical == IntPtr.Zero)
        {
            return;
        }

        string? library = ResolveLibrary();
        if (library is null)
        {
            return;
        }

        try
        {
            IntPtr buffer = BufferHandle(physical);
            try
            {
                DestroyPhysicalMonitors(library, 1, buffer);
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }
        catch (Exception)
        {
            // Best-effort cleanup only.
        }
    }

    private static bool GetNumberOfPhysicalMonitors(string library, IntPtr hMonitor, out uint count)
    {
        return library == "monitorconfigapi.dll"
            ? RogScreenNativeMethods.MonitorConfigApi.GetNumberOfPhysicalMonitorsFromHMONITOR(hMonitor, out count)
            : RogScreenNativeMethods.Dxva2Api.GetNumberOfPhysicalMonitorsFromHMONITOR(hMonitor, out count);
    }

    private static bool GetPhysicalMonitors(string library, IntPtr hMonitor, uint size, IntPtr buffer)
    {
        return library == "monitorconfigapi.dll"
            ? RogScreenNativeMethods.MonitorConfigApi.GetPhysicalMonitorsFromHMONITOR(hMonitor, size, buffer)
            : RogScreenNativeMethods.Dxva2Api.GetPhysicalMonitorsFromHMONITOR(hMonitor, size, buffer);
    }

    private static void DestroyPhysicalMonitors(string library, uint size, IntPtr buffer)
    {
        if (library == "monitorconfigapi.dll")
        {
            RogScreenNativeMethods.MonitorConfigApi.DestroyPhysicalMonitors(size, buffer);
        }
        else
        {
            RogScreenNativeMethods.Dxva2Api.DestroyPhysicalMonitors(size, buffer);
        }
    }

    private static bool SetVCPFeature(IntPtr hMonitor, byte code, uint value)
    {
        string? library = ResolveLibrary();
        if (library is null)
        {
            return false;
        }

        return library == "monitorconfigapi.dll"
            ? RogScreenNativeMethods.MonitorConfigApi.SetVCPFeature(hMonitor, code, value)
            : RogScreenNativeMethods.Dxva2Api.SetVCPFeature(hMonitor, code, value);
    }
}

/// <summary>
/// Native DEVMODE (ANSI variant, 220 bytes on x64). Only the fields the
/// current-mode detection reads (dmPelsWidth / dmPelsHeight) are used.
/// </summary>
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Ansi)]
internal struct DEVMODE
{
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)]
    public string dmDeviceName;
    public ushort dmSpecVersion;
    public ushort dmDriverVersion;
    public ushort dmSize;
    public ushort dmDriverExtra;
    public uint dmFields;
    public int dmPositionX;
    public int dmPositionY;
    public uint dmDisplayOrientation;
    public uint dmDisplayFixedOutput;
    public short dmColor;
    public short dmDuplex;
    public short dmYResolution;
    public short dmTTOption;
    public short dmCollate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)]
    public string dmFormName;
    public ushort dmLogPixels;
    public uint dmBitsPerPel;
    public uint dmPelsWidth;
    public uint dmPelsHeight;
    public uint dmDisplayFlags;
    public uint dmDisplayFrequency;
    public uint dmICMMethod;
    public uint dmICMIntent;
    public uint dmMediaType;
    public uint dmDitherType;
    public uint dmReserved1;
    public uint dmReserved2;
    public uint dmPanningWidth;
    public uint dmPanningHeight;
}
