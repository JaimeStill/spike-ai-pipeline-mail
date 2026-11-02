using System.Diagnostics;
using System.Text.Json;
using MimeKit;
using MimeKit.Utils;
using MsgKit;
using PstBuilder;
using PstBuilder.Eml;
using PstBuilder.Messaging;

namespace SpikeAiPipeline.Mail.Generator;

/// <summary>
/// The committed set: every store of layout.json as a .pst under pst/, every listed source as a
/// .msg under msg/, and manifest.json. Each output is built twice, to learn whether it is
/// byte-stable, and the run reports how each source and output differs from the manifest it
/// replaces.
/// </summary>
public static class Fixtures
{
    const string Note =
        "Written by the generator (MISE_ENV=fixtures mise run fixtures); edit layout.json or the "
        + ".eml sources, never this file. Each output was built twice in the run, and byte_stable "
        + "is whether the two builds were identical. Neither format is: PST Builder 1.3.0 writes a "
        + "random store UID and a random search key per message (Guid.NewGuid), and MsgKit 3.0.6 "
        + "random record keys and entry IDs and the time of conversion as the creation and "
        + "modification times. So each output's sha256 records the committed file only; a rerun "
        + "shows drift in an output's size, and the tests show drift in its content.";

    public static int Run(string dir)
    {
        var layout = Layout.Read(Path.Combine(dir, "layout.json"));
        var manifestPath = Path.Combine(dir, "manifest.json");
        var previous = Manifest.ReadIfExists(manifestPath);
        var emlDir = Path.Combine(dir, "eml");

        var sources = Directory.GetFiles(emlDir, "*.eml")
            .Select(path => Path.GetFileName(path))
            .Order(StringComparer.Ordinal)
            .Select(name => new SourceEntry($"eml/{name}", Manifest.Sha256(Path.Combine(emlDir, name))))
            .ToList();
        var known = sources.Select(s => s.File).ToHashSet();
        foreach (var name in layout.Stores.SelectMany(s => s.Folders).SelectMany(f => f.Messages).Concat(layout.Msg))
        {
            if (!known.Contains($"eml/{name}"))
            {
                Console.Error.WriteLine($"generator: layout.json names eml/{name}, which doesn't exist");
                return 1;
            }
        }

        var outputs = new List<OutputEntry>();
        foreach (var store in layout.Stores)
        {
            outputs.Add(Build(dir, store.File, path => BuildStore(store, emlDir, path),
                store.Folders.SelectMany(f => f.Messages).Select(name => $"eml/{name}").ToList()));
        }
        foreach (var name in layout.Msg)
        {
            var file = $"msg/{Path.GetFileNameWithoutExtension(name)}.msg";
            outputs.Add(Build(dir, file, path => Converter.ConvertEmlToMsg(Path.Combine(emlDir, name), path),
                [$"eml/{name}"]));
        }

        var manifest = new Manifest(Note, Tools(), sources, outputs);
        manifest.Write(manifestPath);
        Report(previous, manifest);
        return 0;
    }

    /// <summary>
    /// Builds one output at its place under the generated directory, then once more beside it to
    /// compare, and records it.
    /// </summary>
    static OutputEntry Build(string dir, string file, Action<string> build, IReadOnlyList<string> sources)
    {
        var path = Path.Combine(dir, file);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        var again = path + ".again";
        // MsgKit refuses to overwrite a file.
        File.Delete(path);
        build(path);
        try
        {
            build(again);
            var sha = Manifest.Sha256(path);
            return new OutputEntry(file, sha, new FileInfo(path).Length, sha == Manifest.Sha256(again), sources);
        }
        finally
        {
            File.Delete(again);
        }
    }

    /// <summary>
    /// One store through PST Builder's in-memory tree, the folders in layout order, each message
    /// mapped from its source by PstBuilder.Eml. A source's first Received header gives the
    /// delivery time, so it differs from the submit time, which is the Date header.
    /// </summary>
    static void BuildStore(StoreLayout store, string emlDir, string path)
    {
        Pst.Write(path, top =>
        {
            foreach (var layout in store.Folders)
            {
                var folder = FolderAt(top, layout.Path);
                foreach (var name in layout.Messages)
                {
                    var eml = File.ReadAllBytes(Path.Combine(emlDir, name));
                    folder.AddEml(eml, receivedUtc: Received(eml));
                }
            }
        }, store.DisplayName);
    }

    static FolderItem FolderAt(FolderItem top, IReadOnlyList<string> path)
    {
        var folder = top;
        foreach (var name in path)
        {
            folder = folder.Subfolders.FirstOrDefault(f => f.Name == name) ?? folder.AddFolder(name);
        }
        return folder;
    }

    /// <summary>The date of a source's first (most recent) Received header, if it has one.</summary>
    static DateTime? Received(byte[] eml)
    {
        using var stream = new MemoryStream(eml);
        var received = MimeMessage.Load(stream).Headers[HeaderId.Received];
        if (received is null)
        {
            return null;
        }
        var at = received.LastIndexOf(';');
        if (at < 0 || !DateUtils.TryParse(received[(at + 1)..].Trim(), out var date))
        {
            throw new InvalidDataException($"a Received header without a date: {received}");
        }
        return date.UtcDateTime;
    }

    /// <summary>The packages whose versions the manifest records: the generator's, and what they read with.</summary>
    static readonly string[] Packages = ["PstBuilder", "PstBuilder.Eml", "MsgKit", "MimeKit", "OpenMcdf"];

    internal static Dictionary<string, string> Tools()
    {
        var tools = new Dictionary<string, string>
        {
            ["dotnet-sdk"] = Sdk(),
            ["dotnet-runtime"] = Environment.Version.ToString(),
        };
        // The versions restored, from the lock file embedded at build: an assembly's own version
        // needn't be its package's (MimeKit 4.18.2's assembly says 4.18.1.0).
        using var stream = typeof(Fixtures).Assembly.GetManifestResourceStream("packages.lock.json")
            ?? throw new InvalidOperationException("packages.lock.json isn't embedded");
        var packages = JsonDocument.Parse(stream).RootElement.GetProperty("dependencies").GetProperty("net10.0");
        foreach (var package in Packages)
        {
            tools[package] = packages.GetProperty(package).GetProperty("resolved").GetString()!;
        }
        return tools;
    }

    /// <summary>The SDK global.json pins, as `dotnet --version` reports it.</summary>
    static string Sdk()
    {
        using var process = Process.Start(new ProcessStartInfo("dotnet", "--version")
        {
            RedirectStandardOutput = true,
        }) ?? throw new InvalidOperationException("dotnet --version didn't start");
        var version = process.StandardOutput.ReadToEnd().Trim();
        process.WaitForExit();
        return version;
    }

    /// <summary>One line for each tool, source and output that differs from the previous manifest.</summary>
    static void Report(Manifest? previous, Manifest current)
    {
        if (previous is null)
        {
            Console.WriteLine("drift: no previous manifest; everything is new");
            return;
        }
        var lines = new List<string>();
        foreach (var (tool, version) in current.Tools)
        {
            var before = previous.Tools.GetValueOrDefault(tool);
            if (before != version)
            {
                lines.Add($"tool {tool}: {before ?? "none"} -> {version}");
            }
        }
        foreach (var source in current.Sources)
        {
            var before = previous.Sources.FirstOrDefault(s => s.File == source.File);
            if (before is null)
            {
                lines.Add($"{source.File}: new");
            }
            else if (before.Sha256 != source.Sha256)
            {
                lines.Add($"{source.File}: changed");
            }
        }
        foreach (var gone in previous.Sources.Where(s => current.Sources.All(c => c.File != s.File)))
        {
            lines.Add($"{gone.File}: removed");
        }
        foreach (var output in current.Outputs)
        {
            var before = previous.Outputs.FirstOrDefault(o => o.File == output.File);
            if (before is null)
            {
                lines.Add($"{output.File}: new");
            }
            else if (output.ByteStable && before.Sha256 != output.Sha256)
            {
                lines.Add($"{output.File}: changed, sha256 {before.Sha256} -> {output.Sha256}");
            }
            else if (!output.ByteStable && before.Size != output.Size)
            {
                lines.Add($"{output.File}: not byte-stable, and its size changed, {before.Size} -> {output.Size}");
            }
            else if (!output.ByteStable)
            {
                lines.Add($"{output.File}: not byte-stable; same size, so only the tests' reading of it can show drift");
            }
        }
        foreach (var gone in previous.Outputs.Where(o => current.Outputs.All(c => c.File != o.File)))
        {
            lines.Add($"{gone.File}: no longer built; remove it");
        }
        Console.WriteLine(lines.Count == 0 ? "drift: none" : "drift:\n  " + string.Join("\n  ", lines));
    }
}
