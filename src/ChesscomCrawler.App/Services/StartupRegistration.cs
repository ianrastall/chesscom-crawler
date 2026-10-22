using Microsoft.Win32;

namespace Jupiter.ChesscomCrawler.App.Services;

/// <summary>The per-user "start when I sign in" entry, which launches the app in the notification area.</summary>
internal static class StartupRegistration
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ValueName = "Jupiter.ChesscomCrawler";

    private static string Command => $"\"{Environment.ProcessPath}\" --background";

    public static bool IsEnabled
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(RunKey);
            return key?.GetValue(ValueName) is string value && value.Equals(Command, StringComparison.OrdinalIgnoreCase);
        }
    }

    public static void Set(bool enabled)
    {
        using var key = Registry.CurrentUser.CreateSubKey(RunKey, writable: true);
        if (enabled)
        {
            key.SetValue(ValueName, Command, RegistryValueKind.String);
        }
        else
        {
            key.DeleteValue(ValueName, throwOnMissingValue: false);
        }
    }
}
