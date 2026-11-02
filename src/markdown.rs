//! The derived Markdown body, made by htmd from HTML, or from plain text set as HTML so that
//! htmd escapes whatever Markdown would otherwise read in it.

/// Markdown from an HTML body. Images keep their sources as written, `cid:` ones included. The
/// head, styles and scripts are no part of the text: htmd would write a style's rules as text.
pub fn from_html(html: &str) -> Result<String, String> {
    htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["head", "style", "script"])
        .build()
        .convert(html)
        .map_err(|err| format!("can't be made into Markdown ({err})"))
}

/// Markdown that reads as `text` does: each paragraph, split at blank lines, is an HTML
/// paragraph whose lines end in line breaks, and its text is escaped as HTML, so htmd treats
/// every character as text and escapes the ones Markdown would read as markup. htmd 0.5.6
/// leaves four kinds of them, which [`escape_rest`] escapes. As in HTML, a run of spaces reads
/// as one.
pub fn from_text(text: &str) -> Result<String, String> {
    let text = text.replace("\r\n", "\n");
    let mut html = String::with_capacity(text.len() + 64);
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim_matches('\n');
        if paragraph.is_empty() {
            continue;
        }
        html.push_str("<p>");
        for (i, line) in paragraph.split('\n').enumerate() {
            if i > 0 {
                html.push_str("<br>");
            }
            escape(line, &mut html);
        }
        html.push_str("</p>");
    }
    from_html(&html).map(|markdown| escape_rest(&markdown))
}

/// Escapes what htmd leaves Markdown to read in text: `|` (a table), `~` (strikethrough), `&`
/// (a character reference), a line that starts as an ordered list item ending in `)`, and a
/// line's first character when it is one of `-=+#>*`, so that no line is a heading, a rule, a
/// quote or a list item, or underlines the line before it into a heading. htmd's own escapes
/// are a backslash and the character after it, copied as they are.
fn escape_rest(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len() + 16);
    for (i, line) in markdown.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let start = out.len();
        let digits = line.bytes().take_while(u8::is_ascii_digit).count();
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next_if_eq(&' ') {
            out.push(c);
        }
        if chars.peek().is_some_and(|c| "-=+#>*".contains(*c)) {
            out.push('\\');
        }
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    out.push(c);
                    out.extend(chars.next());
                }
                '|' | '~' | '&' => {
                    out.push('\\');
                    out.push(c);
                }
                ')' if (1..=9).contains(&digits) && out.len() - start == digits => {
                    out.push_str("\\)");
                }
                _ => out.push(c),
            }
        }
    }
    out
}

fn escape(text: &str, html: &mut String) {
    for c in text.chars() {
        match c {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            _ => html.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_reads_literally() {
        let text = "Hi *all* _x_ `c` [a](b) <t> & a|b ~~s~~ \\ end\r\n\
                    # not a heading\r\n- not an item\r\n> not a quote\r\n\
                    1) not an item\r\n2. not an item\r\nunderlined\r\n-----\r\n\
                    ===\r\n+ x\r\n\r\nnext paragraph";
        assert_eq!(
            from_text(text).unwrap(),
            "Hi \\*all\\* \\_x\\_ \\`c\\` \\[a\\](b) \\<t> \\& a\\|b \\~\\~s\\~\\~ \\\\ end  \n\
             \\# not a heading  \n\\- not an item  \n\\> not a quote  \n\
             1\\) not an item  \n2\\. not an item  \nunderlined  \n\\-----  \n\
             \\===  \n\\+ x\n\nnext paragraph"
        );
    }

    #[test]
    fn html_keeps_cid_images_and_drops_styles() {
        let html = "<html><head><title>T</title><style>p {color: red}</style></head>\
                    <body><p>A <b>moth</b>.</p><img src=\"cid:moth@example.net\" alt=\"moth\">\
                    <script>x()</script></body></html>";
        assert_eq!(
            from_html(html).unwrap(),
            "A **moth**.\n\n![moth](cid:moth@example.net)"
        );
    }
}
