using System.Security.Cryptography;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace SpikeAiPipeline.Mail.Generator;

/// <summary>
/// manifest.json: what the last run built. Each source and output has its SHA-256; each output
/// names its sources and whether two builds of it in that run were byte-identical.
/// </summary>
public sealed record Manifest(
    [property: JsonPropertyName("note")] string Note,
    [property: JsonPropertyName("tools")] IReadOnlyDictionary<string, string> Tools,
    [property: JsonPropertyName("sources")] IReadOnlyList<SourceEntry> Sources,
    [property: JsonPropertyName("outputs")] IReadOnlyList<OutputEntry> Outputs)
{
    static readonly JsonSerializerOptions Options = new()
    {
        WriteIndented = true,
        // Non-ASCII stays readable in the file; it holds no HTML.
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    public static Manifest? ReadIfExists(string path) =>
        File.Exists(path) ? JsonSerializer.Deserialize<Manifest>(File.ReadAllBytes(path)) : null;

    public void Write(string path) =>
        File.WriteAllText(path, JsonSerializer.Serialize(this, Options) + "\n");

    /// <summary>The lowercase hex SHA-256 of a file's bytes.</summary>
    public static string Sha256(string path) =>
        Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(path)));
}

/// <summary>One source, an .eml or an item source, by its path under the generated directory.</summary>
public sealed record SourceEntry(
    [property: JsonPropertyName("file")] string File,
    [property: JsonPropertyName("sha256")] string Sha256);

/// <summary>
/// One built file. ByteStable is false when a second build in the same run differed: then its
/// SHA-256 changes on every run, and only its size and its content, as the tests read it, can show
/// drift.
/// </summary>
public sealed record OutputEntry(
    [property: JsonPropertyName("file")] string File,
    [property: JsonPropertyName("sha256")] string Sha256,
    [property: JsonPropertyName("size")] long Size,
    [property: JsonPropertyName("byte_stable")] bool ByteStable,
    [property: JsonPropertyName("sources")] IReadOnlyList<string> Sources);
