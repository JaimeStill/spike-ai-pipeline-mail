//! The RTF body: PidTagRtfCompressed decompressed through compressed-rtf (MS-OXRTFCP), and the
//! HTML an RTF body encapsulates recovered from it (MS-OXRTFEX 2.2.3), which is the CLI's own
//! code.
//!
//! De-encapsulation follows MS-OXRTFEX 2.2.3.2, with these choices:
//! - text and `\'hh` escapes are decoded in the header's `\ansicpg` code page, else 1252; the
//!   per-font code pages of the font table are not tracked;
//! - an `\*\mhtmltag` group is skipped (2.1.3.1.5): it holds a rewritten copy of a tag whose
//!   original, its `cid:` reference included, is in an `\*\htmltag` group (footnote 8).

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::codepage::{self, FALLBACK};
use crate::panic_text;

/// The header of compressed RTF (MS-OXRTFCP 2.1.3.1.1): COMPSIZE, RAWSIZE, COMPTYPE and CRC.
const HEADER: usize = 16;

/// COMPTYPE of uncompressed RTF, "MELA".
const UNCOMPRESSED: u32 = 0x414C_454D;

/// Decompresses PidTagRtfCompressed, or says why it can't. compressed-rtf 1.0.1 slices its
/// input by the header's sizes unchecked, so those are checked first, and any other panic it
/// raises is caught here and becomes the reason.
pub fn decompress(compressed: &[u8]) -> Result<String, String> {
    if compressed.len() < HEADER {
        return Err(format!(
            "is {} bytes, shorter than its {HEADER}-byte header",
            compressed.len()
        ));
    }
    let word = |at: usize| {
        u32::from_le_bytes([
            compressed[at],
            compressed[at + 1],
            compressed[at + 2],
            compressed[at + 3],
        ])
    };
    let raw_size = word(4) as usize;
    if word(8) == UNCOMPRESSED && raw_size > compressed.len() - HEADER {
        return Err(format!(
            "says it holds {raw_size} bytes of uncompressed RTF, more than it has"
        ));
    }
    match catch_unwind(AssertUnwindSafe(|| {
        compressed_rtf::decompress_rtf(compressed)
    })) {
        Ok(Ok(rtf)) => Ok(rtf),
        Ok(Err(err)) => Err(format!("can't be decompressed ({err})")),
        Err(payload) => Err(format!(
            "can't be decompressed (the decompressor panicked: {})",
            panic_text(&*payload)
        )),
    }
}

/// What an RTF document encapsulates (MS-OXRTFEX 2.2.3.1).
#[derive(Debug, PartialEq)]
enum Encapsulated {
    Html,
    Text,
    None,
}

/// How many tokens from the start the encapsulation is looked for in (MS-OXRTFEX 2.2.3.1).
const RECOGNIZE_TOKENS: usize = 10;

fn recognize(rtf: &[u8]) -> Encapsulated {
    if !rtf.starts_with(b"{\\rtf1") {
        return Encapsulated::None;
    }
    let mut lexer = Lexer { rtf, at: 0 };
    for _ in 0..RECOGNIZE_TOKENS {
        match lexer.next() {
            Some(Token::Open) => {}
            Some(Token::Word(b"fromhtml", Some(1))) => return Encapsulated::Html,
            Some(Token::Word(b"fromtext", _)) => return Encapsulated::Text,
            Some(Token::Word(..)) => {}
            _ => return Encapsulated::None,
        }
    }
    Encapsulated::None
}

/// The HTML an RTF document encapsulates, with notes on any text decoded lossily, or None when
/// it encapsulates no HTML (\fromtext, or plain RTF). `rtf` is as compressed-rtf gives it: each
/// char one byte of the RTF.
pub fn html(rtf: &str) -> Option<(String, Vec<String>)> {
    let bytes: Vec<u8> = rtf
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect();
    (recognize(&bytes) == Encapsulated::Html).then(|| Deencapsulator::new(&bytes).run())
}

/// One RTF token (RTF 1.9.1, "Conventions of an RTF reader").
#[derive(Debug, PartialEq)]
enum Token<'a> {
    Open,
    Close,
    /// A control word and its numeric parameter.
    Word(&'a [u8], Option<i32>),
    /// A control symbol other than `\'`: `\{`, `\~`, `\*` and the like.
    Symbol(u8),
    /// `\'hh`.
    Hex(u8),
    /// One byte of text.
    Text(u8),
    /// The bytes of `\binN`, which are data and never text.
    Binary,
}

struct Lexer<'a> {
    rtf: &'a [u8],
    at: usize,
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            let byte = *self.rtf.get(self.at)?;
            self.at += 1;
            return Some(match byte {
                b'{' => Token::Open,
                b'}' => Token::Close,
                // Line ends in RTF are not text.
                b'\r' | b'\n' => continue,
                b'\\' => self.control(),
                _ => Token::Text(byte),
            });
        }
    }

    fn peek(&self) -> Option<u8> {
        self.rtf.get(self.at).copied()
    }

    /// The token after a backslash.
    fn control(&mut self) -> Token<'a> {
        let Some(first) = self.peek() else {
            return Token::Text(b'\\');
        };
        if !first.is_ascii_alphabetic() {
            self.at += 1;
            if first != b'\'' {
                return Token::Symbol(first);
            }
            let hex = self
                .rtf
                .get(self.at..self.at + 2)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok());
            return match hex {
                Some(byte) => {
                    self.at += 2;
                    Token::Hex(byte)
                }
                // A malformed escape is dropped.
                None => Token::Symbol(b'\''),
            };
        }
        let start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_alphabetic()) {
            self.at += 1;
        }
        let word = &self.rtf[start..self.at];
        let digits = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        let parameter = std::str::from_utf8(&self.rtf[digits..self.at])
            .ok()
            .and_then(|digits| digits.parse::<i32>().ok());
        if parameter.is_none() {
            // A lone '-' is text, not part of the word.
            self.at = digits;
        }
        // One space delimits a control word and is part of it.
        if self.peek() == Some(b' ') {
            self.at += 1;
        }
        if word == b"bin" {
            let skip = parameter.and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
            self.at = self.at.saturating_add(skip).min(self.rtf.len());
            return Token::Binary;
        }
        Token::Word(word, parameter)
    }
}

/// What a group's content is to the de-encapsulator.
#[derive(Clone, Copy, PartialEq)]
enum Destination {
    /// The document's own text: copied unless \htmlrtf suppresses it.
    Text,
    /// An `\*\htmltag` group's HTML fragment: always copied.
    HtmlTag,
    /// A destination that is not visible text: skipped.
    Skip,
}

/// The state a group starts with, a copy of its parent's, restored when it ends.
#[derive(Clone, Copy)]
struct State {
    destination: Destination,
    /// Whether \htmlrtf suppresses text and control words.
    suppressed: bool,
    /// \ucN: how many characters stand in for each \uN that follows.
    uc: usize,
}

/// Destinations that are standard RTF, not `\*`, and never visible text (RTF 1.9.1).
const INVISIBLE: &[&[u8]] = &[
    b"fonttbl",
    b"colortbl",
    b"stylesheet",
    b"info",
    b"pict",
    b"object",
    b"listtable",
    b"listoverridetable",
    b"revtbl",
    b"rsidtbl",
    b"filetbl",
    b"header",
    b"headerl",
    b"headerr",
    b"headerf",
    b"footer",
    b"footerl",
    b"footerr",
    b"footerf",
    b"fldinst",
    b"bkmkstart",
    b"bkmkend",
    b"xe",
    b"tc",
];

struct Deencapsulator<'a> {
    lexer: Lexer<'a>,
    state: State,
    stack: Vec<State>,
    /// Whether the next token is the first in its group, where a destination is named.
    group_start: bool,
    /// Whether the group was opened with `\*`, an ignorable destination.
    starred: bool,
    /// How many characters after a \uN are still to be skipped.
    skip: usize,
    out: Out,
}

/// The HTML being written, with the bytes and UTF-16 units of the current run not yet decoded.
struct Out {
    html: String,
    bytes: Vec<u8>,
    units: Vec<u16>,
    code_page: u16,
    notes: Vec<String>,
}

impl Out {
    fn byte(&mut self, byte: u8) {
        self.flush_units();
        self.bytes.push(byte);
    }

    fn unit(&mut self, unit: u16) {
        self.flush_bytes();
        self.units.push(unit);
    }

    fn text(&mut self, text: &str) {
        self.flush();
        self.html.push_str(text);
    }

    fn flush(&mut self) {
        self.flush_bytes();
        self.flush_units();
    }

    fn flush_bytes(&mut self) {
        if self.bytes.is_empty() {
            return;
        }
        let (text, lossy) = codepage::decode(&self.bytes, self.code_page);
        if let Some(lossy) = lossy {
            let note = format!("the RTF body's encapsulated HTML {lossy}");
            if !self.notes.contains(&note) {
                self.notes.push(note);
            }
        }
        self.html.push_str(&text);
        self.bytes.clear();
    }

    fn flush_units(&mut self) {
        if !self.units.is_empty() {
            self.html.push_str(&String::from_utf16_lossy(&self.units));
            self.units.clear();
        }
    }
}

impl<'a> Deencapsulator<'a> {
    fn new(rtf: &'a [u8]) -> Self {
        let code_page = ansicpg(rtf).unwrap_or(FALLBACK);
        Self {
            lexer: Lexer { rtf, at: 0 },
            state: State {
                destination: Destination::Text,
                suppressed: false,
                uc: 1,
            },
            stack: Vec::new(),
            group_start: false,
            starred: false,
            skip: 0,
            out: Out {
                html: String::new(),
                bytes: Vec::new(),
                units: Vec::new(),
                code_page,
                notes: Vec::new(),
            },
        }
    }

    fn run(mut self) -> (String, Vec<String>) {
        while let Some(token) = self.lexer.next() {
            let first = std::mem::take(&mut self.group_start);
            match token {
                Token::Open => {
                    self.stack.push(self.state);
                    self.group_start = true;
                    self.starred = false;
                    self.skip = 0;
                }
                Token::Close => {
                    if let Some(state) = self.stack.pop() {
                        self.state = state;
                    }
                    self.skip = 0;
                }
                Token::Symbol(b'*') if first => {
                    self.starred = true;
                    // The destination's word still comes first.
                    self.group_start = true;
                }
                Token::Word(word, _) if first && self.opens(word) => {}
                token => self.content(token),
            }
        }
        self.out.flush();
        (self.out.html, self.out.notes)
    }

    /// Names the group's destination by its first word; true when the word did.
    fn opens(&mut self, word: &[u8]) -> bool {
        let inside_tag = self.state.destination == Destination::HtmlTag;
        let destination = match word {
            b"htmltag" if self.starred => Destination::HtmlTag,
            // \*\mhtmltag among them: MS-OXRTFEX 2.1.3.1.5 has it skipped.
            _ if self.starred => Destination::Skip,
            _ if INVISIBLE.contains(&word) => Destination::Skip,
            _ => return false,
        };
        if self.state.destination != Destination::Skip && !inside_tag {
            self.state.destination = destination;
        } else if inside_tag && destination != Destination::HtmlTag {
            // A destination nested in a tag's fragment is not part of it.
            self.state.destination = Destination::Skip;
        }
        true
    }

    /// One token of a group's content.
    fn content(&mut self, token: Token) {
        let destination = self.state.destination;
        if destination == Destination::Skip {
            return;
        }
        let in_tag = destination != Destination::Text;
        if let Token::Word(b"htmlrtf", parameter) = token {
            if !in_tag {
                self.state.suppressed = parameter != Some(0);
            }
            return;
        }
        if let Token::Word(b"uc", Some(n)) = token {
            self.state.uc = usize::try_from(n).unwrap_or(0);
            return;
        }
        if !in_tag && self.state.suppressed {
            return;
        }
        // The characters standing in for a \uN are skipped: one per byte of text or escape,
        // or per control word.
        if self.skip > 0 && matches!(token, Token::Text(_) | Token::Hex(_) | Token::Word(..)) {
            self.skip -= 1;
            return;
        }
        match token {
            Token::Text(byte) | Token::Hex(byte) => self.out.byte(byte),
            Token::Symbol(symbol @ (b'{' | b'}' | b'\\')) => self.out.byte(symbol),
            Token::Symbol(b'~') => self.out.text("\u{A0}"),
            Token::Symbol(b'_') => self.out.text("\u{AD}"),
            Token::Word(b"u", Some(n)) => {
                // \uN is a signed 16-bit value.
                self.out.unit(n as u16);
                self.skip = self.state.uc;
            }
            Token::Word(word, _) => match word {
                b"par" => self.out.text("\r\n"),
                b"line" if !in_tag => self.out.text("\r\n"),
                b"tab" => self.out.text("\t"),
                b"lquote" => self.out.text("\u{2018}"),
                b"rquote" => self.out.text("\u{2019}"),
                b"ldblquote" => self.out.text("\u{201C}"),
                b"rdblquote" => self.out.text("\u{201D}"),
                b"bullet" => self.out.text("\u{2022}"),
                b"endash" => self.out.text("\u{2013}"),
                b"emdash" => self.out.text("\u{2014}"),
                _ => {}
            },
            _ => {}
        }
    }
}

/// The header's \ansicpgN, the default code page of the document's text.
fn ansicpg(rtf: &[u8]) -> Option<u16> {
    let mut lexer = Lexer { rtf, at: 0 };
    while let Some(token) = lexer.next() {
        match token {
            Token::Word(b"ansicpg", Some(n)) => return u16::try_from(n).ok(),
            // The header ends where the font table, or the text, starts.
            Token::Word(b"fonttbl", _) | Token::Text(_) => return None,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The HTML recovered from `rtf`, given as the bytes of the RTF, with its notes.
    fn recover(rtf: &[u8]) -> Option<(String, Vec<String>)> {
        html(&rtf.iter().map(|&byte| char::from(byte)).collect::<String>())
    }

    fn recovered(rtf: &[u8]) -> String {
        let (html, notes) = recover(rtf).expect("encapsulated HTML");
        assert_eq!(notes, Vec::<String>::new());
        html
    }

    /// An HTML-encapsulating document around `body`, with its header and font table.
    fn document(code_page: u16, body: &str) -> Vec<u8> {
        format!(
            "{{\\rtf1\\ansi\\ansicpg{code_page}\\fromhtml1 \\deff0{{\\fonttbl\r\n\
             {{\\f0\\fswiss Arial;}}{{\\f1\\fmodern Courier New;}}}}\r\n\
             {{\\colortbl\\red0\\green0\\blue0;}}\r\n\
             {{\\*\\generator Microsoft Exchange Server;}}\r\n{body}}}"
        )
        .into_bytes()
    }

    #[test]
    fn only_fromhtml1_encapsulates_html() {
        assert_eq!(
            recover(b"{\\rtf1\\ansi\\fromtext \\deff0 Hello\\par }"),
            None
        );
        assert_eq!(recover(b"{\\rtf1\\ansi\\deff0 Hello\\par }"), None);
        assert_eq!(recover(b"{\\rtf1\\ansi\\fromhtml0 Hello}"), None);
        assert_eq!(recover(b"<html>{\\rtf1\\fromhtml1 }"), None);
        // Past the first ten tokens, \fromhtml1 is not looked for.
        let late = b"{\\rtf1\\a\\b\\c\\d\\e\\f\\g\\h\\i\\j\\fromhtml1 x}";
        assert_eq!(recover(late), None);
        assert!(recover(b"{\\rtf1\\ansi\\fromhtml1 x}").is_some());
    }

    #[test]
    fn tags_are_copied_and_htmlrtf_text_is_not() {
        let rtf = document(
            1252,
            "{\\*\\htmltag19 <html>}{\\*\\htmltag34 <head>}{\\*\\htmltag41 </head>}\r\n\
             {\\*\\htmltag50 <body>}\\htmlrtf {\\f0\\fs20 \\htmlrtf0 \
             {\\*\\htmltag64 <p class=\"x\">}\\htmlrtf {\\htmlrtf0 Hello, {\\*\\htmltag84 <b>}\
             \\htmlrtf\\b \\htmlrtf0 world{\\*\\htmltag92 </b>}}\\htmlrtf\\par\\htmlrtf0\
             {\\*\\htmltag72 </p>\\par}}\
             {\\*\\htmltag58 </body>}{\\*\\htmltag27 </html>}",
        );
        assert_eq!(
            recovered(&rtf),
            "<html><head></head><body><p class=\"x\">Hello, <b>world</b></p>\r\n</body></html>"
        );
    }

    #[test]
    fn htmlrtf_state_is_restored_when_a_group_ends() {
        // Suppression begun inside a group ends with it; ended inside one, it resumes after.
        let rtf = document(
            1252,
            "a{\\htmlrtf b}c\\htmlrtf d{\\htmlrtf0 e{f}}g\\htmlrtf0 h{{{\\htmlrtf i}j}k}",
        );
        assert_eq!(recovered(&rtf), "acefhjk");
    }

    #[test]
    fn nested_groups_inside_a_tag_are_part_of_it() {
        let rtf = document(
            1252,
            "{\\*\\htmltag <a href=\"x\" {title}=\"{\\b y}\">}{\\*\\htmltag {\\*\\other no}</a>}",
        );
        assert_eq!(recovered(&rtf), "<a href=\"x\" title=\"y\"></a>");
    }

    #[test]
    fn par_tab_and_escapes_in_and_out_of_tags() {
        let rtf = document(
            1252,
            "{\\*\\htmltag <style>\\par p\\{x:\\\\y\\}\\tab\\par </style>}\
             a\\{b\\}c\\\\d\\tab e\\line f\\~g\\lquote h\\rquote\\emdash\\bullet",
        );
        assert_eq!(
            recovered(&rtf),
            "<style>\r\np{x:\\y}\t\r\n</style>a{b}c\\d\te\r\nf\u{A0}g\u{2018}h\u{2019}\u{2014}\u{2022}"
        );
    }

    #[test]
    fn hex_escapes_are_decoded_in_the_ansicpg() {
        // Windows-1251 Cyrillic, in a tag and in text.
        let rtf = document(
            1251,
            "{\\*\\htmltag <p title=\"\\'cf\">}\\'cf\\'f0\\'e8\\'e2\\'e5\\'f2",
        );
        assert_eq!(recovered(&rtf), "<p title=\"П\">Привет");
        // GBK: the bytes of one character are decoded together.
        let rtf = document(936, "\\'b4\\'b2\\'c7\\'b0");
        assert_eq!(recovered(&rtf), "床前");
        // No \ansicpg: Windows-1252.
        let rtf = b"{\\rtf1\\ansi\\fromhtml1 caf\\'e9}";
        assert_eq!(recovered(rtf), "café");
    }

    #[test]
    fn bytes_not_valid_in_the_code_page_are_decoded_lossily_with_a_note() {
        let (html, notes) = recover(&document(65001, "a\\'ffb")).unwrap();
        assert_eq!(html, "a\u{FFFD}b");
        assert_eq!(
            notes,
            ["the RTF body's encapsulated HTML is not valid in code page 65001; decoded lossily"]
        );
    }

    #[test]
    fn unicode_escapes_skip_their_uc_fallbacks() {
        // \uc1 by default: one fallback character, here an escape.
        let rtf = document(1252, "\\u1055\\'3f\\u1088?x");
        assert_eq!(recovered(&rtf), "Прx");
        // \uc2 skips two; \uc0 none. A negative \uN is its 16-bit value.
        let rtf = document(1252, "\\uc2\\u8364??a\\uc0\\u-3913b");
        assert_eq!(recovered(&rtf), "€a\u{F0B7}b");
        // A surrogate pair in two \uN.
        let rtf = document(1252, "\\u-10179?\\u-8701?");
        assert_eq!(recovered(&rtf), "😃");
        // \ucN is restored when its group ends; a group's end stops the skipping.
        let rtf = document(1252, "{\\uc3 \\u1055???}\\u1088?{\\u1080}x");
        assert_eq!(recovered(&rtf), "Приx");
        // In a tag too.
        let rtf = document(1252, "{\\*\\htmltag <p title=\"\\u1055?\">}");
        assert_eq!(recovered(&rtf), "<p title=\"П\">");
    }

    #[test]
    fn mhtmltag_is_skipped_and_its_htmltag_kept() {
        let rtf = document(
            1252,
            "{\\*\\mhtmltag84 <img src=\"\\htmlbase http://x/moth.png\">}\
             {\\*\\htmltag84 <img src=\"cid:moth@example.net\">}",
        );
        assert_eq!(recovered(&rtf), "<img src=\"cid:moth@example.net\">");
    }

    #[test]
    fn invisible_destinations_and_binary_data_are_skipped() {
        let rtf = document(
            1252,
            "a{\\info{\\title T}}{\\pict\\pngblip 89504e47}{\\*\\bkmkstart m}b\\bin3 {}}c",
        );
        assert_eq!(recovered(&rtf), "abc");
    }

    #[test]
    fn decompression_round_trips_and_refuses_what_it_cant_read() {
        let rtf = "{\\rtf1\\ansi\\fromhtml1 {\\*\\htmltag <p>}Hi}";
        let compressed = compressed_rtf::compress_rtf(rtf).unwrap();
        assert_eq!(decompress(&compressed).as_deref(), Ok(rtf));

        assert_eq!(
            decompress(&compressed[..10]),
            Err("is 10 bytes, shorter than its 16-byte header".to_owned())
        );
        let mut bad_crc = compressed.clone();
        *bad_crc.last_mut().unwrap() ^= 0xFF;
        assert!(
            decompress(&bad_crc)
                .unwrap_err()
                .starts_with("can't be decompressed (")
        );

        // Uncompressed ("MELA") RTF is read as it is, unless its size runs past its end.
        let mut mela = Vec::new();
        for word in [12 + 4, 4, UNCOMPRESSED, 0] {
            mela.extend_from_slice(&u32::to_le_bytes(word));
        }
        mela.extend_from_slice(b"{\\x}");
        assert_eq!(decompress(&mela).as_deref(), Ok("{\\x}"));
        mela[4] = 200;
        assert_eq!(
            decompress(&mela),
            Err("says it holds 200 bytes of uncompressed RTF, more than it has".to_owned())
        );
    }
}
