using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.AssemblyDirectory | DllImportSearchPath.System32)]
[assembly: DisableRuntimeMarshalling]

namespace Jupiter.ChesscomCrawler.Interop;

/// <summary>Declarations for the Rust C ABI in core/ffi (ABI version 1).</summary>
internal static unsafe partial class NativeMethods
{
    private const string Library = "chesscom_crawler_ffi";

    public const uint AbiVersion = 1;

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial uint ccc_abi_version();

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial NativeStatus ccc_create(byte* config, nuint configLength, out CoreSafeHandle core, out ResultSafeHandle error);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial NativeStatus ccc_call(CoreSafeHandle core, byte* request, nuint requestLength, out ResultSafeHandle result);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial NativeStatus ccc_poll_events(CoreSafeHandle core, uint max, out ResultSafeHandle result);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial NativeStatus ccc_result_bytes(ResultSafeHandle result, out byte* bytes, out nuint length);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial void ccc_result_free(nint result);

    [LibraryImport(Library)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial void ccc_destroy(nint core);
}

/// <summary>Transport-level outcomes of an ABI call. Domain failures arrive as Response.error.</summary>
public enum NativeStatus
{
    Ok = 0,
    InvalidArgument = 1,
    MalformedRequest = 2,
    Panic = 3,
    Unusable = 4,
    CreateFailed = 5,
}

internal sealed class CoreSafeHandle() : SafeHandle(nint.Zero, ownsHandle: true)
{
    public override bool IsInvalid => handle == nint.Zero;

    // Stops the crawl and waits up to 30 seconds; normal disposal runs off the UI thread.
    protected override bool ReleaseHandle()
    {
        NativeMethods.ccc_destroy(handle);
        return true;
    }
}

internal sealed class ResultSafeHandle() : SafeHandle(nint.Zero, ownsHandle: true)
{
    public override bool IsInvalid => handle == nint.Zero;

    protected override bool ReleaseHandle()
    {
        NativeMethods.ccc_result_free(handle);
        return true;
    }

    /// <summary>Copies the result bytes into managed memory before the handle is freed.</summary>
    public unsafe byte[] CopyBytes()
    {
        var status = NativeMethods.ccc_result_bytes(this, out var ptr, out var length);
        if (status != NativeStatus.Ok)
        {
            throw new CrawlerException(CrawlerErrorKind.Transport, $"Reading a result failed ({status}).");
        }
        if (length > int.MaxValue)
        {
            throw new CrawlerException(CrawlerErrorKind.Transport, "A result was too large.");
        }
        return length == 0 ? [] : new ReadOnlySpan<byte>(ptr, checked((int)length)).ToArray();
    }
}
