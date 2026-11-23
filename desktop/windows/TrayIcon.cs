using System;
using System.IO;
using System.Runtime.InteropServices;

namespace AwDrop.Windows;

internal sealed class TrayIcon : IDisposable
{
    private const uint CallbackMessage = 0x8000 + 0x35;
    private const uint SubclassId = 1;
    private const uint LeftUp = 0x0202;
    private const uint ContextMenu = 0x007B;
    private const uint KeySelect = 0x0401;
    private const uint Destroy = 0x0082;
    private const uint OpenCommand = 1;
    private const uint QuitCommand = 2;

    private readonly nint window;
    private readonly nint icon;
    private readonly uint taskbarCreated;
    private readonly SubclassProc callback;
    private readonly Action toggle, open, quit;
    private bool subclassed;
    private bool disposed;
    public string OpenText { get; set; } = "Open";
    public string QuitText { get; set; } = "Quit";
    public bool Registered { get; private set; }

    public TrayIcon(nint window, Action toggle, Action open, Action quit)
    {
        this.window = window;
        this.toggle = toggle;
        this.open = open;
        this.quit = quit;
        callback = Receive;
        taskbarCreated = RegisterWindowMessage("TaskbarCreated");
        icon = LoadImage(0, Path.Combine(AppContext.BaseDirectory, "AwDrop.ico"), 1,
            GetSystemMetrics(49), GetSystemMetrics(50), 0x10);
        if (icon == 0) return;
        subclassed = SetWindowSubclass(window, callback, SubclassId, 0);
        if (subclassed) Register();
    }

    private void Register()
    {
        var data = Data();
        Registered = ShellNotifyIcon(0, ref data);
        if (!Registered) return;
        data.Version = 4;
        if (!ShellNotifyIcon(4, ref data))
        {
            ShellNotifyIcon(2, ref data);
            Registered = false;
        }
    }

    private NotifyIconData Data() => new()
    {
        Size = (uint)Marshal.SizeOf<NotifyIconData>(),
        Window = window,
        Id = 1,
        Flags = 0x87, // callback, icon, tip and the version 4 tooltip
        Callback = CallbackMessage,
        Icon = icon,
        Tip = "AwDrop",
        Info = "",
        InfoTitle = "",
    };

    private nint Receive(nint hwnd, uint message, nuint wParam, nint lParam, nuint id, nuint reference)
    {
        if (message == CallbackMessage)
        {
            switch ((uint)lParam & 0xffff)
            {
                case LeftUp:
                case KeySelect:
                    toggle();
                    break;
                case ContextMenu:
                    ShowMenu(unchecked((short)(wParam & 0xffff)), unchecked((short)(wParam >> 16)));
                    break;
            }
            return 0;
        }
        if (message == taskbarCreated && taskbarCreated != 0) Register();
        if (message == Destroy) Dispose();
        return DefSubclassProc(hwnd, message, wParam, lParam);
    }

    private void ShowMenu(int x, int y)
    {
        nint menu = CreatePopupMenu();
        if (menu == 0) return;
        uint choice;
        try
        {
            AppendMenu(menu, 0, OpenCommand, OpenText);
            AppendMenu(menu, 0, QuitCommand, QuitText);
            SetForegroundWindow(window);
            choice = TrackPopupMenu(menu, 0x0182, x, y, 0, window, 0);
            PostMessage(window, 0, 0, 0);
        }
        finally { DestroyMenu(menu); }
        if (choice == OpenCommand) open();
        if (choice == QuitCommand) quit();
    }

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        if (Registered)
        {
            var data = Data();
            ShellNotifyIcon(2, ref data);
            Registered = false;
        }
        if (subclassed) RemoveWindowSubclass(window, callback, SubclassId);
        if (icon != 0) DestroyIcon(icon);
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NotifyIconData
    {
        public uint Size;
        public nint Window;
        public uint Id, Flags, Callback;
        public nint Icon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string Tip;
        public uint State, StateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string Info;
        public uint Version;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string InfoTitle;
        public uint InfoFlags;
        public Guid GuidItem;
        public nint BalloonIcon;
    }

    private delegate nint SubclassProc(nint hwnd, uint message, nuint wParam, nint lParam, nuint id, nuint reference);
    [DllImport("comctl32.dll")] private static extern bool SetWindowSubclass(nint hwnd, SubclassProc callback, nuint id, nuint reference);
    [DllImport("comctl32.dll")] private static extern bool RemoveWindowSubclass(nint hwnd, SubclassProc callback, nuint id);
    [DllImport("comctl32.dll")] private static extern nint DefSubclassProc(nint hwnd, uint message, nuint wParam, nint lParam);
    [DllImport("shell32.dll", EntryPoint = "Shell_NotifyIconW")] private static extern bool ShellNotifyIcon(uint message, ref NotifyIconData data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern nint LoadImage(nint instance, string name, uint type, int cx, int cy, uint flags);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern uint RegisterWindowMessage(string message);
    [DllImport("user32.dll")] private static extern int GetSystemMetrics(int metric);
    [DllImport("user32.dll")] private static extern bool DestroyIcon(nint icon);
    [DllImport("user32.dll")] private static extern nint CreatePopupMenu();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern bool AppendMenu(nint menu, uint flags, nuint id, string text);
    [DllImport("user32.dll")] private static extern uint TrackPopupMenu(nint menu, uint flags, int x, int y, int reserved, nint owner, nint rect);
    [DllImport("user32.dll")] private static extern bool SetForegroundWindow(nint hwnd);
    [DllImport("user32.dll")] private static extern bool DestroyMenu(nint menu);
    [DllImport("user32.dll", EntryPoint = "PostMessageW")] private static extern bool PostMessage(nint hwnd, uint message, nuint wParam, nint lParam);
}
