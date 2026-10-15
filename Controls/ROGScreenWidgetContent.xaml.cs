using System;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using RogControl.Platform;

namespace RogControl.Controls;

/// <summary>
/// ROG screen feature widget: a brightness slider that drives the primary
/// physical monitor over DDC/CI (works with ROG monitors, DDC/CI enabled by
/// default). The slider debounces writes so a fast drag does not spam the
/// display's control channel. Every native call is guarded so a monitor
/// without DDC/CI (or a stripped-down OS) degrades to a message instead of
/// crashing the widget window.
///
/// The dual-mode toggle switches the panel over VCP 0xFD; each switch makes
/// the panel re-handshake the link (new EDID, Windows re-enumerates the
/// display), which invalidates the cached physical-monitor handle. The
/// brightness side therefore reconnects on demand: a failed read/write
/// re-opens the handle, and the slider stays usable and lazily reconnects on
/// the next drag.
/// </summary>
public sealed partial class ROGScreenWidgetContent : UserControl
{
    private MonitorBrightnessController? _brightness;
    private readonly DispatcherTimer _applyTimer;
    private bool _restoringSlider;
    private bool _switchingDualMode;
    private bool _dualModeOn; // true = 1K (485 Hz), false = 4K (162 Hz)
    private int _pendingBrightness = -1;
    private int _brightnessRefreshVersion;

    public ROGScreenWidgetContent()
    {
        InitializeComponent();
        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
        BrightnessSlider.ValueChanged += BrightnessSlider_ValueChanged;

        _applyTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(200) };
        _applyTimer.Tick += (_, _) =>
        {
            _applyTimer.Stop();
            ApplyPendingBrightness();
        };
    }

    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        InitializeBrightness();
        InitializeDualMode();
    }

    private void OnUnloaded(object sender, RoutedEventArgs e)
    {
        _applyTimer.Stop();
        _brightness?.Dispose();
        _brightness = null;
    }

    private void InitializeBrightness()
    {
        EnsureBrightnessController();

        if (_brightness is not { IsSupported: true })
        {
            BrightnessPercentText.Text = "--";
            RetryBrightnessInit();
            return;
        }

        if (!TryRestoreBrightnessUi())
        {
            RetryBrightnessInit();
        }
    }

    /// <summary>
    /// Retries the startup brightness read with growing delays (2s/4s/6s):
    /// right after launch the DDC/CI channel can still be settling, and a
    /// transient failure should not leave the slider showing "--" forever.
    /// </summary>
    private int _brightnessInitVersion;

    private async void RetryBrightnessInit()
    {
        int version = ++_brightnessInitVersion;

        for (int attempt = 0; attempt < 3; attempt++)
        {
            await Task.Delay(TimeSpan.FromSeconds(2 + attempt * 2));

            if (version != _brightnessInitVersion)
            {
                return; // superseded
            }

            EnsureBrightnessController();
            if (_brightness is { IsSupported: true } && TryRestoreBrightnessUi())
            {
                return;
            }
        }

        if (version == _brightnessInitVersion)
        {
            ShowBrightnessUnsupported();
        }
    }

    /// <summary>Creates the brightness controller when it is missing or no
    /// longer usable (e.g. the monitor handle went stale after a dual-mode
    /// panel switch). Never throws.</summary>
    private void EnsureBrightnessController()
    {
        if (_brightness is { IsSupported: true })
        {
            return;
        }

        _brightness?.Dispose();
        _brightness = null;
        try
        {
            _brightness = new MonitorBrightnessController();
        }
        catch (Exception)
        {
            _brightness = null;
        }
    }

    /// <summary>Refreshes the slider and percentage from the current monitor
    /// state. Returns false (leaving the UI unchanged) when the read failed,
    /// so the caller can retry.</summary>
    private bool TryRestoreBrightnessUi()
    {
        _restoringSlider = true;
        try
        {
            BrightnessSlider.Value = _brightness!.GetBrightnessPercent();
        }
        catch (Exception)
        {
            _restoringSlider = false;
            return false;
        }
        finally
        {
            _restoringSlider = false;
        }

        BrightnessPercentText.Text = $"{BrightnessSlider.Value:0}%";
        StatusText.Visibility = Visibility.Collapsed;
        return true;
    }

    private void ShowBrightnessUnsupported()
    {
        StatusText.Text = string.IsNullOrEmpty(_brightness?.UnsupportedReason)
            ? "显示器不支持 DDC/CI 亮度调节"
            : $"不支持亮度调节：{_brightness!.UnsupportedReason}";
        StatusText.Visibility = Visibility.Visible;
        BrightnessPercentText.Text = "--";
    }

    /// <summary>Sets the custom switch to the current display mode.</summary>
    private void InitializeDualMode()
    {
        try
        {
            var current = RogScreenDualMode.GetCurrentMode();
            SetDualModeUi(current == RogScreenDualMode.DualMode.Mode1K);
            DualModeSwitchContainer.IsHitTestVisible = current != RogScreenDualMode.DualMode.Unknown;
        }
        catch (Exception)
        {
            DualModeSwitchContainer.IsHitTestVisible = false;
        }
    }

    /// <summary>Updates the custom switch visuals (red dot position).</summary>
    private void SetDualModeUi(bool on)
    {
        _dualModeOn = on;
        DualModeThumb.HorizontalAlignment = on ? HorizontalAlignment.Right : HorizontalAlignment.Left;
        DualModeThumb.Margin = on ? new Thickness(0, 0, 5, 0) : new Thickness(5, 0, 0, 0);
    }

    private async void DualModeSwitchContainer_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        if (_switchingDualMode)
        {
            return;
        }

        _switchingDualMode = true;
        DualModeSwitchContainer.IsHitTestVisible = false;

        var target = _dualModeOn
            ? RogScreenDualMode.DualMode.Mode4K
            : RogScreenDualMode.DualMode.Mode1K;

        // VCP write is a blocking native call; run it off the UI thread.
        string error = await Task.Run(() => RogScreenDualMode.SwitchTo(target));

        if (string.IsNullOrEmpty(error))
        {
            // Sync the switch with the mode the display actually ended in.
            // If the state read fails right after the panel re-handshake
            // (EDID still settling), keep the intended target instead of
            // flipping the switch to the wrong side.
            var now = RogScreenDualMode.GetCurrentMode();
            if (now != RogScreenDualMode.DualMode.Unknown)
            {
                SetDualModeUi(now == RogScreenDualMode.DualMode.Mode1K);
            }

            // The panel re-handshake re-enumerates the display, which can
            // invalidate the brightness controller's cached monitor handle.
            // Rebuild it (with retries) so the slider keeps working.
            RefreshBrightnessAfterDualModeSwitch();
        }
        else
        {
            StatusText.Text = "双模切换失败：" + error;
            StatusText.Visibility = Visibility.Visible;

            SetDualModeUi(!_dualModeOn);
        }

        DualModeSwitchContainer.IsHitTestVisible = true;
        _switchingDualMode = false;
    }

    private void BrightnessSlider_ValueChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (_restoringSlider)
        {
            return;
        }

        int percent = (int)Math.Round(e.NewValue);
        BrightnessPercentText.Text = $"{percent}%";
        _pendingBrightness = percent;

        // Debounce: apply shortly after the thumb settles.
        _applyTimer.Stop();
        _applyTimer.Start();
    }

    private void ApplyPendingBrightness()
    {
        if (_pendingBrightness < 0)
        {
            return;
        }

        int percent = _pendingBrightness;
        _pendingBrightness = -1;

        // Reconnect if the controller went stale (dual-mode switch etc.).
        EnsureBrightnessController();
        if (_brightness is not { IsSupported: true })
        {
            return;
        }

        try
        {
            _brightness.SetBrightnessPercent(percent);
            StatusText.Visibility = Visibility.Collapsed;
        }
        catch (Exception ex)
        {
            StatusText.Text = "调节失败：" + ex.Message;
            StatusText.Visibility = Visibility.Visible;
        }
    }

    /// <summary>
    /// Rebuilds the brightness controller after a dual-mode switch, retrying
    /// with growing delays because the DDC/CI channel needs time to settle
    /// after the panel re-handshake. A newer switch supersedes an in-flight
    /// refresh. If every attempt fails, the reason is shown but the slider
    /// stays usable and reconnects lazily on the next drag.
    /// </summary>
    private async void RefreshBrightnessAfterDualModeSwitch()
    {
        int version = ++_brightnessRefreshVersion;

        for (int attempt = 0; attempt < 3; attempt++)
        {
            await Task.Delay(TimeSpan.FromSeconds(2 + attempt * 2));

            if (version != _brightnessRefreshVersion)
            {
                return; // superseded by a newer dual-mode switch
            }

            EnsureBrightnessController();
            if (_brightness is { IsSupported: true } && TryRestoreBrightnessUi())
            {
                return;
            }
        }

        ShowBrightnessUnsupported();
    }
}
