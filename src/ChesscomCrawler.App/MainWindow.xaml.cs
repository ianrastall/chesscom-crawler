using Jupiter.ChesscomCrawler.App.Services;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Graphics;

namespace Jupiter.ChesscomCrawler.App;

public sealed partial class MainWindow : Window
{
    private readonly ShellPreferences _prefs;

    /// <summary>When false, closing the window hides it to the notification area.</summary>
    public bool AllowClose { get; set; }

    public MainWindow(ShellPreferences prefs)
    {
        _prefs = prefs;
        InitializeComponent();
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "app.ico"));
        AppWindow.Title = "Chess.com Crawler";
        RestoreBounds();
        AppWindow.Closing += OnClosing;
    }

    public FrameworkElement RootElement => Root;

    public Views.StatusView Status => StatusPage;

    public Views.SettingsView Settings => SettingsPage;

    private void OnPageChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        var settings = sender.SelectedItem == SettingsItem;
        StatusPage.Visibility = settings ? Visibility.Collapsed : Visibility.Visible;
        SettingsPage.Visibility = settings ? Visibility.Visible : Visibility.Collapsed;
    }

    public void ShowAndActivate()
    {
        if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized } p)
        {
            p.Restore();
        }
        AppWindow.Show();
        Activate();
    }

    private void OnClosing(AppWindow sender, AppWindowClosingEventArgs args)
    {
        SaveBounds();
        if (!AllowClose)
        {
            args.Cancel = true;
            sender.Hide();
        }
    }

    private void RestoreBounds()
    {
        var area = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Primary).WorkArea;
        var width = _prefs.WindowWidth > 0 ? _prefs.WindowWidth : (int)(980 * Dpi());
        var height = _prefs.WindowHeight > 0 ? _prefs.WindowHeight : (int)(780 * Dpi());
        width = Math.Min(width, area.Width);
        height = Math.Min(height, area.Height);
        var saved = new RectInt32(_prefs.WindowX, _prefs.WindowY, width, height);
        var visible = _prefs.WindowX != int.MinValue
            && saved.X >= area.X - 32 && saved.Y >= area.Y - 8
            && saved.X + 64 <= area.X + area.Width && saved.Y + 64 <= area.Y + area.Height;
        if (visible)
        {
            AppWindow.MoveAndResize(saved);
        }
        else
        {
            AppWindow.MoveAndResize(new RectInt32(area.X + (area.Width - width) / 2, area.Y + (area.Height - height) / 2, width, height));
        }
    }

    private double Dpi() => Win32Dpi.GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0;

    private void SaveBounds()
    {
        if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Restored })
        {
            _prefs.WindowX = AppWindow.Position.X;
            _prefs.WindowY = AppWindow.Position.Y;
            _prefs.WindowWidth = AppWindow.Size.Width;
            _prefs.WindowHeight = AppWindow.Size.Height;
            _prefs.Save();
        }
    }
}

internal static partial class Win32Dpi
{
    [System.Runtime.InteropServices.LibraryImport("user32.dll")]
    public static partial uint GetDpiForWindow(nint hwnd);
}
