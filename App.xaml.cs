using System.Diagnostics;
using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;

namespace RogControl;

public partial class App : Application
{
    private const string SingleInstanceMutexName = @"Local\RogControl_SingleInstance";
    private static Mutex? _singleInstanceMutex;
    private Window? _window;

    private const int SW_RESTORE = 9;

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        // 单实例：已有 RogControl 在运行则不再创建第二个，
        // 只把已运行实例的主窗口拉到前台，然后退出当前进程。
        _singleInstanceMutex = new Mutex(true, SingleInstanceMutexName, out bool createdNew);
        if (!createdNew)
        {
            ActivateExistingInstance();
            Environment.Exit(0);
            return;
        }

        _window = new MainWindow();
        _window.Activate();
    }

    private static void ActivateExistingInstance()
    {
        string processName;
        using (var self = Process.GetCurrentProcess())
        {
            processName = self.ProcessName;
        }

        foreach (var proc in Process.GetProcessesByName(processName))
        {
            if (proc.Id == Environment.ProcessId)
                continue;

            try
            {
                IntPtr hwnd = proc.MainWindowHandle;
                if (hwnd != IntPtr.Zero)
                {
                    ShowWindow(hwnd, SW_RESTORE);
                    SetForegroundWindow(hwnd);
                    return;
                }
            }
            finally
            {
                proc.Dispose();
            }
        }
    }
}
