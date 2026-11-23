using System;
using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace AwDrop.Windows;

internal sealed record BridgeStatus(int Version, bool TransfersAvailable, string[] Adapters);

internal static class Bridge
{
    private static readonly JsonSerializerOptions Options = new(JsonSerializerDefaults.Web);

    public static async Task<BridgeStatus> ReadStatus()
    {
        string executable = Path.Combine(AppContext.BaseDirectory, "awdrop-desktop-bridge.exe");
        var info = new ProcessStartInfo(executable)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };
        info.ArgumentList.Add("status");
        using var process = Process.Start(info) ?? throw new IOException("Could not start the AwDrop bridge.");
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(5));
        try
        {
            Task<string> stdout = process.StandardOutput.ReadToEndAsync(deadline.Token);
            Task<string> stderr = process.StandardError.ReadToEndAsync(deadline.Token);
            await Task.WhenAll(stdout, stderr, process.WaitForExitAsync(deadline.Token));
            if (process.ExitCode != 0) throw new IOException("The AwDrop bridge failed.");
            BridgeStatus? status = JsonSerializer.Deserialize<BridgeStatus>(await stdout, Options);
            if (status is not { Version: 1, Adapters: not null })
                throw new InvalidDataException("Unsupported AwDrop bridge response.");
            return status;
        }
        catch (OperationCanceledException)
        {
            try { process.Kill(); }
            catch (InvalidOperationException) { }
            throw;
        }
    }
}
