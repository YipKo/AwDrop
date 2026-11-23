using System;
using System.Runtime.InteropServices;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Microsoft.Windows.AppLifecycle;
using Microsoft.UI.Xaml;
using Windows.ApplicationModel.Activation;

namespace AwDrop.Windows;

public partial class App : Application
{
    private MainWindow? window;
    private AppInstance? instance;
    private DispatcherQueue? dispatcher;

    public App() => InitializeComponent();

    protected override async void OnLaunched(Microsoft.UI.Xaml.LaunchActivatedEventArgs args)
    {
        var activation = AppInstance.GetCurrent().GetActivatedEventArgs();
        instance = AppInstance.FindOrRegisterForKey("AwDrop.MainWindow");
        if (!instance.IsCurrent)
        {
            AllowSetForegroundWindow(instance.ProcessId);
            await instance.RedirectActivationToAsync(activation);
            Exit();
            return;
        }
        dispatcher = DispatcherQueue.GetForCurrentThread();
        instance.Activated += RedirectedLaunch;
        window = new MainWindow();
        await ActivateWindow(window, activation);
#if DEBUG
        if (System.Environment.GetEnvironmentVariable("AWDROP_DESKTOP_CHECK") is { Length: > 0 } resultPath)
            _ = window.RunDesktopCheck(resultPath);
#endif
    }

    private void RedirectedLaunch(object? sender, AppActivationArguments args)
    {
        dispatcher?.TryEnqueue(async () =>
        {
            if (window is { } target) await ActivateWindow(target, args);
        });
    }

    private static async Task ActivateWindow(MainWindow target, AppActivationArguments args)
    {
        if (args.Data is ShareTargetActivatedEventArgs share)
            await target.StageShare(share.ShareOperation);
        else
            target.BringToFront();
    }

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool AllowSetForegroundWindow(uint processId);
}
