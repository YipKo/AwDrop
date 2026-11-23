using System;
using System.Globalization;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;

namespace AwDrop.Windows;

internal enum TransferPhase
{
    WaitingForAcceptance, Sending, Receiving, Saving, Sent, Received, Cancelled, Failed,
}

internal sealed record TransferSnapshot(TransferPhase Phase, long BytesTransferred, long? TotalBytes, double? BytesPerSecond);

public sealed partial class TransferProgressView : UserControl
{
    private static readonly string[] Units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    private readonly DispatcherQueueTimer updateTimer;
    private TransferSnapshot? current;
    private Strings? strings;
    private bool animate;

    public TransferProgressView()
    {
        InitializeComponent();
        updateTimer = DispatcherQueue.CreateTimer();
        updateTimer.Interval = TimeSpan.FromMilliseconds(250);
        updateTimer.IsRepeating = false;
        updateTimer.Tick += (_, _) => Render();
        Meter.Loaded += (_, _) => ApplyMotionPreference();
        Unloaded += (_, _) => updateTimer.Stop();
    }

    internal void Update(TransferSnapshot? snapshot)
    {
        bool phaseChanged = current?.Phase != snapshot?.Phase;
        current = snapshot;
        if (phaseChanged || snapshot is null)
        {
            updateTimer.Stop();
            Render();
        }
        else if (!updateTimer.IsRunning)
        {
            updateTimer.Start();
        }
    }

    internal void SetPresentation(Strings language, bool allowMotion)
    {
        strings = language;
        animate = allowMotion;
        ApplyMotionPreference();
        if (!animate && SpeedLabel.Text.Length > 0)
            ElementCompositionPreview.GetElementVisual(SpeedLabel).StopAnimation("Opacity");
        if (current is not null) Render();
    }

    private void Render()
    {
        if (current is not { } snapshot || strings is null)
        {
            Meter.IsIndeterminate = false;
            Visibility = Visibility.Collapsed;
            return;
        }

        Visibility = Visibility.Visible;
        bool completed = snapshot.Phase is TransferPhase.Sent or TransferPhase.Received;
        bool transferring = snapshot.Phase is TransferPhase.Sending or TransferPhase.Receiving;
        string phase = strings[snapshot.Phase switch
        {
            TransferPhase.WaitingForAcceptance => "Waiting for the other device to accept",
            TransferPhase.Sending => "Sending",
            TransferPhase.Receiving => "Receiving",
            TransferPhase.Saving => "Saving",
            TransferPhase.Sent => "Sent",
            TransferPhase.Received => "Received",
            TransferPhase.Cancelled => "Cancelled",
            _ => "The transfer couldn't finish.",
        }];
        if (PhaseLabel.Text != phase)
        {
            PhaseLabel.Text = phase;
            AutomationProperties.SetName(Meter, phase);
        }
        DirectionIcon.Glyph = snapshot.Phase switch
        {
            TransferPhase.Receiving or TransferPhase.Saving or TransferPhase.Received => "\uE896",
            TransferPhase.Sending or TransferPhase.WaitingForAcceptance or TransferPhase.Sent => "\uE898",
            _ => "\uEA39",
        };

        var culture = CultureInfo.GetCultureInfo(strings.Code);
        long transferred = Math.Max(0, snapshot.BytesTransferred);
        long total = snapshot.TotalBytes.GetValueOrDefault();
        bool hasAmount = completed || total > 0;
        Meter.IsIndeterminate = transferring && !hasAmount && animate;
        Meter.Visibility = hasAmount || Meter.IsIndeterminate ? Visibility.Visible : Visibility.Collapsed;
        Meter.ShowError = snapshot.Phase == TransferPhase.Failed;
        double progress = completed ? 100 : total > 0 ? Math.Min(99, 100.0 * transferred / total) : 0;
        Meter.Value = progress;
        string amount = FormatSize(transferred, culture);
        if (total > 0) amount = $"{amount} / {FormatSize(total, culture)}";
        AmountLabel.Text = hasAmount ? $"{amount} · {Math.Floor(progress).ToString("0", culture)}%" : amount;

        string speed = transferring && snapshot.BytesPerSecond is double rate && double.IsFinite(rate) && rate >= 0
            ? $"{FormatSize(rate, culture)}/s" : "";
        UpdateSpeed(speed);
    }

    private void UpdateSpeed(string value)
    {
        if (SpeedLabel.Text == value) return;
        bool hadSpeed = SpeedLabel.Text.Length > 0;
        SpeedLabel.Text = value;
        var visual = ElementCompositionPreview.GetElementVisual(SpeedLabel);
        visual.StopAnimation("Opacity");
        visual.Opacity = 1;
        if (!animate || !hadSpeed || value.Length == 0 || !IsLoaded) return;

        var fade = visual.Compositor.CreateScalarKeyFrameAnimation();
        fade.InsertKeyFrame(0, 0.70f);
        fade.InsertKeyFrame(1, 1);
        fade.Duration = TimeSpan.FromMilliseconds(120);
        visual.StartAnimation("Opacity", fade);
    }

    private void ApplyMotionPreference()
    {
        if (VisualTreeHelper.GetChildrenCount(Meter) == 0 ||
            VisualTreeHelper.GetChild(Meter, 0) is not FrameworkElement root) return;

        // The native bar uses a reposition transition whenever its value changes.
        foreach (VisualStateGroup group in VisualStateManager.GetVisualStateGroups(root))
        foreach (VisualTransition transition in group.Transitions)
        {
            if (transition.Storyboard is { } storyboard)
                storyboard.SpeedRatio = animate ? 1 : 1000;
        }
    }

    private static string FormatSize(double amount, CultureInfo culture)
    {
        int unit = 0;
        while (amount >= 1000 && unit < Units.Length - 1)
        {
            amount /= 1000;
            ++unit;
        }
        return $"{amount.ToString(unit == 0 ? "0" : "0.0", culture)} {Units[unit]}";
    }
}
