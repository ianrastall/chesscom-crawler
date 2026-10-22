using System.Diagnostics;
using Jupiter.ChesscomCrawler.Interop;

namespace Jupiter.ChesscomCrawler.App.ViewModels;

/// <summary>Owns the crawler core for the shell and reports command failures.</summary>
public sealed class CoreHost
{
    public CrawlerCore? Core { get; private set; }

    /// <summary>Receives a short description of a failed command.</summary>
    public Action<string>? ReportProblem { get; set; }

    public void Attach(CrawlerCore core) => Core = core;

    public CrawlerCore? Detach()
    {
        var core = Core;
        Core = null;
        return core;
    }

    public void Start() => Run(c => c.Start(), "Starting the crawl failed");

    public void Stop() => Run(c => c.Stop(), "Stopping the crawl failed");

    private void Run(Action<CrawlerCore> action, string what)
    {
        if (Core is not { } core)
        {
            return;
        }
        try
        {
            action(core);
        }
        catch (CrawlerException e)
        {
            ReportProblem?.Invoke($"{what}: {e.Message}");
        }
    }

    public void OpenFolder(string folder)
    {
        if (string.IsNullOrWhiteSpace(folder))
        {
            return;
        }
        try
        {
            Directory.CreateDirectory(folder);
            Process.Start(new ProcessStartInfo { FileName = folder, UseShellExecute = true });
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or System.ComponentModel.Win32Exception)
        {
            ReportProblem?.Invoke($"Opening {folder} failed: {e.Message}");
        }
    }
}
