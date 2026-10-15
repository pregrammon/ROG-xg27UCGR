using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text.Json;
using H.NotifyIcon;
using H.NotifyIcon.Core;
using Microsoft.UI.Composition;
using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.Win32;
using Windows.Foundation;
using Windows.Graphics;
using RogControl.Platform;
using WinRT;
using WinRT.Interop;
using WinUIEx;

namespace RogControl;

/// <summary>
/// Transparent frameless ROG floating control window. It lives in the
/// Explorer desktop layer (any application window covers it), is draggable
/// from the title strip unless pinned, and can be resized by dragging its
/// edges/corners. Position and size are remembered.
/// </summary>
public sealed partial class MainWindow : Window
{
    private const int MinWidth = 280;
    private const int MinHeight = 160;

    private string _settingsPath = "";
    private bool _pinned;
    private TaskbarIcon? _trayIcon;
    private ToggleMenuFlyoutItem? _transparentItem;
    private ToggleMenuFlyoutItem? _blurItem;
    private ToggleMenuFlyoutItem? _autoStartItem;
    private bool _transparent = true; // startup state matches the current fully transparent look
    private bool _blur;

    // Desktop Acrylic controller, configured exactly like DeskBox's "Thin"
    // acrylic (dark, default material intensity 0.65).
    private DesktopAcrylicController? _acrylicController;
    private SystemBackdropConfiguration? _acrylicConfig;
    private bool _acrylicAttached;

    private bool _dragging;
    private Point _dragStartScreen;
    private int _dragStartWindowX;
    private int _dragStartWindowY;

    private bool _resizing;
    private string _resizeDirection = "";
    private Point _resizeStartScreen;
    private int _resizeStartX;
    private int _resizeStartY;
    private int _resizeStartWidth;
    private int _resizeStartHeight;

    public MainWindow()
    {
        InitializeComponent();

        ConfigureFramelessWindow();

        _settingsPath = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "RogControl", "settings.json");

        RootGrid.Loaded += OnRootLoaded;
        Closed += OnClosed;

        CreateTrayIcon();
    }

    // ---- system tray ----

    /// <summary>Creates the tray icon (red dot) with a right-click menu so the
    /// window can be closed from the notification area.</summary>
    private void CreateTrayIcon()
    {
        try
        {
            var menu = new MenuFlyout { ShouldConstrainToRootBounds = false };

            _transparentItem = new ToggleMenuFlyoutItem { Text = "窗口透明", IsChecked = _transparent };
            _transparentItem.Click += (_, _) => SetTransparent(_transparentItem.IsChecked);

            _blurItem = new ToggleMenuFlyoutItem { Text = "高斯模糊", IsChecked = _blur };
            _blurItem.Click += (_, _) => SetBlur(_blurItem.IsChecked);

            _autoStartItem = new ToggleMenuFlyoutItem { Text = "开机自启动", IsChecked = IsAutoStartEnabled() };
            _autoStartItem.Click += (_, _) => SetAutoStart(_autoStartItem.IsChecked);

            var exitItem = new MenuFlyoutItem { Text = "退出" };
            exitItem.Click += (_, _) => ExitApplication();

            menu.Items.Add(_transparentItem);
            menu.Items.Add(_blurItem);
            menu.Items.Add(_autoStartItem);
            menu.Items.Add(new MenuFlyoutSeparator());
            menu.Items.Add(exitItem);

            _trayIcon = new TaskbarIcon
            {
                Icon = CreateTrayIconImage(),
                ToolTipText = "ROG 屏幕控制",
                ContextMenuMode = ContextMenuMode.SecondWindow,
                MenuActivation = PopupActivationMode.RightClick,
                ContextFlyout = menu,
            };
            _trayIcon.ForceCreate();
        }
        catch (Exception)
        {
            // Tray unavailable; the window itself is still usable.
        }
    }

    /// <summary>Window transparency toggle. Turning it on also turns blur
    /// off (they are mutually exclusive backgrounds).</summary>
    private void SetTransparent(bool on)
    {
        _transparent = on;
        if (on)
        {
            _blur = false;
            if (_blurItem is not null)
            {
                _blurItem.IsChecked = false;
            }
            MakeWindowTransparent();
        }
        else
        {
            ApplyOpaque();
        }
    }

    /// <summary>Acrylic (gaussian blur + tint) toggle, the same material
    /// family DeskBox uses for its blurred widgets. Turning it on also turns
    /// full transparency off.</summary>
    private void SetBlur(bool on)
    {
        _blur = on;
        if (on)
        {
            _transparent = false;
            if (_transparentItem is not null)
            {
                _transparentItem.IsChecked = false;
            }
            ApplyBlur();
        }
        else
        {
            _transparent = true;
            if (_transparentItem is not null)
            {
                _transparentItem.IsChecked = true;
            }
            MakeWindowTransparent();
        }
    }

    /// <summary>
    /// Acrylic backdrop, identical to DeskBox's "Thin" acrylic: a
    /// DesktopAcrylicController with the dark surface tint (#202226),
    /// tintOpacity 0.29 and luminosityOpacity 0.42 (computed from DeskBox's
    /// default material intensity 0.65).
    /// </summary>
    private void ApplyBlur()
    {
        IntPtr hwnd = WindowNative.GetWindowHandle(this);
        if (hwnd == IntPtr.Zero)
        {
            return;
        }

        try
        {
            // The controller owns the backdrop; release the XAML system
            // backdrop (transparent tint / none) first.
            SystemBackdrop = null;

            _acrylicConfig ??= new SystemBackdropConfiguration
            {
                IsInputActive = true,
                Theme = SystemBackdropTheme.Dark,
                HighContrastBackgroundColor = Windows.UI.Color.FromArgb(0xFF, 0x20, 0x20, 0x20),
            };

            if (_acrylicController is null)
            {
                _acrylicController = new DesktopAcrylicController
                {
                    Kind = DesktopAcrylicKind.Thin,
                    TintColor = Windows.UI.Color.FromArgb(0xFF, 0x20, 0x22, 0x26),
                    FallbackColor = Windows.UI.Color.FromArgb(0xFF, 0x20, 0x22, 0x26),
                    TintOpacity = 0.29f,
                    LuminosityOpacity = 0.42f,
                };
            }

            if (!_acrylicAttached)
            {
                if (!_acrylicController.AddSystemBackdropTarget(this.As<ICompositionSupportsSystemBackdrop>()))
                {
                    _acrylicController.Dispose();
                    _acrylicController = null;
                    return;
                }

                _acrylicController.SetSystemBackdropConfiguration(_acrylicConfig);
                _acrylicAttached = true;
            }
        }
        catch (Exception)
        {
            // Acrylic unsupported (e.g. very old OS); leave the previous look.
        }

        uint borderColor = 0x00212223; // keep the outer edge #212223
        NativeMethods.DwmSetWindowAttribute(hwnd, 34 /*DWMWA_BORDER_COLOR*/, ref borderColor, sizeof(uint));
    }

    /// <summary>Detaches the acrylic controller so a XAML backdrop or a plain
    /// root background can take over again.</summary>
    private void DetachAcrylic()
    {
        if (_acrylicController is null || !_acrylicAttached)
        {
            return;
        }

        try
        {
            _acrylicController.RemoveAllSystemBackdropTargets();
        }
        catch (Exception)
        {
            // Best-effort detach.
        }
        finally
        {
            _acrylicAttached = false;
        }
    }

    /// <summary>Opaque mode: no system backdrop, solid #212223 root
    /// background so the card is fully solid.</summary>
    private void ApplyOpaque()
    {
        DetachAcrylic();
        SystemBackdrop = null;
        RootGrid.Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(
            Windows.UI.Color.FromArgb(255, 0x21, 0x22, 0x23));
    }

    // ---- auto start ----

    private const string AutoStartValueName = "RogControl";

    private static bool IsAutoStartEnabled()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(
                @"Software\Microsoft\Windows\CurrentVersion\Run");
            return key?.GetValue(AutoStartValueName) is not null;
        }
        catch (Exception)
        {
            return false;
        }
    }

    private static void SetAutoStart(bool enable)
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(
                @"Software\Microsoft\Windows\CurrentVersion\Run", writable: true);
            if (enable)
            {
                string exe = Environment.ProcessPath
                    ?? Path.Combine(AppContext.BaseDirectory, "RogControl.exe");
                key?.SetValue(AutoStartValueName, $"\"{exe}\"");
            }
            else
            {
                key?.DeleteValue(AutoStartValueName, throwOnMissingValue: false);
            }
        }
        catch (Exception)
        {
            // Best-effort; the user can retry from the tray menu.
        }
    }

    private void ExitApplication()
    {
        _trayIcon?.Dispose();
        _trayIcon = null;
        Close();
    }

    /// <summary>Red "R" glyph on a transparent background.</summary>
    private static System.Drawing.Icon CreateTrayIconImage()
    {
        using var bmp = new System.Drawing.Bitmap(32, 32);
        using (var g = System.Drawing.Graphics.FromImage(bmp))
        {
            g.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
            g.TextRenderingHint = System.Drawing.Text.TextRenderingHint.AntiAlias;
            g.Clear(System.Drawing.Color.Transparent);
            using var font = new System.Drawing.Font(
                "Arial", 22, System.Drawing.FontStyle.Bold, System.Drawing.GraphicsUnit.Pixel);
            using var brush = new System.Drawing.SolidBrush(System.Drawing.Color.FromArgb(245, 25, 40));
            var size = g.MeasureString("R", font);
            g.DrawString("R", font, brush, (bmp.Width - size.Width) / 2, (bmp.Height - size.Height) / 2);
        }

        return System.Drawing.Icon.FromHandle(bmp.GetHicon());
    }

    /// <summary>Borderless fixed-style overlapped window, hidden from Alt+Tab.</summary>
    private void ConfigureFramelessWindow()
    {
        var appWindow = AppWindow;
        appWindow.SetPresenter(AppWindowPresenterKind.Overlapped);
        if (appWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.IsResizable = false;
            presenter.IsMaximizable = false;
            presenter.IsMinimizable = false;
            presenter.SetBorderAndTitleBar(false, false);
        }

        appWindow.IsShownInSwitchers = false;
        appWindow.Title = "ROG Control";
        appWindow.Resize(new SizeInt32(340, 232));
    }

    private void OnRootLoaded(object sender, RoutedEventArgs e)
    {
        RestoreWindowState();
        EmbedInDesktopLayer();
        MakeWindowTransparent();
    }

    private void OnClosed(object sender, WindowEventArgs args)
    {
        SaveWindowState();
    }

    // ---- transparency ----

    /// <summary>
    /// Makes the window itself fully transparent via a transparent system
    /// backdrop (WinUIEx), so only the XAML content (the rounded card and its
    /// controls) renders. Also clears the 1px DWM frame color so no edge is
    /// visible around the transparent area.
    /// </summary>
    private void MakeWindowTransparent()
    {
        IntPtr hwnd = WindowNative.GetWindowHandle(this);
        if (hwnd == IntPtr.Zero)
        {
            return;
        }

        DetachAcrylic();
        SystemBackdrop = new TransparentTintBackdrop(Windows.UI.Color.FromArgb(0, 0, 0, 0));

        // Restore the transparent root background in case opaque mode set it.
        RootGrid.Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(
            Windows.UI.Color.FromArgb(0, 0, 0, 0));

        // The 1px DWM frame cannot be truly transparent (it renders black);
        // paint it the same color as the card border (#212223) so it merges
        // with the window's own border instead of showing as a dark edge.
        uint borderColor = 0x00212223; // COLORREF (0x00BBGGRR) for #212223
        NativeMethods.DwmSetWindowAttribute(hwnd, 34 /*DWMWA_BORDER_COLOR*/, ref borderColor, sizeof(uint));
    }

    // ---- desktop layer ----

    /// <summary>Puts the window inside the Explorer desktop icon layer so any
    /// application window covers it; falls back to a plain bottom Z order.
    /// SetParent can disturb the DWM transparency, so the transparency is
    /// re-applied after embedding.</summary>
    private void EmbedInDesktopLayer()
    {
        IntPtr hwnd = WindowNative.GetWindowHandle(this);
        if (hwnd == IntPtr.Zero)
        {
            return;
        }

        if (!Win32DesktopLayer.AttachToDesktopLayer(hwnd))
        {
            Win32DesktopLayer.MoveToBottom(hwnd);
        }

        MakeWindowTransparent();
    }

    // ---- pin (lock position) ----

    private void PinButton_Click(object sender, RoutedEventArgs e)
    {
        _pinned = !_pinned;
        PinIcon.Glyph = _pinned ? "\uE77A" : "\uE718";
        PinIcon.Foreground = _pinned
            ? new Microsoft.UI.Xaml.Media.SolidColorBrush(Windows.UI.Color.FromArgb(255, 0xF5, 0x19, 0x28))
            : new Microsoft.UI.Xaml.Media.SolidColorBrush(Windows.UI.Color.FromArgb(255, 0x8A, 0x8B, 0x90));
    }

    // ---- window state (position + size, screen coordinates) ----

    private void RestoreWindowState()
    {
        try
        {
            if (File.Exists(_settingsPath))
            {
                var state = JsonSerializer.Deserialize<PersistedState>(File.ReadAllText(_settingsPath));
                if (state is not null)
                {
                    int width = Math.Clamp((int)state.Width, MinWidth, 2000);
                    int height = Math.Clamp((int)state.Height, MinHeight, 2000);
                    AppWindow.Resize(new SizeInt32(width, height));

                    if (NativeMethods.TryGetWorkArea(out var wa))
                    {
                        int x = Math.Clamp((int)state.X, wa.Left, Math.Max(wa.Left, wa.Right - width));
                        int y = Math.Clamp((int)state.Y, wa.Top, Math.Max(wa.Top, wa.Bottom - height));
                        SetWindowRect(WindowNative.GetWindowHandle(this), x, y, width, height, moveOnly: false);
                    }
                    else
                    {
                        SetWindowRect(WindowNative.GetWindowHandle(this), (int)state.X, (int)state.Y, width, height, moveOnly: false);
                    }
                }
            }
        }
        catch (Exception)
        {
            // Fall back to defaults.
        }
    }

    private void SaveWindowState()
    {
        try
        {
            GetWindowRect(out int x, out int y, out int width, out int height);
            Directory.CreateDirectory(Path.GetDirectoryName(_settingsPath)!);
            var state = new PersistedState
            {
                X = x,
                Y = y,
                Width = width,
                Height = height,
            };
            File.WriteAllText(_settingsPath, JsonSerializer.Serialize(state));
        }
        catch (Exception)
        {
            // Best-effort persistence only.
        }
    }

    private sealed class PersistedState
    {
        public double X { get; set; }
        public double Y { get; set; }
        public double Width { get; set; }
        public double Height { get; set; }
    }

    // ---- manual dragging (title strip, disabled while pinned) ----

    private void TitleBar_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        if (_pinned || !e.GetCurrentPoint(TitleBar).Properties.IsLeftButtonPressed)
        {
            return;
        }

        _dragStartScreen = GetCursorScreenPoint();
        GetWindowRect(out _dragStartWindowX, out _dragStartWindowY, out _, out _);
        _dragging = true;
        TitleBar.CapturePointer(e.Pointer);
    }

    private void TitleBar_PointerMoved(object sender, PointerRoutedEventArgs e)
    {
        if (!_dragging)
        {
            return;
        }

        Point cursor = GetCursorScreenPoint();
        int newX = _dragStartWindowX + (int)(cursor.X - _dragStartScreen.X);
        int newY = _dragStartWindowY + (int)(cursor.Y - _dragStartScreen.Y);
        SetWindowRect(WindowNative.GetWindowHandle(this), newX, newY, 0, 0, moveOnly: true);
    }

    private void TitleBar_PointerReleased(object sender, PointerRoutedEventArgs e)
    {
        if (!_dragging)
        {
            return;
        }

        _dragging = false;
        TitleBar.ReleasePointerCaptures();
        SaveWindowState();
    }

    private void TitleBar_PointerCaptureLost(object sender, PointerRoutedEventArgs e)
    {
        _dragging = false;
    }

    // ---- manual resize (edges and corners) ----

    private void ResizeThumb_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        if (!e.GetCurrentPoint(RootGrid).Properties.IsLeftButtonPressed)
        {
            return;
        }

        _resizeDirection = (string)((FrameworkElement)sender).Tag;
        _resizeStartScreen = GetCursorScreenPoint();
        GetWindowRect(out _resizeStartX, out _resizeStartY, out _resizeStartWidth, out _resizeStartHeight);
        _resizing = true;
        ((UIElement)sender).CapturePointer(e.Pointer);
    }

    private void ResizeThumb_PointerMoved(object sender, PointerRoutedEventArgs e)
    {
        if (!_resizing)
        {
            return;
        }

        Point cursor = GetCursorScreenPoint();
        int dx = (int)(cursor.X - _resizeStartScreen.X);
        int dy = (int)(cursor.Y - _resizeStartScreen.Y);

        int newX = _resizeStartX;
        int newY = _resizeStartY;
        int newWidth = _resizeStartWidth;
        int newHeight = _resizeStartHeight;

        if (_resizeDirection.Contains("R"))
        {
            newWidth = Math.Max(MinWidth, _resizeStartWidth + dx);
        }
        else if (_resizeDirection.Contains("L"))
        {
            newWidth = Math.Max(MinWidth, _resizeStartWidth - dx);
            newX = _resizeStartX + (_resizeStartWidth - newWidth);
        }
        if (_resizeDirection.Contains("B"))
        {
            newHeight = Math.Max(MinHeight, _resizeStartHeight + dy);
        }
        else if (_resizeDirection.Contains("T"))
        {
            newHeight = Math.Max(MinHeight, _resizeStartHeight - dy);
            newY = _resizeStartY + (_resizeStartHeight - newHeight);
        }

        SetWindowRect(WindowNative.GetWindowHandle(this), newX, newY, newWidth, newHeight, moveOnly: false);
    }

    private void ResizeThumb_PointerReleased(object sender, PointerRoutedEventArgs e)
    {
        if (!_resizing)
        {
            return;
        }

        _resizing = false;
        ((UIElement)sender).ReleasePointerCaptures();
        SaveWindowState();
    }

    private void ResizeThumb_PointerCaptureLost(object sender, PointerRoutedEventArgs e)
    {
        _resizing = false;
    }

    // ---- Win32 helpers ----

    /// <summary>Cursor position in physical screen pixels (Win32).</summary>
    private static Point GetCursorScreenPoint()
    {
        NativeMethods.GetCursorPos(out NativeMethods.POINT pt);
        return new Point(pt.X, pt.Y);
    }

    private void GetWindowRect(out int x, out int y, out int width, out int height)
    {
        NativeMethods.GetWindowRect(WindowNative.GetWindowHandle(this), out NativeMethods.RECT rect);
        x = rect.Left;
        y = rect.Top;
        width = rect.Right - rect.Left;
        height = rect.Bottom - rect.Top;
    }

    private static void SetWindowRect(IntPtr hwnd, int x, int y, int width, int height, bool moveOnly)
    {
        if (hwnd == IntPtr.Zero)
        {
            return;
        }

        uint flags = NativeMethods.SWP_NOZORDER | NativeMethods.SWP_NOACTIVATE;
        if (moveOnly)
        {
            flags |= NativeMethods.SWP_NOSIZE;
        }

        NativeMethods.SetWindowPos(hwnd, IntPtr.Zero, x, y, width, height, flags);
    }

    private static class NativeMethods
    {
        public const uint SWP_NOSIZE = 0x0001;
        public const uint SWP_NOZORDER = 0x0004;
        public const uint SWP_NOACTIVATE = 0x0010;

        [StructLayout(LayoutKind.Sequential)]
        public struct RECT
        {
            public int Left;
            public int Top;
            public int Right;
            public int Bottom;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct MARGINS
        {
            public int cxLeftWidth;
            public int cxRightWidth;
            public int cyTopHeight;
            public int cyBottomHeight;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct POINT
        {
            public int X;
            public int Y;
        }

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool GetCursorPos(out POINT lpPoint);

        [DllImport("dwmapi.dll")]
        public static extern int DwmExtendFrameIntoClientArea(IntPtr hwnd, ref MARGINS margins);

        [DllImport("dwmapi.dll")]
        public static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref uint value, int size);

        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool SystemParametersInfoW(
            uint uiAction, uint uiParam, ref RECT pvParam, uint fWinIni);

        private const uint SPI_GETWORKAREA = 0x0030;

        public static bool TryGetWorkArea(out RECT workArea)
        {
            var rect = new RECT();
            if (SystemParametersInfoW(SPI_GETWORKAREA, 0, ref rect, 0))
            {
                workArea = rect;
                return true;
            }

            workArea = default;
            return false;
        }

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool SetWindowPos(
            IntPtr hWnd, IntPtr hWndInsertAfter, int x, int y, int cx, int cy, uint uFlags);
    }
}
