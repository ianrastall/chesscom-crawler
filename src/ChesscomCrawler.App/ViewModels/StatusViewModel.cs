using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Jupiter.ChesscomCrawler.Messages;
using Microsoft.UI.Xaml.Controls;

namespace Jupiter.ChesscomCrawler.App.ViewModels;

public sealed record ActivityItem(string Time, string Message, string Glyph, bool IsProblem);

public sealed partial class StatusViewModel(CoreHost host) : ObservableObject
{
    private const int ActivityLimit = 500;

    public ObservableCollection<ActivityItem> Activity { get; } = [];

    [ObservableProperty] public partial string StateText { get; set; } = "Starting";
    [ObservableProperty] public partial string PhaseText { get; set; } = "Opening the crawler core";
    [ObservableProperty] public partial string MonthText { get; set; } = "—";
    [ObservableProperty] public partial string AccountsText { get; set; } = "—";
    [ObservableProperty] public partial double AccountsProgress { get; set; }
    [ObservableProperty] public partial bool AccountsProgressVisible { get; set; }
    [ObservableProperty] public partial string MonthGamesText { get; set; } = "—";
    [ObservableProperty] public partial string TotalGamesText { get; set; } = "—";
    [ObservableProperty] public partial string IdentifiedText { get; set; } = "—";
    [ObservableProperty] public partial string RequestsText { get; set; } = "—";
    [ObservableProperty] public partial string FideText { get; set; } = "—";
    [ObservableProperty] public partial string OutputFolder { get; set; } = "";
    [ObservableProperty] public partial bool ActivityEmpty { get; set; } = true;
    [ObservableProperty] public partial bool NoticeOpen { get; set; }
    [ObservableProperty] public partial string NoticeTitle { get; set; } = "";
    [ObservableProperty] public partial string NoticeMessage { get; set; } = "";
    [ObservableProperty] public partial InfoBarSeverity NoticeSeverity { get; set; }

    [ObservableProperty, NotifyCanExecuteChangedFor(nameof(StartCommand)), NotifyCanExecuteChangedFor(nameof(StopCommand))]
    public partial RunState State { get; set; } = RunState.Idle;

    [ObservableProperty, NotifyCanExecuteChangedFor(nameof(StartCommand))]
    public partial bool CoreReady { get; set; }

    public bool IsActive => State is RunState.Running or RunState.Waiting or RunState.Stopping;

    /// <summary>One-line summary for the notification-area tooltip.</summary>
    public string Summary { get; private set; } = "Chess.com Crawler";

    private bool CanStart() => CoreReady && !IsActive;

    private bool CanStop() => State is RunState.Running or RunState.Waiting;

    [RelayCommand(CanExecute = nameof(CanStart))]
    private void Start() => host.Start();

    [RelayCommand(CanExecute = nameof(CanStop))]
    private void Stop() => host.Stop();

    [RelayCommand]
    private void OpenOutputFolder() => host.OpenFolder(OutputFolder);

    public void ShowCoreFailure(string message)
    {
        StateText = "Unavailable";
        PhaseText = "The crawler core could not start.";
        NoticeTitle = "The crawler core could not start";
        NoticeMessage = message;
        NoticeSeverity = InfoBarSeverity.Error;
        NoticeOpen = true;
        Summary = "Chess.com Crawler — unavailable";
    }

    public void Apply(Status s, string outputFolder, bool identifies)
    {
        CoreReady = true;
        State = s.State;
        OnPropertyChanged(nameof(IsActive));
        StateText = s.State switch
        {
            RunState.Running => "Running",
            RunState.Waiting => "Waiting",
            RunState.Stopping => "Stopping",
            RunState.Completed => "Completed",
            RunState.Failed => "Failed",
            _ => "Not running",
        };
        PhaseText = s.Phase;
        MonthText = s.Month.Length == 0
            ? "—"
            : s.MonthsTotal > 0 ? $"{s.Month}  ·  {s.MonthsDone:N0} of {s.MonthsTotal:N0} scheduled months done" : s.Month;
        AccountsProgressVisible = s.MonthPlayersTotal > 0;
        AccountsProgress = s.MonthPlayersTotal > 0 ? 100.0 * s.MonthPlayersDone / s.MonthPlayersTotal : 0;
        AccountsText = s.MonthPlayersTotal > 0 ? $"{s.MonthPlayersDone:N0} of {s.MonthPlayersTotal:N0}" : "—";
        MonthGamesText = s.Month.Length == 0 ? "—" : s.MonthGames.ToString("N0");
        TotalGamesText = s.TotalGames.ToString("N0");
        IdentifiedText = !identifies
            ? "Off: collecting with Chess.com usernames and ratings"
            : s.KnownPlayers > 0 ? $"{s.ResolvedPlayers:N0} of {s.KnownPlayers:N0} accounts matched to FIDE players" : "—";
        RequestsText = s.Requests == 0
            ? "None yet"
            : $"{s.Requests:N0}  ·  last at {DateTimeOffset.FromUnixTimeSeconds(s.LastRequestUnix).ToLocalTime():HH:mm:ss}";
        FideText = s.FideLast.Length == 0
            ? "No standard lists found"
            : $"{s.FideFirst} to {s.FideLast}" + (s.FideIndexList.Length > 0 ? $"  ·  names from {s.FideIndexList}" : "");
        OutputFolder = outputFolder;

        var until = s.WaitingUntilUnix > 0 ? DateTimeOffset.FromUnixTimeSeconds(s.WaitingUntilUnix).ToLocalTime() : (DateTimeOffset?)null;
        if (s.State == RunState.Failed)
        {
            NoticeTitle = "The crawl stopped";
            NoticeMessage = $"{s.LastError} Collected games and progress are kept; Start resumes once the cause is fixed.";
            NoticeSeverity = InfoBarSeverity.Error;
            NoticeOpen = true;
        }
        else if (s.State == RunState.Waiting && until is { } u && s.LastError.Length > 0 && !s.Phase.StartsWith("Waiting for", StringComparison.Ordinal))
        {
            NoticeTitle = "Pausing before the next request";
            NoticeMessage = $"{s.LastError}. Retrying at {u:HH:mm}.";
            NoticeSeverity = InfoBarSeverity.Warning;
            NoticeOpen = true;
        }
        else
        {
            NoticeOpen = false;
        }

        Summary = s.State switch
        {
            RunState.Running or RunState.Waiting when s.Month.Length > 0 => $"Chess.com Crawler — {StateText.ToLowerInvariant()}, {s.Month}, {s.MonthGames:N0} games this month",
            _ => $"Chess.com Crawler — {StateText.ToLowerInvariant()}, {s.TotalGames:N0} games collected",
        };
    }

    public void AddEvents(IReadOnlyList<Event> events)
    {
        foreach (var e in events)
        {
            var time = DateTimeOffset.FromUnixTimeMilliseconds(e.UnixMs).ToLocalTime().ToString("HH:mm:ss");
            var (glyph, problem) = e.Level switch
            {
                EventLevel.Error => ("", true),
                EventLevel.Warning => ("", true),
                _ => ("", false),
            };
            Activity.Insert(0, new ActivityItem(time, e.Message, glyph, problem));
        }
        while (Activity.Count > ActivityLimit)
        {
            Activity.RemoveAt(Activity.Count - 1);
        }
        ActivityEmpty = Activity.Count == 0;
    }
}
