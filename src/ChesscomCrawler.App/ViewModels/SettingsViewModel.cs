using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Jupiter.ChesscomCrawler.Interop;
using Jupiter.ChesscomCrawler.Messages;
using Microsoft.UI.Xaml.Controls;

namespace Jupiter.ChesscomCrawler.App.ViewModels;

public sealed partial class TitleOption(string title) : ObservableObject
{
    public string Title { get; } = title;

    [ObservableProperty] public partial bool IsSelected { get; set; }
}

/// <summary>The crawl settings form. Values are validated by the core when saved.</summary>
public sealed partial class SettingsViewModel(CoreHost host) : ObservableObject
{
    private static readonly string[] Titles = ["GM", "WGM", "IM", "WIM", "FM", "WFM", "CM", "WCM", "NM", "WNM"];

    private static readonly Dictionary<string, string> FieldLabels = new()
    {
        ["floors"] = "Rating floors",
        ["time_classes"] = "Time controls",
        ["seed_titles"] = "Title lists",
        ["extra_accounts"] = "Additional accounts",
        ["fide_folders"] = "FIDE database folders",
        ["request_interval_seconds"] = "Seconds between requests",
        ["max_fide_list_age_months"] = "Oldest usable FIDE list",
        ["contact"] = "Contact for Chess.com",
        ["output_folder"] = "Output folder",
        ["months.start"] = "Start month",
        ["months.stop"] = "Stop month",
        ["months.specific"] = "Months",
        ["prefilter_margin"] = "Opponent prefilter",
    };

    private Settings _saved = new();

    public ObservableCollection<TitleOption> TitleOptions { get; } = [.. Titles.Select(t => new TitleOption(t))];

    [ObservableProperty] public partial bool IsLoaded { get; set; }

    [ObservableProperty, NotifyPropertyChangedFor(nameof(IsFide), nameof(IsChesscom), nameof(Identifies))]
    public partial int RatingSourceIndex { get; set; }

    [ObservableProperty, NotifyPropertyChangedFor(nameof(Identifies))]
    public partial bool RequireIdentification { get; set; }

    public bool IsFide => RatingSourceIndex == 1;
    public bool IsChesscom => RatingSourceIndex == 0;

    /// <summary>Whether players are identified against the FIDE lists under the edited settings.</summary>
    public bool Identifies => IsFide || RequireIdentification;
    [ObservableProperty] public partial string Floors { get; set; } = "";
    [ObservableProperty] public partial bool Bullet { get; set; }
    [ObservableProperty] public partial bool Blitz { get; set; }
    [ObservableProperty] public partial bool Rapid { get; set; }
    [ObservableProperty] public partial bool Daily { get; set; }
    [ObservableProperty] public partial bool StandardOnly { get; set; }
    [ObservableProperty] public partial bool RatedOnly { get; set; }
    [ObservableProperty] public partial bool CheckmateOnly { get; set; }
    [ObservableProperty] public partial bool ExcludeFairPlay { get; set; }
    [ObservableProperty] public partial bool IncludeUntitled { get; set; }

    [ObservableProperty, NotifyPropertyChangedFor(nameof(IsContinuous), nameof(IsSpecific))]
    public partial int MonthModeIndex { get; set; }

    public bool IsContinuous => MonthModeIndex == 0;
    public bool IsSpecific => MonthModeIndex == 1;

    [ObservableProperty, NotifyPropertyChangedFor(nameof(StartMonthEnabled))]
    public partial bool StartFromPreviousMonth { get; set; }

    public bool StartMonthEnabled => !StartFromPreviousMonth;

    [ObservableProperty] public partial string StartMonth { get; set; } = "";
    [ObservableProperty] public partial int DirectionIndex { get; set; }
    [ObservableProperty] public partial string StopMonth { get; set; } = "";
    [ObservableProperty] public partial string SpecificMonths { get; set; } = "";
    [ObservableProperty] public partial bool FollowNewMonths { get; set; }
    [ObservableProperty] public partial string ExtraAccounts { get; set; } = "";
    [ObservableProperty] public partial bool DiscoverOpponents { get; set; }
    [ObservableProperty] public partial string FideFolders { get; set; } = "";
    [ObservableProperty] public partial double MaxFideListAge { get; set; }
    [ObservableProperty] public partial double RequestInterval { get; set; }
    [ObservableProperty] public partial string Contact { get; set; } = "";
    [ObservableProperty] public partial double PrefilterMargin { get; set; }
    [ObservableProperty] public partial string OutputFolder { get; set; } = "";
    [ObservableProperty] public partial bool RemoveClockComments { get; set; }
    [ObservableProperty] public partial bool StartOnLaunch { get; set; }

    [ObservableProperty] public partial bool MessageOpen { get; set; }
    [ObservableProperty] public partial string MessageTitle { get; set; } = "";
    [ObservableProperty] public partial string MessageText { get; set; } = "";
    [ObservableProperty] public partial InfoBarSeverity MessageSeverity { get; set; }

    /// <summary>The saved output folder, for the status page.</summary>
    public string SavedOutputFolder => _saved.OutputFolder;

    /// <summary>Whether the saved settings identify players, for the status page.</summary>
    public bool SavedIdentifies => _saved.RatingSource == RatingSource.Fide || _saved.RequireIdentification;

    public void Load(Settings s)
    {
        _saved = s;
        RatingSourceIndex = s.RatingSource == RatingSource.Fide ? 1 : 0;
        RequireIdentification = s.RequireIdentification;
        Floors = string.Join(", ", s.Floors);
        Bullet = s.Bullet;
        Blitz = s.Blitz;
        Rapid = s.Rapid;
        Daily = s.Daily;
        StandardOnly = s.StandardOnly;
        RatedOnly = s.RatedOnly;
        CheckmateOnly = s.CheckmateOnly;
        ExcludeFairPlay = s.ExcludeFairPlayClosures;
        IncludeUntitled = s.IncludeUntitledAccounts;
        MonthModeIndex = s.MonthMode == MonthMode.Specific ? 1 : 0;
        StartFromPreviousMonth = s.StartMonth.Length == 0;
        StartMonth = s.StartMonth;
        DirectionIndex = s.Direction == Direction.Forward ? 1 : 0;
        StopMonth = s.StopMonth;
        SpecificMonths = s.SpecificMonths;
        FollowNewMonths = s.FollowNewMonths;
        foreach (var option in TitleOptions)
        {
            option.IsSelected = s.SeedTitles.Contains(option.Title);
        }
        ExtraAccounts = string.Join(", ", s.ExtraAccounts);
        DiscoverOpponents = s.DiscoverOpponents;
        FideFolders = string.Join(Environment.NewLine, s.FideFolders);
        MaxFideListAge = s.MaxFideListAgeMonths;
        RequestInterval = s.RequestIntervalSeconds;
        Contact = s.Contact;
        PrefilterMargin = s.PrefilterMargin;
        OutputFolder = s.OutputFolder;
        RemoveClockComments = s.RemoveClockComments;
        StartOnLaunch = s.StartOnLaunch;
        IsLoaded = true;
        OnPropertyChanged(nameof(SavedOutputFolder));
        OnPropertyChanged(nameof(SavedIdentifies));
    }

    private Settings? Build()
    {
        var floors = new List<uint>();
        foreach (var part in Floors.Split([',', ' ', ';'], StringSplitOptions.RemoveEmptyEntries))
        {
            if (!uint.TryParse(part, out var f))
            {
                ShowMessage("Rating floors", $"“{part}” is not a number. Enter floors such as 2400, 2700.", InfoBarSeverity.Error);
                return null;
            }
            floors.Add(f);
        }
        static uint Whole(double v) => double.IsNaN(v) || v < 0 ? 0 : (uint)Math.Round(v);
        var s = new Settings
        {
            RatingSource = RatingSourceIndex == 1 ? RatingSource.Fide : RatingSource.Chesscom,
            RequireIdentification = RequireIdentification,
            Bullet = Bullet,
            Blitz = Blitz,
            Rapid = Rapid,
            Daily = Daily,
            StandardOnly = StandardOnly,
            RatedOnly = RatedOnly,
            CheckmateOnly = CheckmateOnly,
            ExcludeFairPlayClosures = ExcludeFairPlay,
            IncludeUntitledAccounts = IncludeUntitled,
            MonthMode = MonthModeIndex == 1 ? MonthMode.Specific : MonthMode.Continuous,
            StartMonth = StartFromPreviousMonth ? "" : StartMonth.Trim(),
            Direction = DirectionIndex == 1 ? Direction.Forward : Direction.Backward,
            StopMonth = StopMonth.Trim(),
            SpecificMonths = SpecificMonths.Trim(),
            FollowNewMonths = FollowNewMonths,
            DiscoverOpponents = DiscoverOpponents,
            MaxFideListAgeMonths = Whole(MaxFideListAge),
            RequestIntervalSeconds = double.IsNaN(RequestInterval) ? 0 : RequestInterval,
            Contact = Contact.Trim(),
            PrefilterMargin = Whole(PrefilterMargin),
            OutputFolder = OutputFolder.Trim(),
            RemoveClockComments = RemoveClockComments,
            StartOnLaunch = StartOnLaunch,
        };
        s.Floors.Add(floors);
        s.SeedTitles.Add(TitleOptions.Where(t => t.IsSelected).Select(t => t.Title));
        s.ExtraAccounts.Add(ExtraAccounts.Split([',', ' ', ';', '\r', '\n'], StringSplitOptions.RemoveEmptyEntries));
        s.FideFolders.Add(FideFolders.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries));
        return s;
    }

    [RelayCommand]
    private void Save()
    {
        if (host.Core is not { } core || Build() is not { } settings)
        {
            return;
        }
        try
        {
            var running = core.GetStatus().State is RunState.Running or RunState.Waiting;
            Load(core.UpdateSettings(settings));
            ShowMessage("Settings saved", running ? "The crawl restarted with the new settings." : "They apply when the crawl next starts.", InfoBarSeverity.Success);
        }
        catch (CrawlerException e)
        {
            var label = FieldLabels.GetValueOrDefault(e.Field, "Settings");
            ShowMessage(label, e.Message, InfoBarSeverity.Error);
        }
    }

    [RelayCommand]
    private void Revert()
    {
        Load(_saved);
        MessageOpen = false;
    }

    [RelayCommand]
    private async Task RewriteCollections()
    {
        if (host.Core is not { } core)
        {
            return;
        }
        ShowMessage("Rewriting collections", "Writing PGN files from collected games…", InfoBarSeverity.Informational);
        try
        {
            var message = await Task.Run(core.ExportCollections);
            ShowMessage("Collections rewritten", message, InfoBarSeverity.Success);
        }
        catch (CrawlerException e)
        {
            ShowMessage("Rewriting collections failed", e.Message, InfoBarSeverity.Error);
        }
    }

    private void ShowMessage(string title, string text, InfoBarSeverity severity)
    {
        MessageTitle = title;
        MessageText = text;
        MessageSeverity = severity;
        MessageOpen = true;
    }
}
