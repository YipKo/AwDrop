#if DEBUG
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.Foundation;

namespace AwDrop.Windows;

public sealed partial class MainWindow
{
    private int activationCount;

    internal async Task RunDesktopCheck(string path)
    {
        var report = new List<object>();
        try
        {
            await WaitForLayout();
            for (int i = 0; i < 3; ++i)
            {
                double opened = await MeasurePane(SettingsButton, true);
                Point origin = AppearanceBox.TransformToVisual(Shell).TransformPoint(new Point(0, 0));
                var language = Pattern<IExpandCollapseProvider>(LanguageBox, PatternInterface.ExpandCollapse);
                var expanded = Stopwatch.StartNew();
                language.Expand();
                if (!LanguageBox.IsDropDownOpen) throw new InvalidOperationException("Language list did not expand.");
                await WaitForLayout();
                Point during = AppearanceBox.TransformToVisual(Shell).TransformPoint(new Point(0, 0));
                if (Math.Abs(origin.X - during.X) > 0.5 || Math.Abs(origin.Y - during.Y) > 0.5)
                    throw new InvalidOperationException("Opening the language list moved the appearance field.");
                double layoutMilliseconds = expanded.Elapsed.TotalMilliseconds;
                language.Collapse();
                if (LanguageBox.IsDropDownOpen) throw new InvalidOperationException("Language list did not collapse.");
                double closed = await MeasurePane(CloseSettingsButton, false);
                report.Add(new { PaneOpenedMs = opened, LanguageLayoutMs = layoutMilliseconds, PaneClosedMs = closed });
            }
            report.Add(new
            {
                AcrylicAttached = SystemBackdrop is DesktopAcrylicBackdrop && Shell.Background is null,
                SystemAnimations = uiSettings.AnimationsEnabled,
                SystemTransparency = uiSettings.AdvancedEffectsEnabled,
            });
            if (Environment.GetEnvironmentVariable("AWDROP_CHECK_SETTINGS") is { Length: > 0 } folder)
            {
                report.Add(await CheckSettings(folder));
                report.Add(await CheckTransferProgress());
            }
            Invoke(ChooseFilesButton);
            await Task.Delay(250);
            report.Add(new { FilePickerPending = chooseInProgress, Status = FileStatus.Text });
        }
        catch (Exception error)
        {
            report.Add(new { Error = error.ToString() });
        }
        await File.WriteAllTextAsync(path, JsonSerializer.Serialize(report, new JsonSerializerOptions { WriteIndented = true }));
    }

    private async Task<object> CheckSettings(string folder)
    {
        if (ReceiveFolderPath.Text != global::Windows.Storage.UserDataPaths.GetDefault().Downloads)
            throw new InvalidOperationException("The default folder is not Windows Downloads.");
        DeviceNameBox.Text = "AwDrop check";
        Invoke(SaveDeviceNameButton);
        SaveReceiveFolder(folder);
        AskBeforeReceivingSwitch.IsOn = false;
        Preferences saved = Preferences.Load();
        if (saved.DeviceName != DeviceTitle.Text || saved.ReceiveFolder != folder || saved.AskBeforeReceiving)
            throw new InvalidOperationException("Receive settings were not saved.");

        AppearanceBox.SelectedIndex = 2;
        if (Shell.RequestedTheme != ElementTheme.Dark)
            throw new InvalidOperationException("Dark mode was not applied.");
        AppearanceBox.SelectedIndex = 0;

        bool trayChecked = tray.Registered;
        if (trayChecked)
        {
            HideToTray();
            if (AppWindow.IsVisible) throw new InvalidOperationException("The window did not hide to the tray.");
        }

        int before = activationCount;
        var start = new ProcessStartInfo(Environment.ProcessPath!);
        start.Environment.Remove("AWDROP_DESKTOP_CHECK");
        using var child = Process.Start(start)!;
        await child.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(10));
        for (int i = 0; i < 20 && activationCount == before; ++i) await Task.Delay(50);
        if (child.ExitCode != 0 || activationCount != before + 1 || !AppWindow.IsVisible)
            throw new InvalidOperationException("The existing window did not receive the second launch.");
        return new { DefaultDownloads = true, SettingsSaved = true, DarkModeApplied = true, SingleInstanceReused = true, TrayRestoreChecked = trayChecked };
    }

    private async Task<object> CheckTransferProgress()
    {
        if (TransferProgress.Visibility != Visibility.Collapsed)
            throw new InvalidOperationException("Transfer progress is visible without data.");
        var meter = (ProgressBar)TransferProgress.FindName("Meter");
        var speed = (TextBlock)TransferProgress.FindName("SpeedLabel");
        var english = new Strings("en");
        TransferProgress.SetPresentation(english, true);
        UpdateTransfer(new(TransferPhase.Receiving, 2, 10, 800_000));
        await WaitForLayout();
        UpdateTransfer(new(TransferPhase.Receiving, 5, 10, 1_500_000));
        UpdateTransfer(new(TransferPhase.Receiving, 10, 10, 2_000_000));
        await Task.Delay(450);
        if (meter.Value != 99 || speed.Text != "2.0 MB/s")
            throw new InvalidOperationException("The transfer view did not apply the most recent sample.");

        TransferProgress.SetPresentation(english, false);
        UpdateTransfer(new(TransferPhase.Sending, 10, null, null));
        if (meter.IsIndeterminate) throw new InvalidOperationException("Reduced motion left a looping indicator.");
        UpdateTransfer(new(TransferPhase.Sent, 10, 10, null));
        if (meter.Value != 100 || speed.Text.Length > 0)
            throw new InvalidOperationException("The completion state is incorrect.");
        UpdateTransfer(null);
        TransferProgress.SetPresentation(strings, !preferences.ReduceMotion && uiSettings.AnimationsEnabled);
        return new { TransferSamplesCoalesced = true, CompletionAt100 = true, TransferReducedMotion = true };
    }

    private async Task<double> MeasurePane(Button button, bool open)
    {
        var finished = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        void Complete(SplitView _, object args) => finished.TrySetResult();
        if (open) SettingsView.PaneOpened += Complete;
        else SettingsView.PaneClosed += Complete;
        var time = Stopwatch.StartNew();
        try
        {
            Invoke(button);
            await finished.Task.WaitAsync(TimeSpan.FromSeconds(2));
            if (SettingsView.IsPaneOpen != open) throw new InvalidOperationException("Unexpected pane state.");
            return time.Elapsed.TotalMilliseconds;
        }
        finally
        {
            if (open) SettingsView.PaneOpened -= Complete;
            else SettingsView.PaneClosed -= Complete;
        }
    }

    private async Task WaitForLayout()
    {
        var ready = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        void Updated(object? sender, object args) => ready.TrySetResult();
        Shell.LayoutUpdated += Updated;
        Shell.InvalidateMeasure();
        try { await ready.Task.WaitAsync(TimeSpan.FromSeconds(2)); }
        finally { Shell.LayoutUpdated -= Updated; }
    }

    private static void Invoke(Button button) => Pattern<IInvokeProvider>(button, PatternInterface.Invoke).Invoke();

    private static T Pattern<T>(UIElement control, PatternInterface pattern) where T : class
    {
        return FrameworkElementAutomationPeer.CreatePeerForElement(control)?.GetPattern(pattern) as T
            ?? throw new InvalidOperationException($"{pattern} is unavailable");
    }
}
#endif
