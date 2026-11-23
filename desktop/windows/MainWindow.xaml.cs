using System;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Threading.Tasks;
using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Windows.ApplicationModel.DataTransfer;
using Windows.ApplicationModel.DataTransfer.ShareTarget;
using Windows.Graphics;
using Windows.Storage;
using Windows.Storage.Pickers;
using Windows.UI.ViewManagement;

namespace AwDrop.Windows;

public sealed partial class MainWindow : Window
{
    private static readonly string[] AppearanceKeys = ["system", "light", "dark"];
    private readonly Preferences preferences = Preferences.Load();
    private readonly ObservableCollection<StagedFile> files = new();
    private readonly UISettings uiSettings = new();
    private readonly nint hwnd;
    private readonly TrayIcon tray;
    private Strings strings;
    private string[]? adapters;
    private bool applyingSettings;
    private bool chooseInProgress;
    private bool quitting;

    public MainWindow()
    {
        strings = new Strings(preferences.Language);
        applyingSettings = true;
        InitializeComponent();
        hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "AwDrop.ico"));
        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.SetBorderAndTitleBar(false, false);
            presenter.IsResizable = false;
            presenter.IsMaximizable = false;
        }
        int rounding = 2; // DWMWCP_ROUND
        DwmSetWindowAttribute(hwnd, 33, ref rounding, sizeof(int)); // DWMWA_WINDOW_CORNER_PREFERENCE
        if (DesktopAcrylicController.IsSupported())
        {
            SystemBackdrop = new DesktopAcrylicBackdrop();
            Shell.Background = null;
            BackdropTint.Visibility = Visibility.Visible;
        }
        FilesList.ItemsSource = files;
        DeviceNameBox.Text = DisplayDeviceName;
        SettingsView.Loaded += (_, _) => ConfigurePaneMotion();
        Shell.SizeChanged += (_, args) =>
        {
            SettingsView.OpenPaneLength = Math.Clamp(args.NewSize.Width - 40, 160, 318);
            MainGrid.Padding = new Thickness(22, 20, 22, Math.Clamp(args.NewSize.Height * 0.125, 20, 70));
        };
        Shell.KeyboardAccelerators.Add(new Microsoft.UI.Xaml.Input.KeyboardAccelerator
        {
            Key = global::Windows.System.VirtualKey.Escape,
        });
        Shell.KeyboardAccelerators[0].Invoked += (_, args) =>
        {
            if (!SettingsView.IsPaneOpen) return;
            SettingsView.IsPaneOpen = false;
            args.Handled = true;
        };

        tray = new TrayIcon(
            hwnd,
            () => DispatcherQueue.TryEnqueue(() =>
            {
                if (AppWindow.IsVisible) HideToTray();
                else ShowFromTray();
            }),
            () => DispatcherQueue.TryEnqueue(ShowFromTray),
            () => DispatcherQueue.TryEnqueue(Quit));
        AppWindow.Closing += (_, args) =>
        {
            if (quitting || !tray.Registered) return;
            args.Cancel = true;
            HideToTray();
        };
        Closed += (_, _) => tray.Dispose();
        ApplyPreferences();
        UpdateFiles();
        PlaceWindow();
        _ = RefreshStatus();
    }

    private void ApplyPreferences()
    {
        applyingSettings = true;
        try
        {
            LanguageBox.ItemsSource = Strings.Languages.Select(item => item.Name).ToArray();
            LanguageBox.SelectedIndex = Array.FindIndex(Strings.Languages, item => item.Code == strings.Code);
            LanguageLabel.Text = strings["Language"];
            AutomationProperties.SetName(LanguageBox, LanguageLabel.Text);
            AppearanceBox.ItemsSource = new[] { strings["System"], strings["Light"], strings["Dark"] };
            AppearanceBox.SelectedIndex = Math.Max(0, Array.IndexOf(AppearanceKeys, preferences.Appearance));
            AppearanceLabel.Text = strings["Appearance"];
            AutomationProperties.SetName(AppearanceBox, AppearanceLabel.Text);
            Shell.RequestedTheme = preferences.Appearance switch
            {
                "light" => ElementTheme.Light,
                "dark" => ElementTheme.Dark,
                _ => ElementTheme.Default,
            };
            ReduceMotionLabel.Text = strings["Reduce motion"];
            AutomationProperties.SetName(ReduceMotionSwitch, ReduceMotionLabel.Text);
            ReduceMotionSwitch.IsOn = preferences.ReduceMotion;
            FilesList.ItemContainerTransitions = new TransitionCollection();
            if (!preferences.ReduceMotion && uiSettings.AnimationsEnabled)
                FilesList.ItemContainerTransitions.Add(new AddDeleteThemeTransition());
            ConfigurePaneMotion();
            TransferProgress.SetPresentation(strings, AppWindow.IsVisible && !preferences.ReduceMotion && uiSettings.AnimationsEnabled);
            SettingsTitle.Text = strings["Settings"];
            SetButtonLabel(SettingsButton, "Settings");
            SetButtonLabel(CloseSettingsButton, "Close settings");
            SetButtonLabel(HideButton, "Hide to tray");
            DeviceLabel.Text = strings["Name other devices see"];
            AutomationProperties.SetName(DeviceNameBox, DeviceLabel.Text);
            SaveDeviceNameButton.Content = strings["Save"];
            DeviceTitle.Text = DisplayDeviceName;
            ReceiveFolderLabel.Text = strings["Received files go to"];
            ReceiveFolderPath.Text = preferences.ReceiveFolder ?? UserDataPaths.GetDefault().Downloads;
            ChooseReceiveFolderButton.Content = strings["Change folder"];
            AskBeforeReceivingLabel.Text = strings["Ask before receiving files"];
            AutomationProperties.SetName(AskBeforeReceivingSwitch, AskBeforeReceivingLabel.Text);
            AskBeforeReceivingSwitch.IsOn = preferences.AskBeforeReceiving;
            PresenceLabel.Text = strings["Not visible"];
            RadioLabel.Text = strings["Radio"];
            AutomationProperties.SetName(RadioSwitch, strings["Radio"]);
            AdapterLabel.Text = strings["Wi-Fi adapter"];
            RefreshLabel.Text = strings["Refresh"];
            SetButtonLabel(RefreshButton, "Refresh");
            NearbyLabel.Text = strings["Nearby"];
            NearbyTitle.Text = strings["Can't connect to nearby devices yet"];
            NearbyMessage.Text = strings["You can choose the files you want to send."];
            DropHint.Text = strings["Drop files here"];
            ClearFilesButton.Content = strings["Clear"];
            tray.OpenText = strings["Open"];
            tray.QuitText = strings["Quit"];
            UpdateAdapterLabel();
            for (int i = 0; i < files.Count; ++i)
                files[i] = files[i] with { RemoveLabel = RemoveLabel(files[i].Name) };
            UpdateFiles();
        }
        finally { applyingSettings = false; }
    }

    private void SetButtonLabel(Button button, string key)
    {
        AutomationProperties.SetName(button, strings[key]);
        ToolTipService.SetToolTip(button, strings[key]);
    }

    private void ConfigurePaneMotion()
    {
        if (VisualTreeHelper.GetChildrenCount(SettingsView) == 0 ||
            VisualTreeHelper.GetChild(SettingsView, 0) is not FrameworkElement templateRoot)
            return;

        bool reduced = preferences.ReduceMotion || !uiSettings.AnimationsEnabled;
        // Tune only the right overlay transitions in the pinned WinUI template.
        // Keep the standard focus, clipping, and light-dismiss behavior.
        foreach (VisualStateGroup group in VisualStateManager.GetVisualStateGroups(templateRoot))
        foreach (VisualTransition transition in group.Transitions)
        {
            if (transition.Storyboard is not { } storyboard) continue;
            if (transition.From == "Closed" && transition.To == "OpenOverlayRight")
                storyboard.SpeedRatio = reduced ? 350 : 350.0 / 167.0;
            if (transition.From == "OpenOverlayRight" && transition.To == "Closed")
                storyboard.SpeedRatio = reduced ? 120 : 1;
        }
    }

    private void UpdateAdapterLabel() => AdapterStatus.Text = adapters switch
    {
        null => strings["Could not check the Wi-Fi adapter."],
        { Length: 0 } => strings["No Wi-Fi adapter recognized"],
        _ => string.Join(", ", adapters),
    };

    private async Task RefreshStatus()
    {
        if (!RefreshButton.IsEnabled) return;
        RefreshButton.IsEnabled = false;
        try { adapters = (await Bridge.ReadStatus()).Adapters; }
        catch (Exception error) when (error is IOException or Win32Exception or JsonException or OperationCanceledException)
        {
            adapters = null;
        }
        finally
        {
            UpdateAdapterLabel();
            RefreshButton.IsEnabled = true;
        }
    }

    private void StorePreferences()
    {
        try { preferences.Save(); }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            FileStatus.Text = strings["Could not save preferences."];
        }
    }

    private string DisplayDeviceName => string.IsNullOrWhiteSpace(preferences.DeviceName)
        ? Environment.MachineName : preferences.DeviceName;

    private void SaveDeviceNameClicked(object sender, RoutedEventArgs args) => SaveDeviceName();

    private void DeviceNameKeyDown(object sender, Microsoft.UI.Xaml.Input.KeyRoutedEventArgs args)
    {
        if (args.Key != global::Windows.System.VirtualKey.Enter) return;
        args.Handled = true;
        SaveDeviceName();
    }

    private void SaveDeviceName()
    {
        string name = DeviceNameBox.Text.Trim();
        if (name.Length == 0 || name.Any(char.IsControl))
        {
            SetStatus(DeviceNameStatus, "Enter a device name.");
            return;
        }
        string? previous = preferences.DeviceName;
        preferences.DeviceName = name;
        if (!TrySaveSetting(DeviceNameStatus))
        {
            preferences.DeviceName = previous;
            return;
        }
        DeviceNameBox.Text = DeviceTitle.Text = name;
    }

    private async void ChooseReceiveFolderClicked(object sender, RoutedEventArgs args)
    {
        if (chooseInProgress) return;
        chooseInProgress = true;
        try
        {
            var picker = new FolderPicker { SuggestedStartLocation = PickerLocationId.Downloads };
            WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);
            picker.FileTypeFilter.Add("*");
            if (await picker.PickSingleFolderAsync() is not { Path.Length: > 0 } folder) return;
            SaveReceiveFolder(folder.Path);
        }
        catch (Exception error) when (error is COMException or UnauthorizedAccessException or IOException)
        {
            SetStatus(ReceiveFolderStatus, "Could not open the file picker.");
        }
        finally { chooseInProgress = false; }
    }

    private void SaveReceiveFolder(string path)
    {
        string? previous = preferences.ReceiveFolder;
        preferences.ReceiveFolder = path;
        if (TrySaveSetting(ReceiveFolderStatus)) ReceiveFolderPath.Text = path;
        else preferences.ReceiveFolder = previous;
    }

    private void AskBeforeReceivingChanged(object sender, RoutedEventArgs args)
    {
        if (applyingSettings) return;
        bool previous = preferences.AskBeforeReceiving;
        preferences.AskBeforeReceiving = AskBeforeReceivingSwitch.IsOn;
        if (TrySaveSetting(SettingsStatus)) return;
        preferences.AskBeforeReceiving = previous;
        RestoreToggle(AskBeforeReceivingSwitch, previous);
    }

    private void RestoreToggle(ToggleSwitch control, bool value)
    {
        applyingSettings = true;
        try { control.IsOn = value; }
        finally { applyingSettings = false; }
    }

    private bool TrySaveSetting(TextBlock status)
    {
        try { preferences.Save(); }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            SetStatus(status, "Could not save preferences.");
            return false;
        }
        SetStatus(status, null);
        return true;
    }

    private void SetStatus(TextBlock status, string? key)
    {
        status.Text = key is null ? "" : strings[key];
        status.Visibility = key is null ? Visibility.Collapsed : Visibility.Visible;
    }

    private void LanguageChanged(object sender, SelectionChangedEventArgs args)
    {
        if (applyingSettings || LanguageBox.SelectedIndex < 0) return;
        preferences.Language = Strings.Languages[LanguageBox.SelectedIndex].Code;
        strings = new Strings(preferences.Language);
        ApplyPreferences();
        StorePreferences();
    }

    private void AppearanceChanged(object sender, SelectionChangedEventArgs args)
    {
        if (applyingSettings || AppearanceBox.SelectedIndex < 0) return;
        preferences.Appearance = AppearanceKeys[AppearanceBox.SelectedIndex];
        ApplyPreferences();
        StorePreferences();
    }

    private void ReduceMotionChanged(object sender, RoutedEventArgs args)
    {
        if (applyingSettings) return;
        preferences.ReduceMotion = ReduceMotionSwitch.IsOn;
        ApplyPreferences();
        StorePreferences();
    }

    private async void RefreshClicked(object sender, RoutedEventArgs args) => await RefreshStatus();
    private void SettingsClicked(object sender, RoutedEventArgs args) => SettingsView.IsPaneOpen = true;
    private void CloseSettingsClicked(object sender, RoutedEventArgs args) => SettingsView.IsPaneOpen = false;
    private void SettingsOpened(SplitView sender, object args) => LanguageBox.Focus(FocusState.Programmatic);
    private void SettingsClosed(SplitView sender, object args) => SettingsButton.Focus(FocusState.Programmatic);
    private void HideClicked(object sender, RoutedEventArgs args) => HideToTray();

    private async void ChooseFilesClicked(object sender, RoutedEventArgs args)
    {
        if (chooseInProgress) return;
        chooseInProgress = true;
        try
        {
            var picker = new FileOpenPicker();
            WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);
            picker.FileTypeFilter.Add("*");
            Stage(await picker.PickMultipleFilesAsync());
        }
        catch (Exception error) when (error is COMException or UnauthorizedAccessException or IOException)
        {
            FileStatus.Text = strings["Could not open the file picker."];
        }
        finally { chooseInProgress = false; }
    }

    private void FilesDragOver(object sender, DragEventArgs args) => args.AcceptedOperation =
        args.DataView.Contains(StandardDataFormats.StorageItems) ? DataPackageOperation.Copy : DataPackageOperation.None;

    private async void FilesDropped(object sender, DragEventArgs args)
    {
        if (!args.DataView.Contains(StandardDataFormats.StorageItems)) return;
        var deferral = args.GetDeferral();
        try { Stage((await args.DataView.GetStorageItemsAsync()).OfType<StorageFile>()); }
        catch (Exception error) when (error is COMException or UnauthorizedAccessException or IOException)
        {
            FileStatus.Text = strings["Couldn't open this file."];
        }
        finally { deferral.Complete(); }
    }

    private void Stage(System.Collections.Generic.IEnumerable<StorageFile> selection)
    {
        foreach (StorageFile file in selection)
        {
            if (string.IsNullOrEmpty(file.Path) || files.Any(item => StringComparer.OrdinalIgnoreCase.Equals(item.Path, file.Path)))
                continue;
            files.Add(new(file.Path, file.Name, RemoveLabel(file.Name)));
        }
        FileStatus.Text = "";
        UpdateFiles();
    }

    internal async Task StageShare(ShareOperation operation)
    {
        ShowFromTray();
        SettingsView.IsPaneOpen = false;
        operation.ReportStarted();
        try
        {
            if (!operation.Data.Contains(StandardDataFormats.StorageItems))
            {
                operation.ReportError(strings["A file could not be added."]);
                return;
            }
            var items = await operation.Data.GetStorageItemsAsync();
            var selection = items.OfType<StorageFile>().Where(file => !string.IsNullOrEmpty(file.Path)).ToArray();
            if (selection.Length == 0)
            {
                operation.ReportError(strings["A file could not be added."]);
                return;
            }
            Stage(selection);
            if (selection.Length < items.Count) FileStatus.Text = strings["A file could not be added."];
            operation.ReportCompleted();
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or COMException)
        {
            operation.ReportError(strings["A file could not be added."]);
        }
    }

    internal void BringToFront()
    {
#if DEBUG
        ++activationCount;
#endif
        ShowFromTray();
    }

    internal void UpdateTransfer(TransferSnapshot? snapshot) => TransferProgress.Update(snapshot);

    private string RemoveLabel(string name) => string.Format(strings["Remove {0}"], name);

    private void RemoveFileClicked(object sender, RoutedEventArgs args)
    {
        if (sender is Button { Tag: string path } && files.FirstOrDefault(item => item.Path == path) is { } item)
            files.Remove(item);
        UpdateFiles();
    }

    private void ClearFilesClicked(object sender, RoutedEventArgs args)
    {
        files.Clear();
        UpdateFiles();
        ChooseFilesButton.Focus(FocusState.Programmatic);
    }

    private void UpdateFiles()
    {
        bool hasFiles = files.Count != 0;
        FilesLabel.Text = hasFiles ? $"{strings["Files to send"]} ({files.Count})" : strings["Files to send"];
        FilesList.Visibility = ClearFilesButton.Visibility = hasFiles ? Visibility.Visible : Visibility.Collapsed;
        DropHint.Visibility = hasFiles ? Visibility.Collapsed : Visibility.Visible;
        string chooseLabel = hasFiles ? "Add files" : "Choose files";
        ChooseFilesLabel.Text = strings[chooseLabel];
        AutomationProperties.SetName(ChooseFilesButton, strings[chooseLabel]);
    }

    private void PlaceWindow()
    {
        DisplayArea display = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Primary);
        RectInt32 work = display.WorkArea;
        double scale = Math.Max(96u, GetDpiForWindow(hwnd)) / 96.0;
        int gap = (int)Math.Round(12 * scale);
        int width = Math.Min((int)Math.Round(448 * scale), Math.Max(1, work.Width - 2 * gap));
        int height = Math.Min((int)Math.Round(560 * scale), Math.Max(1, work.Height - 2 * gap));
        AppWindow.MoveAndResize(new RectInt32(work.X + work.Width - width - gap, work.Y + work.Height - height - gap, width, height));
    }

    private void ShowFromTray()
    {
        if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized } presenter)
            presenter.Restore();
        AppWindow.Show();
        Activate();
        TransferProgress.SetPresentation(strings, !preferences.ReduceMotion && uiSettings.AnimationsEnabled);
    }
    private void HideToTray()
    {
        if (chooseInProgress) return;
        if (tray.Registered)
        {
            AppWindow.Hide();
            TransferProgress.SetPresentation(strings, false);
        }
        else Quit();
    }
    private void Quit() { quitting = true; Close(); }

    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(nint hwnd);
    [DllImport("dwmapi.dll")] private static extern int DwmSetWindowAttribute(nint hwnd, int attribute, ref int value, int size);
}

public sealed record StagedFile(string Path, string Name, string RemoveLabel);
