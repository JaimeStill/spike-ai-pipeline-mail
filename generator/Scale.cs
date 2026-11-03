using System.Diagnostics;
using System.Security.Cryptography;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;
using PstBuilder.Eml;
using PstBuilder.Messaging;

namespace SpikeAiPipeline.Mail.Generator;

/// <summary>
/// The scale mode: one large .pst built from a seed, for the CLI's time and peak memory, never
/// committed. Every message is a MIME message the generator writes itself from a seeded
/// System.Random, fixed boundaries and IDs included, and is added through PstBuilder.Eml, as the
/// committed fixtures are. So a seed and the parameters below give the same messages, in the same
/// folders and order, on every run: content_sha256 in scale.json, a hash of every message's
/// folder and MIME bytes in order, shows it. The .pst's bytes still differ on every run, as PST
/// Builder 1.3.0 writes a random store UID and search keys (manifest.json's note).
/// </summary>
public static class Scale
{
    /// <summary>The parameters a scale store is built from, recorded in scale.json.</summary>
    public sealed record Parameters(
        [property: JsonPropertyName("seed")] int Seed,
        [property: JsonPropertyName("messages")] int Messages,
        [property: JsonPropertyName("folders")] IReadOnlyList<WeightedFolder> Folders,
        [property: JsonPropertyName("people")] int People,
        [property: JsonPropertyName("bodies")] string Bodies,
        [property: JsonPropertyName("reply_share")] double ReplyShare,
        [property: JsonPropertyName("inline_image_share")] double InlineImageShare,
        [property: JsonPropertyName("attachment_share")] double AttachmentShare,
        [property: JsonPropertyName("attachments_per_message")] string AttachmentsPerMessage,
        [property: JsonPropertyName("attachment_bytes")] string AttachmentBytes,
        [property: JsonPropertyName("large_attachment_share")] double LargeAttachmentShare,
        [property: JsonPropertyName("large_attachment_bytes")] string LargeAttachmentBytes);

    /// <summary>A folder, by its path under the IPM subtree's root, and its share of the messages.</summary>
    public sealed record WeightedFolder(
        [property: JsonPropertyName("path")] string Path,
        [property: JsonPropertyName("weight")] int Weight);

    /// <summary>scale.json: what one scale run built, beside the store.</summary>
    public sealed record Record(
        [property: JsonPropertyName("note")] string Note,
        [property: JsonPropertyName("parameters")] Parameters Parameters,
        [property: JsonPropertyName("tools")] IReadOnlyDictionary<string, string> Tools,
        [property: JsonPropertyName("content_sha256")] string ContentSha256,
        [property: JsonPropertyName("counts")] Counts Counts,
        [property: JsonPropertyName("parts")] IReadOnlyList<Part> Parts,
        [property: JsonPropertyName("seconds")] double Seconds);

    /// <summary>What the messages hold, counted as they were made.</summary>
    public sealed record Counts(
        [property: JsonPropertyName("messages")] int Messages,
        [property: JsonPropertyName("html")] int Html,
        [property: JsonPropertyName("html_only")] int HtmlOnly,
        [property: JsonPropertyName("replies")] int Replies,
        [property: JsonPropertyName("attachments")] int Attachments,
        [property: JsonPropertyName("inline_images")] int InlineImages,
        [property: JsonPropertyName("attachment_bytes")] long AttachmentBytes,
        [property: JsonPropertyName("largest_attachment")] long LargestAttachment,
        [property: JsonPropertyName("mime_bytes")] long MimeBytes);

    /// <summary>One .pst the session wrote; one, unless the store passed PST Builder's single-file limit.</summary>
    public sealed record Part(
        [property: JsonPropertyName("file")] string File,
        [property: JsonPropertyName("size")] long Size);

    const string Note =
        "Written by the generator's scale mode (MISE_ENV=fixtures mise run scale); never committed. "
        + "The messages are a function of the parameters alone: content_sha256 hashes each message's "
        + "folder path and MIME bytes, in order, and is the same on every run with these parameters "
        + "and tools. The .pst's bytes are not: PST Builder 1.3.0 writes a random store UID and "
        + "search keys, so the store has no SHA-256 worth recording.";

    /// <summary>
    /// The folders, by weight: an Inbox holding most messages, so one contents table holds
    /// thousands of rows, and nested folders three deep, one with a non-ASCII name. None is
    /// "Deleted Items", which PST Builder makes in every store: a session's folder path of that
    /// name makes a second folder beside it.
    /// </summary>
    static readonly WeightedFolder[] Folders =
    [
        new("Inbox", 30),
        new("Inbox/Projects/Engine", 8),
        new("Inbox/Projects/Notes", 6),
        new("Inbox/Projects/Été", 3),
        new("Inbox/Lists", 10),
        new("Sent Items", 15),
        new("Archive/2024/Q1", 5),
        new("Archive/2024/Q2", 5),
        new("Archive/2024/Q3", 5),
        new("Archive/2024/Q4", 5),
        new("Archive/2025", 6),
        new("Junk E-mail", 2),
    ];

    const int People = 200;
    const double ReplyShare = 0.3;
    const double InlineImageShare = 0.1;
    const double AttachmentShare = 0.18;
    const int SmallMin = 256, SmallMax = 512 * 1024;
    const double LargeShare = 0.002;
    const int LargeMin = 1024 * 1024, LargeMax = 16 * 1024 * 1024;

    public static int Run(string[] args)
    {
        string? dir = null;
        int seed = 16, messages = 50_000;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--seed" when i + 1 < args.Length && int.TryParse(args[i + 1], out seed):
                    i++;
                    break;
                case "--messages" when i + 1 < args.Length && int.TryParse(args[i + 1], out messages) && messages > 0:
                    i++;
                    break;
                case var arg when !arg.StartsWith("--") && dir is null:
                    dir = arg;
                    break;
                default:
                    return Usage();
            }
        }
        if (dir is null)
        {
            return Usage();
        }

        var parameters = new Parameters(seed, messages, Folders, People,
            "plain only 30%, plain and HTML 60%, HTML only 10%; 1 to 6 paragraphs of 1 to 5 sentences of 4 to 17 words",
            ReplyShare, InlineImageShare, AttachmentShare,
            "1 to 3, uniform",
            $"log-uniform from {SmallMin} to {SmallMax}",
            LargeShare,
            $"log-uniform from {LargeMin} to {LargeMax}, one more attachment");

        Directory.CreateDirectory(dir);
        var path = Path.Combine(dir, "scale.pst");
        // A previous run's parts would be mistaken for this one's.
        foreach (var old in Directory.GetFiles(dir, "scale*.pst"))
        {
            File.Delete(old);
        }

        var clock = Stopwatch.StartNew();
        var maker = new Maker(new Random(seed), seed);
        using var content = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var progress = new Progress<ExportProgress>(p =>
            Console.Error.WriteLine($"scale: {p.ItemsWritten} items, {p.BytesWritten:N0} bytes"));
        using var session = PstExportSession.Create(path, progress: progress, progressInterval: 5000);
        for (var n = 0; n < messages; n++)
        {
            var (folder, eml, received) = maker.Next(n);
            content.AppendData(Encoding.UTF8.GetBytes(folder + "\n"));
            content.AppendData(eml);
            session.AddEml(folder, eml, received);
        }
        session.Complete();
        // The session's parts are the files it wrote: scale.pst, then scale-002.pst and on, past
        // PST Builder's single-file limit.
        var parts = Directory.GetFiles(dir, "scale*.pst")
            .Order(StringComparer.Ordinal)
            .Select(file => new Part(Path.GetFileName(file), new FileInfo(file).Length))
            .ToList();
        clock.Stop();

        var record = new Record(Note, parameters, Fixtures.Tools(),
            Convert.ToHexStringLower(content.GetHashAndReset()), maker.Counts(messages), parts,
            Math.Round(clock.Elapsed.TotalSeconds, 1));
        var options = new JsonSerializerOptions
        {
            WriteIndented = true,
            Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        };
        File.WriteAllText(Path.Combine(dir, "scale.json"), JsonSerializer.Serialize(record, options) + "\n");
        Console.WriteLine($"scale: {messages} messages in {string.Join(", ", parts.Select(p => $"{p.File} ({p.Size:N0} bytes)"))}, "
            + $"content {record.ContentSha256}, {record.Seconds} s");
        return 0;
    }

    static int Usage()
    {
        Console.Error.WriteLine("usage: generator scale <dir> [--seed <n>] [--messages <n>]");
        return 2;
    }

    /// <summary>
    /// Makes the messages, one at a time, from one seeded Random. Every draw is made in a fixed
    /// order, so a seed gives the same sequence of messages on every run.
    /// </summary>
    sealed class Maker(Random random, int seed)
    {
        static readonly string[] First =
        [
            "Ada", "Charles", "Grace", "Alan", "Mary", "Edsger", "Barbara", "Donald", "Frances", "John",
            "Katherine", "Dennis", "Radia", "Ken", "Margaret", "Niklaus", "Hedy", "Tim", "Sophie", "Linus",
            "José", "Zoë", "Søren", "Anaïs", "Björn", "Анна", "Дмитрий", "王伟", "佐藤", "Łukasz",
        ];

        static readonly string[] Last =
        [
            "Lovelace", "Babbage", "Hopper", "Turing", "Somerville", "Dijkstra", "Liskov", "Knuth",
            "Allen", "Backus", "Johnson", "Ritchie", "Perlman", "Thompson", "Hamilton", "Wirth",
            "Lamarr", "Berners-Lee", "Wilson", "Torvalds", "Müller", "Ørsted", "Петрова", "Иванов",
            "Núñez", "Kowalczyk",
        ];

        static readonly string[] Words =
        [
            "engine", "note", "table", "number", "relay", "panel", "log", "book", "card", "loom",
            "punch", "mill", "store", "column", "row", "carry", "difference", "analytical", "program",
            "compiler", "routine", "subroutine", "memory", "drum", "tape", "valve", "circuit",
            "schedule", "meeting", "review", "draft", "report", "budget", "quarter", "figure",
            "the", "a", "of", "and", "to", "in", "with", "for", "on", "by", "from", "we", "shall",
            "café", "naïve", "résumé", "straße", "заметка", "таблица", "数字", "会議", "ἀριθμός",
        ];

        static readonly (string Extension, string Type, bool Text)[] Kinds =
        [
            ("txt", "text/plain", true),
            ("csv", "text/csv", true),
            ("pdf", "application/pdf", false),
            ("png", "image/png", false),
            ("jpg", "image/jpeg", false),
            ("zip", "application/zip", false),
            ("docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document", false),
            ("bin", "application/octet-stream", false),
        ];

        readonly List<string> subjects = [];
        readonly List<string> ids = [];
        readonly DateTime start = new(2024, 1, 1, 8, 0, 0, DateTimeKind.Utc);
        DateTime clock;
        int html, htmlOnly, replies, attachments, inlineImages;
        long attachmentBytes, largestAttachment, mimeBytes;

        public (string Folder, byte[] Eml, DateTime Received) Next(int n)
        {
            if (n == 0)
            {
                clock = start;
            }
            clock = clock.AddSeconds(30 + random.Next(1200));
            var received = clock.AddSeconds(1 + random.Next(90));
            var folder = Folder();
            var from = Person();
            var to = Enumerable.Range(0, 1 + random.Next(4)).Select(_ => Person()).ToList();
            var cc = Enumerable.Range(0, random.Next(4) == 0 ? 1 + random.Next(3) : 0).Select(_ => Person()).ToList();

            var reply = subjects.Count > 0 && random.NextDouble() < ReplyShare;
            string subject;
            string? parent = null;
            if (reply)
            {
                var at = random.Next(subjects.Count);
                subject = "Re: " + subjects[at];
                parent = ids[at];
                replies++;
            }
            else
            {
                subject = Sentence(3 + random.Next(6), false);
            }
            var id = $"<scale-{seed}-{n}@example.org>";
            subjects.Add(subject.StartsWith("Re: ") ? subject[4..] : subject);
            ids.Add(id);

            var paragraphs = Enumerable.Range(0, 1 + random.Next(6))
                .Select(_ => string.Join(" ", Enumerable.Range(0, 1 + random.Next(5)).Select(_ => Sentence(4 + random.Next(14), true))))
                .ToList();
            var shape = random.NextDouble();
            var hasHtml = shape >= 0.3;
            var hasText = shape < 0.9;
            var inline = hasHtml && random.NextDouble() < InlineImageShare;
            var files = new List<(string Name, string Type, byte[] Bytes)>();
            if (random.NextDouble() < AttachmentShare)
            {
                var count = 1 + random.Next(3);
                for (var i = 0; i < count; i++)
                {
                    files.Add(Attachment(n, i, SmallMin, SmallMax));
                }
            }
            if (random.NextDouble() < LargeShare)
            {
                files.Add(Attachment(n, files.Count, LargeMin, LargeMax));
            }
            byte[]? image = inline ? Bytes(2048 + random.Next(30 * 1024), false) : null;

            var eml = Mime(n, id, parent, from, to, cc, subject, received, paragraphs, hasText, hasHtml, image, files);
            html += hasHtml ? 1 : 0;
            htmlOnly += hasText ? 0 : 1;
            inlineImages += inline ? 1 : 0;
            attachments += files.Count;
            foreach (var file in files)
            {
                attachmentBytes += file.Bytes.Length;
                largestAttachment = Math.Max(largestAttachment, file.Bytes.Length);
            }
            mimeBytes += eml.Length;
            return (folder, eml, received);
        }

        public Counts Counts(int messages) => new(messages, html, htmlOnly, replies, attachments, inlineImages,
            attachmentBytes, largestAttachment, mimeBytes);

        string Folder()
        {
            var total = Folders.Sum(f => f.Weight);
            var pick = random.Next(total);
            foreach (var folder in Folders)
            {
                if (pick < folder.Weight)
                {
                    return folder.Path;
                }
                pick -= folder.Weight;
            }
            throw new UnreachableException();
        }

        (string Name, string Address) Person()
        {
            var i = random.Next(People);
            var first = First[i % First.Length];
            var last = Last[i / First.Length % Last.Length];
            return ($"{first} {last}", $"person{i}@example.org");
        }

        string Sentence(int words, bool period)
        {
            var text = string.Join(" ", Enumerable.Range(0, words).Select(_ => Words[random.Next(Words.Length)]));
            text = char.ToUpperInvariant(text[0]) + text[1..];
            return period ? text + "." : text;
        }

        /// <summary>One attachment of message n, its size drawn log-uniformly between min and max.</summary>
        (string Name, string Type, byte[] Bytes) Attachment(int n, int index, int min, int max)
        {
            var kind = Kinds[random.Next(Kinds.Length)];
            var size = (int)Math.Round(Math.Exp(Math.Log(min) + random.NextDouble() * (Math.Log(max) - Math.Log(min))));
            var name = $"{kind.Extension}-{n}-{index + 1}.{kind.Extension}";
            return (name, kind.Type, Bytes(size, kind.Text));
        }

        /// <summary>Text of words, or random bytes, of exactly `size` bytes.</summary>
        byte[] Bytes(int size, bool text)
        {
            var bytes = new byte[size];
            if (!text)
            {
                random.NextBytes(bytes);
                return bytes;
            }
            for (var i = 0; i < size; i++)
            {
                bytes[i] = (byte)(i % 64 == 63 ? '\n' : 'a' + random.Next(26));
            }
            return bytes;
        }

        static string Encoded(string text) =>
            text.All(c => c < 128) ? text : $"=?utf-8?B?{Convert.ToBase64String(Encoding.UTF8.GetBytes(text))}?=";

        static string Mailbox((string Name, string Address) person) => $"\"{Encoded(person.Name)}\" <{person.Address}>";

        static string Base64(byte[] bytes) => Convert.ToBase64String(bytes, Base64FormattingOptions.InsertLineBreaks);

        static byte[] Mime(int n, string id, string? parent, (string, string) from, List<(string, string)> to,
            List<(string, string)> cc, string subject, DateTime received, List<string> paragraphs, bool hasText,
            bool hasHtml, byte[]? image, List<(string Name, string Type, byte[] Bytes)> files)
        {
            var sb = new StringBuilder();
            void Line(string line) => sb.Append(line).Append("\r\n");
            Line($"Received: from mx.example.org by mail.example.org; {received:ddd, dd MMM yyyy HH:mm:ss} +0000");
            Line($"Message-ID: {id}");
            if (parent is not null)
            {
                Line($"In-Reply-To: {parent}");
                Line($"References: {parent}");
            }
            Line($"Date: {received.AddSeconds(-1):ddd, dd MMM yyyy HH:mm:ss} +0000");
            Line($"From: {Mailbox(from)}");
            Line($"To: {string.Join(", ", to.Select(Mailbox))}");
            if (cc.Count > 0)
            {
                Line($"Cc: {string.Join(", ", cc.Select(Mailbox))}");
            }
            Line($"Subject: {Encoded(subject)}");
            Line("MIME-Version: 1.0");

            var text = string.Join("\n\n", paragraphs).Replace("\n", "\r\n");
            var cid = $"image-{n}@example.org";
            var markup = "<html><body>" + string.Concat(paragraphs.Select((p, i) => i == 0 ? $"<p><b>{p}</b></p>" : $"<p>{p}</p>"))
                + (image is null ? "" : $"<p><img src=\"cid:{cid}\" alt=\"figure\"></p>") + "</body></html>";

            // The body, innermost first: text and HTML as an alternative, the inline image related
            // to the HTML, then the files mixed in.
            string Part(string type, byte[] bytes, string? extra = null) =>
                $"Content-Type: {type}\r\n{extra}Content-Transfer-Encoding: base64\r\n\r\n{Base64(bytes)}\r\n";
            string Multipart(string type, string boundary, IEnumerable<string> parts) =>
                $"Content-Type: multipart/{type}; boundary=\"{boundary}\"\r\n\r\n"
                + string.Concat(parts.Select(p => $"--{boundary}\r\n{p}"))
                + $"--{boundary}--\r\n";

            var textPart = Part("text/plain; charset=utf-8", Encoding.UTF8.GetBytes(text));
            var htmlPart = Part("text/html; charset=utf-8", Encoding.UTF8.GetBytes(markup));
            var content = hasText && hasHtml ? Multipart("alternative", $"alt-{n}", [textPart, htmlPart])
                : hasHtml ? htmlPart : textPart;
            if (image is not null)
            {
                content = Multipart("related", $"rel-{n}",
                [
                    content,
                    Part("image/png; name=\"figure.png\"", image,
                        $"Content-Disposition: inline; filename=\"figure.png\"\r\nContent-ID: <{cid}>\r\n"),
                ]);
            }
            if (files.Count > 0)
            {
                content = Multipart("mixed", $"mix-{n}",
                    [content, .. files.Select(f => Part($"{f.Type}; name=\"{f.Name}\"", f.Bytes,
                        $"Content-Disposition: attachment; filename=\"{f.Name}\"\r\n"))]);
            }
            sb.Append(content);
            return Encoding.UTF8.GetBytes(sb.ToString());
        }
    }
}
