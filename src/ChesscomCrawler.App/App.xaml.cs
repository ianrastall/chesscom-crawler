using Jupiter.ChesscomCrawler.App.Services;
using Jupiter.ChesscomCrawler.App.ViewModels;
using Jupiter.ChesscomCrawler.Interop;
using Jupiter.ChesscomCrawler.Messages;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace Jupiter.ChesscomCrawler.App;

public partial class App : Application
{
    private ShellPreferences _prefs = null!;
    private TrayIcon? _tray;
    private DispatcherQueueTimer? _timer;
    private EventWaitHandle? _showSignal;
    private bool _exiting;

    public App()
    {
        InitializeComponent();
        Status = new StatusViewModel(Host);
        Settings = new SettingsViewModel(Host);
        Host.ReportProblem = message => Report(EventLevel.Error, message);
        UnhandledException += (_, e) =>
        {
            e.Handled = true;
            Report(EventLevel.Error, $"Unexpected error: {e.Message}");
        };
    }

    public static new App Current => (App)Application.Current;

    public CoreHost Host { get; } = new();

    public StatusViewModel Status { get; }

    public SettingsViewModel Settings { get; }

    public AccentService Accent { get; private set; } = null!;

    public MainWindow Window { get; private set; } = null!;

    /// <summary>Settings and state; CHESSCOM_CRAWLER_DATA_DIR overrides it for development and testing.</summary>
    public static string DataDirectory { get; } =
        Environment.GetEnvironmentVariable("CHESSCOM_CRAWLER_DATA_DIR") is { Length: > 0 } custom
            ? custom
            : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Jupiter", "ChesscomCrawler");

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        Directory.CreateDirectory(DataDirectory);
        _prefs = ShellPreferences.Load(Path.Combine(DataDirectory, "ui.json"));
        var dispatcher = DispatcherQueue.GetForCurrentThread();

        Accent = new AccentService(_prefs, dispatcher);
        Window = new MainWindow(_prefs);
        Accent.Attach(Window.RootElement);

        _tray = new TrayIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "app.ico"), "Chess.com Crawler", TrayMenu);
        _tray.Activated += ShowWindow;
        if (!Program.StartInBackground)
        {
            Window.ShowAndActivate();
        }
        ListenForShowRequests(dispatcher);

        _timer = dispatcher.CreateTimer();
        _timer.Interval = TimeSpan.FromSeconds(1);
        _timer.Tick += (_, _) => Refresh();
        _timer.Start();

        _ = OpenCoreAsync();
    }

    private async Task OpenCoreAsync()
    {
        var output = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "Chess.com collections");
        try
        {
            // Scanning the FIDE folders can take a moment; keep it off the UI thread.
            var core = await Task.Run(() => CrawlerCore.Create(DataDirectory, output));
            if (_exiting)
            {
                await Task.Run(core.Dispose);
                return;
            }
            Host.Attach(core);
            var settings = core.GetSettings();
            Settings.Load(settings);
            Refresh();
            if (settings.StartOnLaunch)
            {
                Host.Start();
            }
        }
        catch (CrawlerException e)
        {
            Status.ShowCoreFailure(e.Message);
        }
    }

    private void Refresh()
    {
        if (Host.Core is not { } core)
        {
            return;
        }
        try
        {
            Status.Apply(core.GetStatus(), Settings.SavedOutputFolder, Settings.SavedIdentifies);
            Status.AddEvents(core.PollEvents());
            if (_tray is not null)
            {
                _tray.Tooltip = Status.Summary;
            }
        }
        catch (CrawlerException e)
        {
            _timer?.Stop();
            Status.ShowCoreFailure(e.Message);
        }
    }

    private void Report(EventLevel level, string message) =>
        Status.AddEvents([new Event { Level = level, Message = message, UnixMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() }]);

    private IReadOnlyList<TrayIcon.MenuItem?> TrayMenu()
    {
        var running = Status.IsActive;
        return
        [
            new("Open Chess.com Crawler", ShowWindow),
            running
                ? new("Stop crawl", Host.Stop, Status.StopCommand.CanExecute(null))
                : new("Start crawl", Host.Start, Status.StartCommand.CanExecute(null)),
            null,
            new("Exit", ExitApp),
        ];
    }

    private void ShowWindow() => Window.ShowAndActivate();

    /// <summary>A second launch signals this event to bring the window forward.</summary>
    private void ListenForShowRequests(DispatcherQueue dispatcher)
    {
        _showSignal = new EventWaitHandle(false, EventResetMode.AutoReset, Program.ShowEventName);
        var signal = _showSignal;
        var thread = new Thread(() =>
        {
            while (signal.WaitOne() && !_exiting)
            {
                dispatcher.TryEnqueue(ShowWindow);
            }
        })
        { IsBackground = true, Name = "show-requests" };
        thread.Start();
    }

    /// <summary>Stops the crawl, releases the core off the UI thread, then exits.</summary>
    private async void ExitApp()
    {
        if (_exiting)
        {
            return;
        }
        _exiting = true;
        _timer?.Stop();
        _tray?.Dispose();
        Window.AppWindow.Hide();
        _showSignal?.Set();
        if (Host.Detach() is { } core)
        {
            await Task.Run(core.Dispose);
        }
        Window.AllowClose = true;
        Window.Close();
        Exit();
    }
}
