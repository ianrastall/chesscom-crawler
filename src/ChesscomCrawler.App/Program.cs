using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace Jupiter.ChesscomCrawler.App;

public static class Program
{
    private const string InstanceMutexName = @"Local\Jupiter.ChesscomCrawler";
    internal const string ShowEventName = @"Local\Jupiter.ChesscomCrawler.Show";

    /// <summary>Started from the sign-in entry: stay in the notification area.</summary>
    public static bool StartInBackground { get; private set; }

    [STAThread]
    public static int Main(string[] args)
    {
        StartInBackground = args.Contains("--background", StringComparer.OrdinalIgnoreCase);

        // One instance per user session. A second launch asks the first to show its window.
        using var instance = new Mutex(initiallyOwned: true, InstanceMutexName, out var createdNew);
        if (!createdNew)
        {
            if (!StartInBackground && EventWaitHandle.TryOpenExisting(ShowEventName, out var show))
            {
                using (show)
                {
                    show.Set();
                }
            }
            return 0;
        }

        WinRT.ComWrappersSupport.InitializeComWrappers();
        Application.Start(callback =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            _ = new App();
        });
        GC.KeepAlive(instance);
        return 0;
    }
}
