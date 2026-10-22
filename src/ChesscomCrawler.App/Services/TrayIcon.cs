using System.Runtime.InteropServices;

namespace Jupiter.ChesscomCrawler.App.Services;

/// <summary>
/// A notification-area icon with a context menu, owned by a hidden top-level window on the UI
/// thread. Re-adds itself when Explorer restarts.
/// </summary>
internal sealed unsafe partial class TrayIcon : IDisposable
{
    public sealed record MenuItem(string Text, Action Invoke, bool Enabled = true);

    private const uint CallbackMessage = Win32.WM_APP + 1;
    private const uint IconId = 1;

    private static TrayIcon? s_current;
    private static readonly uint s_taskbarCreated = Win32.RegisterWindowMessageW("TaskbarCreated");

    private readonly nint _window;
    private readonly nint _icon;
    private readonly Func<IReadOnlyList<MenuItem?>> _menu;
    private string _tooltip;
    private bool _disposed;

    public event Action? Activated;

    public TrayIcon(string iconPath, string tooltip, Func<IReadOnlyList<MenuItem?>> menu)
    {
        if (s_current is not null)
        {
            throw new InvalidOperationException("Only one tray icon is supported.");
        }
        _menu = menu;
        _tooltip = tooltip;
        var instance = Win32.GetModuleHandleW(null);
        fixed (char* className = "Jupiter.ChesscomCrawler.Tray")
        {
            var wc = new Win32.WNDCLASSEXW
            {
                cbSize = (uint)sizeof(Win32.WNDCLASSEXW),
                lpfnWndProc = &WndProc,
                hInstance = instance,
                lpszClassName = className,
            };
            Win32.RegisterClassExW(&wc);
            _window = Win32.CreateWindowExW(0, className, className, Win32.WS_POPUP, 0, 0, 0, 0, 0, 0, instance, 0);
        }
        if (_window == 0)
        {
            throw new InvalidOperationException($"Creating the notification-area window failed ({Marshal.GetLastPInvokeError()}).");
        }
        var size = Win32.GetSystemMetricsForDpi(Win32.SM_CXSMICON, Win32.GetDpiForSystem());
        _icon = Win32.LoadImageW(0, iconPath, Win32.IMAGE_ICON, size, size, Win32.LR_LOADFROMFILE);
        s_current = this;
        Add();
    }

    public string Tooltip
    {
        get => _tooltip;
        set
        {
            if (_tooltip == value || _disposed)
            {
                return;
            }
            _tooltip = value;
            var data = Data(Win32.NIF_TIP | Win32.NIF_SHOWTIP);
            Win32.Shell_NotifyIconW(Win32.NIM_MODIFY, &data);
        }
    }

    private Win32.NOTIFYICONDATAW Data(uint flags)
    {
        var data = new Win32.NOTIFYICONDATAW
        {
            cbSize = (uint)sizeof(Win32.NOTIFYICONDATAW),
            hWnd = _window,
            uID = IconId,
            uFlags = flags,
            uCallbackMessage = CallbackMessage,
            hIcon = _icon,
        };
        var tip = _tooltip.Length > 127 ? _tooltip[..127] : _tooltip;
        tip.AsSpan().CopyTo(new Span<char>(data.szTip, 128));
        return data;
    }

    private void Add()
    {
        var data = Data(Win32.NIF_MESSAGE | Win32.NIF_ICON | Win32.NIF_TIP | Win32.NIF_SHOWTIP);
        Win32.Shell_NotifyIconW(Win32.NIM_ADD, &data);
        data.uVersion = Win32.NOTIFYICON_VERSION_4;
        Win32.Shell_NotifyIconW(Win32.NIM_SETVERSION, &data);
    }

    private void ShowMenu(int x, int y)
    {
        var items = _menu();
        var menu = Win32.CreatePopupMenu();
        try
        {
            for (var i = 0; i < items.Count; i++)
            {
                if (items[i] is { } item)
                {
                    Win32.AppendMenuW(menu, item.Enabled ? Win32.MF_STRING : Win32.MF_STRING | Win32.MF_GRAYED, (nuint)(i + 1), item.Text);
                }
                else
                {
                    Win32.AppendMenuW(menu, Win32.MF_SEPARATOR, 0, null);
                }
            }
            // The owner must be foreground for the menu to close when focus moves elsewhere.
            Win32.SetForegroundWindow(_window);
            var chosen = Win32.TrackPopupMenuEx(menu, Win32.TPM_RETURNCMD | Win32.TPM_RIGHTBUTTON, x, y, _window, 0);
            Win32.PostMessageW(_window, 0, 0, 0);
            if (chosen > 0 && items[chosen - 1] is { Enabled: true } picked)
            {
                picked.Invoke();
            }
        }
        finally
        {
            Win32.DestroyMenu(menu);
        }
    }

    [UnmanagedCallersOnly]
    private static nint WndProc(nint hwnd, uint message, nint wParam, nint lParam)
    {
        var tray = s_current;
        try
        {
            if (tray is not null && hwnd == tray._window)
            {
                if (message == CallbackMessage)
                {
                    var trayEvent = (uint)(lParam & 0xFFFF);
                    switch (trayEvent)
                    {
                        case Win32.NIN_SELECT:
                        case Win32.NIN_KEYSELECT:
                            tray.Activated?.Invoke();
                            return 0;
                        case Win32.WM_CONTEXTMENU:
                            tray.ShowMenu((short)(wParam & 0xFFFF), (short)((wParam >> 16) & 0xFFFF));
                            return 0;
                    }
                }
                else if (message == s_taskbarCreated)
                {
                    tray.Add();
                    return 0;
                }
            }
        }
        catch (Exception)
        {
            // Exceptions must not cross a native frame.
        }
        return Win32.DefWindowProcW(hwnd, message, wParam, lParam);
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }
        _disposed = true;
        var data = Data(0);
        Win32.Shell_NotifyIconW(Win32.NIM_DELETE, &data);
        if (_icon != 0)
        {
            Win32.DestroyIcon(_icon);
        }
        Win32.DestroyWindow(_window);
        s_current = null;
    }
}

internal static unsafe partial class Win32
{
    public const uint WM_APP = 0x8000;
    public const uint WM_CONTEXTMENU = 0x007B;
    public const uint NIN_SELECT = 0x0400;
    public const uint NIN_KEYSELECT = 0x0401;
    public const uint WS_POPUP = 0x80000000;
    public const int SM_CXSMICON = 49;
    public const uint IMAGE_ICON = 1;
    public const uint LR_LOADFROMFILE = 0x10;
    public const uint NIM_ADD = 0, NIM_MODIFY = 1, NIM_DELETE = 2, NIM_SETVERSION = 4;
    public const uint NIF_MESSAGE = 0x1, NIF_ICON = 0x2, NIF_TIP = 0x4, NIF_SHOWTIP = 0x80;
    public const uint NOTIFYICON_VERSION_4 = 4;
    public const uint MF_STRING = 0, MF_GRAYED = 0x1, MF_SEPARATOR = 0x800;
    public const uint TPM_RIGHTBUTTON = 0x2, TPM_RETURNCMD = 0x100;

    [StructLayout(LayoutKind.Sequential)]
    public struct WNDCLASSEXW
    {
        public uint cbSize;
        public uint style;
        public delegate* unmanaged<nint, uint, nint, nint, nint> lpfnWndProc;
        public int cbClsExtra;
        public int cbWndExtra;
        public nint hInstance;
        public nint hIcon;
        public nint hCursor;
        public nint hbrBackground;
        public char* lpszMenuName;
        public char* lpszClassName;
        public nint hIconSm;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct NOTIFYICONDATAW
    {
        public uint cbSize;
        public nint hWnd;
        public uint uID;
        public uint uFlags;
        public uint uCallbackMessage;
        public nint hIcon;
        public fixed char szTip[128];
        public uint dwState;
        public uint dwStateMask;
        public fixed char szInfo[256];
        public uint uVersion;
        public fixed char szInfoTitle[64];
        public uint dwInfoFlags;
        public Guid guidItem;
        public nint hBalloonIcon;
    }

    [LibraryImport("user32.dll", SetLastError = true)]
    public static partial ushort RegisterClassExW(WNDCLASSEXW* wc);

    [LibraryImport("user32.dll", SetLastError = true)]
    public static partial nint CreateWindowExW(uint exStyle, char* className, char* windowName, uint style, int x, int y, int width, int height, nint parent, nint menu, nint instance, nint param);

    [LibraryImport("user32.dll")]
    public static partial nint DefWindowProcW(nint hwnd, uint message, nint wParam, nint lParam);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool DestroyWindow(nint hwnd);

    [LibraryImport("user32.dll", StringMarshalling = StringMarshalling.Utf16)]
    public static partial uint RegisterWindowMessageW(string name);

    [LibraryImport("kernel32.dll", StringMarshalling = StringMarshalling.Utf16)]
    public static partial nint GetModuleHandleW(string? name);

    [LibraryImport("shell32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool Shell_NotifyIconW(uint message, NOTIFYICONDATAW* data);

    [LibraryImport("user32.dll", StringMarshalling = StringMarshalling.Utf16)]
    public static partial nint LoadImageW(nint instance, string name, uint type, int cx, int cy, uint load);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool DestroyIcon(nint icon);

    [LibraryImport("user32.dll")]
    public static partial int GetSystemMetricsForDpi(int index, uint dpi);

    [LibraryImport("user32.dll")]
    public static partial uint GetDpiForSystem();

    [LibraryImport("user32.dll")]
    public static partial nint CreatePopupMenu();

    [LibraryImport("user32.dll", StringMarshalling = StringMarshalling.Utf16)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool AppendMenuW(nint menu, uint flags, nuint id, string? text);

    [LibraryImport("user32.dll")]
    public static partial int TrackPopupMenuEx(nint menu, uint flags, int x, int y, nint hwnd, nint parameters);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool DestroyMenu(nint menu);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool SetForegroundWindow(nint hwnd);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool PostMessageW(nint hwnd, uint message, nint wParam, nint lParam);
}
