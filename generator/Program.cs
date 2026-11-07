// The fixtures generator of spike-ai-pipeline-mail: hand-written .eml and item sources in, .pst
// and .msg fixtures out. Each mode is one command: `fixtures` builds the committed set, and
// `scale` a large store built from a seed, never committed (Scale.cs).

using System.Text;
using SpikeAiPipeline.Mail.Generator;

// .NET has only the Unicode and Latin-1 encodings built in. MimeKit registers the code pages
// itself; MsgKit doesn't, and fails on a windows-1251 or GB2312 source without them.
Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);

return args switch
{
    ["fixtures", var dir] => Fixtures.Run(dir),
    ["scale", .. var rest] => Scale.Run(rest),
    _ => Usage(),
};

static int Usage()
{
    Console.Error.WriteLine("usage: generator fixtures <tests/fixtures/generated>\n"
        + "       generator scale <dir> [--seed <n>] [--messages <n>]");
    return 2;
}
