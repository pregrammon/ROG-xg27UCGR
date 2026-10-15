using System;
using System.Runtime.InteropServices;
using System.Text;

namespace RogControl.Platform;

/// <summary>
/// Embeds a window in the Explorer desktop layer (the same technique DeskBox
/// uses for desktop-pinned widgets). The window becomes a child of the
/// desktop icon host (SHELLDLL_DefView), so it floats above the wallpaper
/// but below every application window: opening any software covers it, and
/// it never steals the foreground on click (WS_EX_NOACTIVATE).
/// </summary>
internal static class Win32DesktopLayer
{
    private const int GWL_EXSTYLE = -20;
    private const long GWLP_HWNDPARENT = -8;
    private const int WS_EX_NOACTIVATE = 0x08000000;
    private static readonly IntPtr HWND_BOTTOM = new(1);
    private static readonly IntPtr HWND_NOTOPMOST = new(-2);
    private const uint SWP_NOSIZE = 0x0001;
    private const uint SWP_NOMOVE = 0x0002;
    private const uint SWP_NOZORDER = 0x0004;
    private const uint SWP_NOACTIVATE = 0x0010;
    private const uint SWP_FRAMECHANGED = 0x0020;
    private const uint SWP_SHOWWINDOW = 0x0040;

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr FindWindowEx(
        IntPtr hwndParent, IntPtr hwndChildAfter, string lpszClass, string? lpszWindow);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);

    private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr SetWindowLongPtr(IntPtr hWnd, int nIndex, IntPtr dwNewLong);

    [DllImport("user32.dll")]
    private static extern IntPtr GetWindowLongPtr(IntPtr hWnd, int nIndex);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowPos(
        IntPtr hWnd, IntPtr hWndInsertAfter, int x, int y, int cx, int cy, uint uFlags);

    /// <summary>Finds the desktop icon host (SHELLDLL_DefView) without forcing
    /// Explorer to create one. Returns IntPtr.Zero when desktop icons are
    /// hidden or Explorer is still starting.</summary>
    public static IntPtr FindDesktopIconView()
    {
        IntPtr defView = IntPtr.Zero;
        EnumWindows((hWnd, _) =>
        {
            IntPtr child = FindWindowEx(hWnd, IntPtr.Zero, "SHELLDLL_DefView", null);
            if (child != IntPtr.Zero)
            {
                defView = child;
                return false; // stop enumeration
            }

            return true;
        }, IntPtr.Zero);
        return defView;
    }

    /// <summary>
    /// Attaches the window to the desktop icon layer. Returns true when the
    /// window now rests inside the desktop (below every application window).
    /// On failure the window is left as a normal non-topmost window and the
    /// caller's bottom placement still applies.
    /// </summary>
    public static bool AttachToDesktopLayer(IntPtr hwnd)
    {
        IntPtr defView = FindDesktopIconView();
        if (defView == IntPtr.Zero)
        {
            return false;
        }

        if (GetWindowLongPtr(hwnd, (int)GWLP_HWNDPARENT) != defView)
        {
            SetWindowLongPtr(hwnd, (int)GWLP_HWNDPARENT, defView);
        }

        // Clear any topmost state and push to the bottom of the desktop band.
        SetWindowPos(
            hwnd, HWND_NOTOPMOST, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        SetWindowPos(
            hwnd, HWND_BOTTOM, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);

        // NOTE: no WS_EX_NOACTIVATE here. DeskBox toggles it dynamically on
        // pointer down; a permanently no-activate window risks swallowing
        // input on some surfaces. A click activates the window briefly (the
        // parent desktop host keeps it inside the desktop layer), which is
        // the price for guaranteed interaction.
        return true;
    }

    public static void SetNoActivate(IntPtr hwnd, bool enabled)
    {
        long style = GetWindowLongPtr(hwnd, GWL_EXSTYLE).ToInt64();
        long updated = enabled
            ? style | WS_EX_NOACTIVATE
            : style & ~WS_EX_NOACTIVATE;
        if (updated == style)
        {
            return;
        }

        SetWindowLongPtr(hwnd, GWL_EXSTYLE, (IntPtr)updated);
        SetWindowPos(
            hwnd, IntPtr.Zero, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }

    /// <summary>Pushes a window to the bottom of the normal Z order (fallback
    /// when no desktop icon host exists). New applications will cover it.</summary>
    public static void MoveToBottom(IntPtr hwnd)
    {
        SetWindowPos(
            hwnd, HWND_BOTTOM, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }

    public static string GetClassNameSafe(IntPtr hwnd)
    {
        var sb = new StringBuilder(256);
        int len = GetClassName(hwnd, sb, sb.Capacity);
        return len > 0 ? sb.ToString() : string.Empty;
    }
}
