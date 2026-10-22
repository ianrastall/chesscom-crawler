using Google.Protobuf;
using Jupiter.ChesscomCrawler.Messages;

namespace Jupiter.ChesscomCrawler.Interop;

public enum CrawlerErrorKind
{
    Transport,
    InvalidSettings,
    Io,
    State,
    UnknownCommand,
}

public sealed class CrawlerException(CrawlerErrorKind kind, string message, string field = "") : Exception(message)
{
    public CrawlerErrorKind Kind { get; } = kind;

    /// <summary>For invalid settings, the settings field at fault.</summary>
    public string Field { get; } = field;

    internal static CrawlerException From(Error error) => new(
        error.Kind switch
        {
            ErrorKind.InvalidSettings => CrawlerErrorKind.InvalidSettings,
            ErrorKind.Io => CrawlerErrorKind.Io,
            ErrorKind.State => CrawlerErrorKind.State,
            ErrorKind.UnknownCommand => CrawlerErrorKind.UnknownCommand,
            _ => CrawlerErrorKind.State,
        },
        error.Message,
        error.Field);
}

/// <summary>
/// Typed access to the Rust crawler core. Calls are short and may come from any thread;
/// <see cref="ExportCollections"/> and <see cref="Dispose"/> can take longer and belong off the UI thread.
/// </summary>
public sealed class CrawlerCore : IDisposable
{
    private readonly CoreSafeHandle _handle;
    private long _nextRequestId;

    private CrawlerCore(CoreSafeHandle handle) => _handle = handle;

    /// <summary>Loads the native core, checks its ABI version and opens the data folder.</summary>
    public static unsafe CrawlerCore Create(string dataDir, string defaultOutputDir)
    {
        uint version;
        try
        {
            version = NativeMethods.ccc_abi_version();
        }
        catch (Exception e) when (e is DllNotFoundException or BadImageFormatException or EntryPointNotFoundException)
        {
            throw new CrawlerException(CrawlerErrorKind.Transport, $"The crawler core (chesscom_crawler_ffi.dll) could not be loaded: {e.Message}");
        }
        if (version != NativeMethods.AbiVersion)
        {
            throw new CrawlerException(CrawlerErrorKind.Transport, $"The crawler core has ABI version {version}; this app needs {NativeMethods.AbiVersion}.");
        }

        var config = new CreateConfig { DataDir = dataDir, DefaultOutputDir = defaultOutputDir }.ToByteArray();
        NativeStatus status;
        CoreSafeHandle core;
        ResultSafeHandle error;
        fixed (byte* ptr = config)
        {
            status = NativeMethods.ccc_create(ptr, (nuint)config.Length, out core, out error);
        }
        using (error)
        {
            if (status == NativeStatus.Ok && !core.IsInvalid)
            {
                return new CrawlerCore(core);
            }
            core.Dispose();
            if (!error.IsInvalid && Response.Parser.ParseFrom(error.CopyBytes()).Error is { } e)
            {
                throw CrawlerException.From(e);
            }
            throw new CrawlerException(CrawlerErrorKind.Transport, $"The crawler core could not start ({status}).");
        }
    }

    private unsafe Response Call(Request request)
    {
        request.RequestId = (ulong)Interlocked.Increment(ref _nextRequestId);
        var bytes = request.ToByteArray();
        NativeStatus status;
        ResultSafeHandle result;
        fixed (byte* ptr = bytes)
        {
            status = NativeMethods.ccc_call(_handle, ptr, (nuint)bytes.Length, out result);
        }
        using (result)
        {
            if (status != NativeStatus.Ok)
            {
                throw new CrawlerException(CrawlerErrorKind.Transport, $"The crawler core rejected a call ({status}).");
            }
            var response = Response.Parser.ParseFrom(result.CopyBytes());
            if (response.RequestId != request.RequestId)
            {
                throw new CrawlerException(CrawlerErrorKind.Transport, "The crawler core answered a different request.");
            }
            if (response.ResultCase == Response.ResultOneofCase.Error)
            {
                throw CrawlerException.From(response.Error);
            }
            return response;
        }
    }

    public Status GetStatus() => Call(new Request { GetStatus = new GetStatus() }).Status;

    public Settings GetSettings() => Call(new Request { GetSettings = new GetSettings() }).Settings;

    /// <summary>Validates and saves settings; returns the normalized copy. A running crawl restarts.</summary>
    public Settings UpdateSettings(Settings settings) =>
        Call(new Request { UpdateSettings = new UpdateSettings { Settings = settings } }).Settings;

    public void Start() => Call(new Request { StartCrawl = new StartCrawl() });

    public void Stop() => Call(new Request { StopCrawl = new StopCrawl() });

    public string ExportCollections() => Call(new Request { ExportCollections = new ExportCollections() }).Done.Message;

    public IReadOnlyList<Event> PollEvents(uint max = 200)
    {
        var status = NativeMethods.ccc_poll_events(_handle, max, out var result);
        using (result)
        {
            if (status != NativeStatus.Ok)
            {
                throw new CrawlerException(CrawlerErrorKind.Transport, $"Reading activity failed ({status}).");
            }
            return EventBatch.Parser.ParseFrom(result.CopyBytes()).Events;
        }
    }

    /// <summary>Stops the crawl and releases the core, waiting up to 30 seconds.</summary>
    public void Dispose() => _handle.Dispose();
}
