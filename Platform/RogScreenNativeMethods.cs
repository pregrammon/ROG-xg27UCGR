using System;
using System.IO;
using System.Runtime.InteropServices;

namespace RogControl.Platform;

/// <summary>
/// Win32 monitor DDC/CI entry points. The physical-monitor brightness API
/// (GetPhysicalMonitorsFromHMONITOR / GetMonitorBrightness /
/// SetMonitorBrightness / DestroyPhysicalMonitors) lives in
/// monitorconfigapi.dll on Windows 8+ and in dxva2.dll on older systems;
/// both DLLs are probed at runtime so the controller works on any supported
/// Windows version. ROG monitors enable DDC/CI by default.
///
/// Array-shaped native calls (GetPhysicalMonitorsFromHMONITOR /
/// DestroyPhysicalMonitors) take an IntPtr so the caller controls the native
/// buffer exactly; the default managed-array marshalling of PHYSICAL_MONITOR
/// (which embeds a 128-wide WCHAR name) is unreliable and caused
/// AccessViolation on some systems.
/// </summary>
internal static class RogScreenNativeMethods
{
    // --- user32 (present on every Windows) ---
    [DllImport("user32.dll")]
    internal static extern IntPtr GetDesktopWindow();

    [DllImport("user32.dll")]
    internal static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint dwFlags);

    internal const uint MONITOR_DEFAULTTOPRIMARY = 0x00000001;

    // --- monitorconfigapi.dll (Windows 8+) ---
    internal static class MonitorConfigApi
    {
        [DllImport("monitorconfigapi.dll", SetLastError = true, EntryPoint = "GetNumberOfPhysicalMonitorsFromHMONITOR")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetNumberOfPhysicalMonitorsFromHMONITOR(
            IntPtr hMonitor, out uint pdwNumberOfPhysicalMonitors);

        [DllImport("monitorconfigapi.dll", SetLastError = true, EntryPoint = "GetPhysicalMonitorsFromHMONITOR")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetPhysicalMonitorsFromHMONITOR(
            IntPtr hMonitor, uint dwPhysicalMonitorArraySize, IntPtr pPhysicalMonitorArray);

        [DllImport("monitorconfigapi.dll", SetLastError = true, EntryPoint = "GetMonitorBrightness")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetMonitorBrightness(
            IntPtr hMonitor, out uint pdwMinimumBrightness,
            out uint pdwCurrentBrightness, out uint pdwMaximumBrightness);

        [DllImport("monitorconfigapi.dll", SetLastError = true, EntryPoint = "SetMonitorBrightness")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool SetMonitorBrightness(IntPtr hMonitor, uint dwNewBrightness);

        [DllImport("monitorconfigapi.dll", SetLastError = true, EntryPoint = "GetVCPFeatureAndVCPFeatureReply")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetVCPFeatureAndVCPFeatureReply(
            IntPtr hMonitor, byte bVCPCode, out uint pdwCurrentValue, out uint pdwMaximumValue);

        [DllImport("monitorconfigapi.dll", SetLastError = true, EntryPoint = "SetVCPFeature")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool SetVCPFeature(IntPtr hMonitor, byte bVCPCode, uint dwNewValue);

        [DllImport("monitorconfigapi.dll", SetLastError = true, EntryPoint = "DestroyPhysicalMonitors")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool DestroyPhysicalMonitors(
            uint dwPhysicalMonitorArraySize, IntPtr pPhysicalMonitorArray);
    }

    // --- dxva2.dll (legacy systems, still exported on Windows 10/11) ---
    internal static class Dxva2Api
    {
        [DllImport("dxva2.dll", SetLastError = true, EntryPoint = "GetNumberOfPhysicalMonitorsFromHMONITOR")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetNumberOfPhysicalMonitorsFromHMONITOR(
            IntPtr hMonitor, out uint pdwNumberOfPhysicalMonitors);

        [DllImport("dxva2.dll", SetLastError = true, EntryPoint = "GetPhysicalMonitorsFromHMONITOR")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetPhysicalMonitorsFromHMONITOR(
            IntPtr hMonitor, uint dwPhysicalMonitorArraySize, IntPtr pPhysicalMonitorArray);

        [DllImport("dxva2.dll", SetLastError = true, EntryPoint = "GetMonitorBrightness")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetMonitorBrightness(
            IntPtr hMonitor, out uint pdwMinimumBrightness,
            out uint pdwCurrentBrightness, out uint pdwMaximumBrightness);

        [DllImport("dxva2.dll", SetLastError = true, EntryPoint = "SetMonitorBrightness")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool SetMonitorBrightness(IntPtr hMonitor, uint dwNewBrightness);

        [DllImport("dxva2.dll", SetLastError = true, EntryPoint = "GetVCPFeatureAndVCPFeatureReply")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool GetVCPFeatureAndVCPFeatureReply(
            IntPtr hMonitor, byte bVCPCode, out uint pdwCurrentValue, out uint pdwMaximumValue);

        [DllImport("dxva2.dll", SetLastError = true, EntryPoint = "SetVCPFeature")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool SetVCPFeature(IntPtr hMonitor, byte bVCPCode, uint dwNewValue);

        [DllImport("dxva2.dll", SetLastError = true, EntryPoint = "DestroyPhysicalMonitors")]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool DestroyPhysicalMonitors(
            uint dwPhysicalMonitorArraySize, IntPtr pPhysicalMonitorArray);
    }

    // Native PHYSICAL_MONITOR layout (x64): HANDLE(8 bytes) + WCHAR[128](256
    // bytes) = 264 bytes, 8-byte aligned. The handle is the first field, so
    // the controller reads/writes it with Marshal.ReadIntPtr/WriteIntPtr and
    // never marshals the whole struct — Marshal.SizeOf returns the managed
    // size (16) for this struct on modern .NET, which made the buffer too
    // small and corrupted the heap (AccessViolation).
    internal const int NativePhysicalMonitorStructSize = 264;
}

/// <summary>
/// Reads and writes the brightness of the first physical monitor attached to
/// the primary display over DDC/CI. <see cref="IsSupported"/> is false when no
/// DDC/CI-capable physical monitor was found; <see cref="UnsupportedReason"/>
/// then carries a human-readable explanation for the status line.
/// </summary>
internal sealed class MonitorBrightnessController : IDisposable
{
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

    private static readonly int s_nativeMonitorStructSize =
        RogScreenNativeMethods.NativePhysicalMonitorStructSize;

    private readonly string? _library;
    private IntPtr _physicalMonitor;
    private uint _min;
    private uint _max;
    private bool _disposed;

    /// <summary>True when a DDC/CI-capable physical monitor was found and opened.</summary>
    public bool IsSupported { get; private set; }

    /// <summary>Human-readable reason when <see cref="IsSupported"/> is false.</summary>
    public string UnsupportedReason { get; private set; } = string.Empty;

    public MonitorBrightnessController()
    {
        try
        {
            _library = ResolveLibrary();
            if (_library is null)
            {
                UnsupportedReason = "系统缺少 DDC/CI 支持库（monitorconfigapi.dll / dxva2.dll）";
                return;
            }

            Initialize();
        }
        catch (Exception)
        {
            // Any P/Invoke failure (missing DLL, no DDC/CI monitor, access
            // denied) simply leaves the controller unsupported. The UI then
            // shows a friendly message instead of crashing.
            ReleaseHandles();
            IsSupported = false;
            if (string.IsNullOrEmpty(UnsupportedReason))
            {
                UnsupportedReason = "初始化 DDC/CI 失败";
            }
        }
    }

    private void Initialize()
    {
        if (!AcquireHandle())
        {
            return;
        }

        if (!GetMonitorBrightness(_physicalMonitor, out _min, out _, out _max))
        {
            ReleaseHandles();
            UnsupportedReason = "读取显示器亮度失败（显示器可能不支持 DDC/CI）";
            return;
        }

        IsSupported = true;
        UnsupportedReason = string.Empty;
    }

    /// <summary>
    /// Opens the physical-monitor handle for the primary display. Returns
    /// false (with <see cref="UnsupportedReason"/> set) when the monitor
    /// cannot be reached.
    /// </summary>
    private bool AcquireHandle()
    {
        IntPtr desktop = RogScreenNativeMethods.GetDesktopWindow();
        IntPtr hMonitor = RogScreenNativeMethods.MonitorFromWindow(
            desktop, RogScreenNativeMethods.MONITOR_DEFAULTTOPRIMARY);
        if (hMonitor == IntPtr.Zero)
        {
            UnsupportedReason = "无法获取主显示器句柄";
            return false;
        }

        if (!GetNumberOfPhysicalMonitors(hMonitor, out uint count) || count == 0)
        {
            UnsupportedReason = "未找到可用的物理显示器（DDC/CI 未开启？）";
            return false;
        }

        // Allocate a native buffer for the PHYSICAL_MONITOR array and parse it
        // element by element. This bypasses managed-array marshalling entirely.
        IntPtr buffer = IntPtr.Zero;
        try
        {
            buffer = Marshal.AllocHGlobal((int)(s_nativeMonitorStructSize * count));
            if (!GetPhysicalMonitors(hMonitor, count, buffer))
            {
                UnsupportedReason = "获取物理显示器失败";
                return false;
            }

            IntPtr firstHandle = IntPtr.Zero;
            for (int i = 0; i < count; i++)
            {
                // The physical monitor handle is the first field of the
                // native struct; read it directly and skip struct marshalling.
                IntPtr handle = Marshal.ReadIntPtr(buffer + (i * s_nativeMonitorStructSize));

                if (i == 0)
                {
                    firstHandle = handle;
                }
                else
                {
                    // Free every handle except the first one we keep.
                    DestroyPhysicalMonitors(1, BufferHandle(handle));
                }
            }

            if (firstHandle == IntPtr.Zero)
            {
                UnsupportedReason = "物理显示器句柄无效";
                return false;
            }

            _physicalMonitor = firstHandle;
            return true;
        }
        finally
        {
            if (buffer != IntPtr.Zero)
            {
                Marshal.FreeHGlobal(buffer);
            }
        }
    }

    /// <summary>
    /// Re-opens the physical-monitor handle. A dual-mode panel switch
    /// re-handshakes the link (new EDID, Windows re-enumerates the display),
    /// which can invalidate the previously cached handle; this method re-fetches
    /// it so brightness keeps working on the new link. Throws when the monitor
    /// cannot be re-acquired.
    /// </summary>
    private void ReopenPhysicalMonitor()
    {
        ReleaseHandles();

        if (!AcquireHandle())
        {
            IsSupported = false;
            throw new InvalidOperationException(
                string.IsNullOrEmpty(UnsupportedReason) ? "显示器连接已断开。" : UnsupportedReason);
        }

        if (!GetMonitorBrightness(_physicalMonitor, out _min, out _, out _max))
        {
            ReleaseHandles();
            IsSupported = false;
            throw new InvalidOperationException("显示器连接已断开，无法读取亮度。");
        }
    }

    private static IntPtr BufferHandle(IntPtr hPhysicalMonitor)
    {
        // Write only the handle (first 8 bytes); the description field is
        // never read by DestroyPhysicalMonitors, so no struct marshalling is
        // needed and the fixed 264-byte slot is always correct.
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

    // ---- library dispatch helpers ----

    private bool GetNumberOfPhysicalMonitors(IntPtr hMonitor, out uint count)
    {
        return _library == "monitorconfigapi.dll"
            ? RogScreenNativeMethods.MonitorConfigApi.GetNumberOfPhysicalMonitorsFromHMONITOR(hMonitor, out count)
            : RogScreenNativeMethods.Dxva2Api.GetNumberOfPhysicalMonitorsFromHMONITOR(hMonitor, out count);
    }

    private bool GetPhysicalMonitors(IntPtr hMonitor, uint size, IntPtr buffer)
    {
        return _library == "monitorconfigapi.dll"
            ? RogScreenNativeMethods.MonitorConfigApi.GetPhysicalMonitorsFromHMONITOR(hMonitor, size, buffer)
            : RogScreenNativeMethods.Dxva2Api.GetPhysicalMonitorsFromHMONITOR(hMonitor, size, buffer);
    }

    private bool GetMonitorBrightness(IntPtr hMonitor, out uint min, out uint current, out uint max)
    {
        return _library == "monitorconfigapi.dll"
            ? RogScreenNativeMethods.MonitorConfigApi.GetMonitorBrightness(hMonitor, out min, out current, out max)
            : RogScreenNativeMethods.Dxva2Api.GetMonitorBrightness(hMonitor, out min, out current, out max);
    }

    private bool SetMonitorBrightness(IntPtr hMonitor, uint value)
    {
        return _library == "monitorconfigapi.dll"
            ? RogScreenNativeMethods.MonitorConfigApi.SetMonitorBrightness(hMonitor, value)
            : RogScreenNativeMethods.Dxva2Api.SetMonitorBrightness(hMonitor, value);
    }

    private void DestroyPhysicalMonitors(uint size, IntPtr buffer)
    {
        if (_library == "monitorconfigapi.dll")
        {
            RogScreenNativeMethods.MonitorConfigApi.DestroyPhysicalMonitors(size, buffer);
        }
        else
        {
            RogScreenNativeMethods.Dxva2Api.DestroyPhysicalMonitors(size, buffer);
        }
    }

    private void ReleaseHandles()
    {
        if (_physicalMonitor != IntPtr.Zero)
        {
            try
            {
                IntPtr buffer = BufferHandle(_physicalMonitor);
                try
                {
                    DestroyPhysicalMonitors(1, buffer);
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

            _physicalMonitor = IntPtr.Zero;
        }
    }

    /// <summary>Gets the current brightness as a percentage in [0, 100].</summary>
    public int GetBrightnessPercent()
    {
        ThrowIfUnsupported();

        try
        {
            if (TryReadBrightness(out uint min, out uint current, out uint max))
            {
                return RangeToPercent(current, min, max);
            }

            // The cached handle can go stale after a dual-mode panel switch
            // (new EDID → Windows re-enumerates the display). Re-open it and
            // retry once before giving up.
            ReopenPhysicalMonitor();
            if (TryReadBrightness(out min, out current, out max))
            {
                return RangeToPercent(current, min, max);
            }

            throw new InvalidOperationException("无法读取显示器亮度。");
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException or InvalidOperationException)
        {
            throw new InvalidOperationException("无法读取显示器亮度。", ex);
        }
    }

    /// <summary>Sets the brightness from a percentage in [0, 100].</summary>
    public void SetBrightnessPercent(int percent)
    {
        ThrowIfUnsupported();

        percent = Math.Clamp(percent, 0, 100);
        uint value = (uint)Math.Round(_min + (_max - _min) * percent / 100.0);

        try
        {
            if (TryWriteBrightness(value))
            {
                return;
            }

            // Same stale-handle recovery as GetBrightnessPercent.
            ReopenPhysicalMonitor();
            if (TryWriteBrightness(value))
            {
                return;
            }

            throw new InvalidOperationException("调节亮度失败。");
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException or InvalidOperationException)
        {
            throw new InvalidOperationException("调节亮度失败。", ex);
        }
    }

    private bool TryReadBrightness(out uint min, out uint current, out uint max)
    {
        return GetMonitorBrightness(_physicalMonitor, out min, out current, out max);
    }

    private bool TryWriteBrightness(uint value)
    {
        return SetMonitorBrightness(_physicalMonitor, value);
    }

    private void ThrowIfUnsupported()
    {
        if (!IsSupported)
        {
            throw new InvalidOperationException(
                string.IsNullOrEmpty(UnsupportedReason) ? "显示器不支持 DDC/CI 亮度调节。" : UnsupportedReason);
        }
    }

    private static int RangeToPercent(uint value, uint min, uint max)
    {
        if (max <= min)
        {
            return 100;
        }

        double percent = (value - min) * 100.0 / (max - min);
        return Math.Clamp((int)Math.Round(percent), 0, 100);
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        ReleaseHandles();
    }
}
