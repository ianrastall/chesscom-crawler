using System.Text.Json;
using System.Text.Json.Serialization;

namespace Jupiter.ChesscomCrawler.App.Services;

/// <summary>Presentation preferences owned by the shell (ui.json beside the core's settings).</summary>
public sealed class ShellPreferences
{
    public const int CurrentVersion = 1;

    public int Version { get; set; } = CurrentVersion;
    public bool UseCustomAccent { get; set; }
    public string CustomAccent { get; set; } = "#C8A15A";
    public int WindowX { get; set; } = int.MinValue;
    public int WindowY { get; set; } = int.MinValue;
    public int WindowWidth { get; set; }
    public int WindowHeight { get; set; }

    [JsonIgnore]
    public string Path { get; private set; } = "";

    public static ShellPreferences Load(string path)
    {
        ShellPreferences? prefs = null;
        try
        {
            if (File.Exists(path))
            {
                prefs = JsonSerializer.Deserialize(File.ReadAllText(path), ShellJson.Default.ShellPreferences);
                if (prefs is { Version: > CurrentVersion })
                {
                    // Written by a newer version: use defaults and leave the file alone.
                    prefs = new ShellPreferences { Path = "" };
                    return prefs;
                }
            }
        }
        catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException)
        {
            prefs = null;
        }
        prefs ??= new ShellPreferences();
        prefs.Path = path;
        return prefs;
    }

    public void Save()
    {
        if (Path.Length == 0)
        {
            return;
        }
        try
        {
            var tmp = Path + ".partial";
            File.WriteAllText(tmp, JsonSerializer.Serialize(this, ShellJson.Default.ShellPreferences));
            File.Move(tmp, Path, overwrite: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            // Presentation preferences are not worth interrupting work for.
        }
    }
}

[JsonSourceGenerationOptions(WriteIndented = true, PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase)]
[JsonSerializable(typeof(ShellPreferences))]
internal sealed partial class ShellJson : JsonSerializerContext;
