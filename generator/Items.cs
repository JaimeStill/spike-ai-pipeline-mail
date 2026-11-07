using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;
using PstBuilder.Messaging;

namespace SpikeAiPipeline.Mail.Generator;

/// <summary>
/// An item source, items/*.json: one item written field by field through PST Builder's
/// MessageItem, for the classes an .eml can't carry, such as Skype for Business's conversation
/// history (IPM.Note.Microsoft.Conversation and IPM.Note.Microsoft.Missed). Its message class is
/// written as given, the HTML body is its only body, and every recipient has the address type
/// SMTP, as EmlMapper gives an .eml's. The submit time is the conversation's start and the
/// delivery time its end, when the history was saved; the creation and modification times are
/// the delivery time, as EmlMapper writes them.
/// </summary>
public sealed record ItemSource(
    [property: JsonPropertyName("class")] string Class,
    [property: JsonPropertyName("subject")] string Subject,
    [property: JsonPropertyName("sender")] Party Sender,
    [property: JsonPropertyName("to")] IReadOnlyList<Party> To,
    [property: JsonPropertyName("submit_time")] string SubmitTime,
    [property: JsonPropertyName("delivery_time")] string DeliveryTime,
    [property: JsonPropertyName("html")] string Html)
{
    [JsonPropertyName("cc")]
    public IReadOnlyList<Party> Cc { get; init; } = [];

    /// <summary>MAPI's recipient types, PidTagRecipientType.</summary>
    const int MapiTo = 1, MapiCc = 2;

    public static ItemSource Read(string path) =>
        JsonSerializer.Deserialize<ItemSource>(File.ReadAllBytes(path))
        ?? throw new InvalidDataException($"{path}: no item");

    public MessageItem ToMessage()
    {
        var delivered = Utc(DeliveryTime);
        var message = new MessageItem
        {
            MessageClass = Class,
            Subject = Subject,
            BodyHtml = Html,
            SenderName = Sender.Name,
            SenderEmail = Sender.Address,
            DisplayTo = string.Join("; ", To.Select(p => p.Name)),
            SubmitTimeUtc = Utc(SubmitTime),
            DeliveryTimeUtc = delivered,
            CreationTimeUtc = delivered,
            LastModificationTimeUtc = delivered,
        };
        foreach (var (parties, type) in new[] { (To, MapiTo), (Cc, MapiCc) })
        {
            foreach (var party in parties)
            {
                message.Recipients.Add(new RecipientItem
                {
                    DisplayName = party.Name,
                    EmailAddress = party.Address,
                    AddressType = "SMTP",
                    RecipientType = type,
                });
            }
        }
        return message;
    }

    /// <summary>An ISO 8601 UTC time, "2026-03-10T14:02:11Z".</summary>
    static DateTime Utc(string time) =>
        DateTime.ParseExact(time, "yyyy-MM-ddTHH:mm:ssZ", CultureInfo.InvariantCulture,
            DateTimeStyles.AdjustToUniversal | DateTimeStyles.AssumeUniversal);
}

/// <summary>A sender or a recipient: a display name and an SMTP address.</summary>
public sealed record Party(
    [property: JsonPropertyName("name")] string Name,
    [property: JsonPropertyName("address")] string Address);
