using Jupiter.ChesscomCrawler.Interop;
using Jupiter.ChesscomCrawler.Messages;

var failures = 0;
void Check(bool ok, string what)
{
    Console.WriteLine($"{(ok ? "pass" : "FAIL")}  {what}");
    if (!ok) failures++;
}

var root = Path.Combine(Path.GetTempPath(), $"ccc-interop-{Environment.ProcessId}");
try
{
    for (var round = 0; round < 3; round++)
    {
        using var core = CrawlerCore.Create(Path.Combine(root, "data"), Path.Combine(root, "out"));
        var settings = core.GetSettings();
        Check(settings.Floors.SequenceEqual(round == 0 ? [2400u] : [2400u, 2700u]), $"round {round}: settings load");
        Check(settings.OutputFolder.EndsWith("out", StringComparison.Ordinal), $"round {round}: default output folder");

        settings.Floors.Clear();
        settings.Floors.Add([2700, 2400, 2700]);
        var saved = core.UpdateSettings(settings);
        Check(saved.Floors.SequenceEqual([2400u, 2700u]), $"round {round}: settings normalized and saved");

        settings.RequestIntervalSeconds = 0.2;
        try
        {
            core.UpdateSettings(settings);
            Check(false, $"round {round}: invalid interval rejected");
        }
        catch (CrawlerException e)
        {
            Check(e.Kind == CrawlerErrorKind.InvalidSettings && e.Field == "request_interval_seconds", $"round {round}: invalid interval rejected ({e.Message})");
        }

        settings.RequestIntervalSeconds = 3;
        settings.SpecificMonths = "2026-13";
        settings.MonthMode = MonthMode.Specific;
        try
        {
            core.UpdateSettings(settings);
            Check(false, $"round {round}: invalid month rejected");
        }
        catch (CrawlerException e)
        {
            Check(e.Field == "months.specific", $"round {round}: invalid month rejected ({e.Message})");
        }

        var status = core.GetStatus();
        Check(status.State == RunState.Idle, $"round {round}: idle status");
        var events = core.PollEvents();
        Check(events.Count >= 1 && events.Any(e => e.Message.Contains("Settings saved")), $"round {round}: events polled ({events.Count})");
        Check(core.ExportCollections().StartsWith("Rewrote collections", StringComparison.Ordinal), $"round {round}: export");
    }

    // Concurrent calls from several threads.
    using (var core = CrawlerCore.Create(Path.Combine(root, "data"), Path.Combine(root, "out")))
    {
        var errors = 0;
        Parallel.For(0, 400, i =>
        {
            try
            {
                if (i % 2 == 0) core.GetStatus(); else core.PollEvents();
            }
            catch
            {
                Interlocked.Increment(ref errors);
            }
        });
        Check(errors == 0, "concurrent calls");
    }
}
catch (Exception e)
{
    Console.WriteLine($"FAIL  unexpected {e.GetType().Name}: {e.Message}");
    failures++;
}
finally
{
    try { Directory.Delete(root, recursive: true); } catch (IOException) { }
}

Console.WriteLine(failures == 0 ? "All interop checks passed." : $"{failures} interop checks failed.");
return failures == 0 ? 0 : 1;
