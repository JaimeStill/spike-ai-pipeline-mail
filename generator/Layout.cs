using System.Text.Json;
using System.Text.Json.Serialization;

namespace SpikeAiPipeline.Mail.Generator;

/// <summary>
/// layout.json: what the generator builds from the .eml sources. Each store lists its folders in
/// the order they are created, each with the sources it holds, in order; every source also becomes
/// one .msg.
/// </summary>
public sealed record Layout(
    [property: JsonPropertyName("stores")] IReadOnlyList<StoreLayout> Stores,
    [property: JsonPropertyName("msg")] IReadOnlyList<string> Msg)
{
    public static Layout Read(string path) =>
        JsonSerializer.Deserialize<Layout>(File.ReadAllBytes(path))
        ?? throw new InvalidDataException($"{path}: no layout");
}

/// <summary>One .pst: its file under the generated directory, its display name and its folders.</summary>
public sealed record StoreLayout(
    [property: JsonPropertyName("file")] string File,
    [property: JsonPropertyName("display_name")] string DisplayName,
    [property: JsonPropertyName("folders")] IReadOnlyList<FolderLayout> Folders);

/// <summary>
/// One folder, by its path of display names under the IPM subtree's root, and the sources
/// (file names under eml/) it holds.
/// </summary>
public sealed record FolderLayout(
    [property: JsonPropertyName("path")] IReadOnlyList<string> Path,
    [property: JsonPropertyName("messages")] IReadOnlyList<string> Messages);
