mod common;

use common::fixture;
use lettera_core::model::BlockKind;
use lettera_core::parse::{
    extract_attachment, html_to_text, invalid_recipients, parse_body, parse_envelope,
    parse_recipients,
};
use lettera_core::reading;

#[test]
fn plain_message() {
    let raw = fixture("01_plain.eml");
    let env = parse_envelope(&raw);
    assert_eq!(env.message_id, "root-001@esempio.it");
    assert_eq!(env.from.name, "Anna Rossi");
    assert_eq!(env.from.addr, "anna@esempio.it");
    assert_eq!(env.to[0].addr, "marco@esempio.it");
    assert_eq!(env.subject, "Pianificazione Q4");
    assert_eq!(env.date, 1760339700);
    assert!(env.references.is_empty() && env.in_reply_to.is_none());
    let body = parse_body(&raw);
    assert!(body.text.contains("giovedì"));
    assert!(body.html.is_none());
    assert!(body.snippet.starts_with("Ciao Marco, ti propongo"));
}

#[test]
fn reply_headers_and_quotes() {
    let raw = fixture("02_reply_with_references.eml");
    let env = parse_envelope(&raw);
    assert_eq!(env.in_reply_to.as_deref(), Some("root-001@esempio.it"));
    assert_eq!(env.references, ["root-001@esempio.it"]);
    assert_eq!(env.cc[0].addr, "giulia@esempio.it");
    let body = parse_body(&raw);
    let blocks = reading::blocks(&body.text);
    assert_eq!(blocks[0].kind, BlockKind::Text);
    assert_eq!(blocks[1].kind, BlockKind::Quote);
    assert_eq!(blocks.len(), 2);
}

#[test]
fn header_only_parse_matches_full() {
    let raw = fixture("02_reply_with_references.eml");
    let headers = common::header_part(&raw);
    assert_eq!(parse_envelope(&headers), parse_envelope(&raw));
}

#[test]
fn multipart_alternative_prefers_plain_text() {
    let raw = fixture("06_multipart_alternative.eml");
    let env = parse_envelope(&raw);
    assert_eq!(env.subject, "Le novità della settimana");
    assert_eq!(env.from.name, "Notizie");
    let body = parse_body(&raw);
    assert!(body.text.contains("in formato testo"));
    let html = body.html.expect("html part");
    assert!(html.contains("<h1>"));
    assert!(body.attachments.is_empty());
}

#[test]
fn html_only_is_sanitized_and_converted() {
    let raw = fixture("07_html_only.eml");
    let body = parse_body(&raw);
    let html = body.html.unwrap();
    assert!(!html.contains("<script"));
    assert!(!html.contains("onload"));
    assert!(!html.contains("alert"));
    assert!(body.text.contains("4815"));
    assert!(body.text.contains("spedito"));
    assert!(!body.text.contains("alert"));
    let v = reading::html_document(&html, lettera_core::privacy::Images::Blocked, "");
    assert!(!v.html.contains("pixel.png"));
    assert_eq!((v.blocked, v.pixels), (0, 1));
    assert!(body.headers.is_some_and(|h| h.contains("Subject:")));
}

#[test]
fn pdf_attachment() {
    let raw = fixture("08_attachment_pdf.eml");
    let env = parse_envelope(&raw);
    assert!(env.has_attachments);
    let body = parse_body(&raw);
    assert_eq!(body.attachments.len(), 1);
    let a = &body.attachments[0];
    assert_eq!(a.filename, "fattura-ottobre.pdf");
    assert_eq!(a.mime, "application/pdf");
    let bytes = extract_attachment(&raw, a.index).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert_eq!(bytes.len(), a.size);
}

#[test]
fn latin1_quoted_printable() {
    let raw = fixture("09_latin1_qp.eml");
    let env = parse_envelope(&raw);
    assert_eq!(env.subject, "Perché sì");
    assert_eq!(env.from.name, "Nicolò Ferrà");
    let body = parse_body(&raw);
    assert!(body.text.contains("Perché sì, così è più facile. Città!"));
}

#[test]
fn windows_1252_8bit() {
    let body = parse_body(&fixture("10_windows1252_8bit.eml"));
    assert!(
        body.text.contains("Il prezzo è 10 € — «offerta»."),
        "{}",
        body.text
    );
}

#[test]
fn folded_headers() {
    let env = parse_envelope(&fixture("11_folded_headers.eml"));
    assert_eq!(env.message_id, "folded-011@esempio.it");
    assert_eq!(
        env.references,
        [
            "root-001@esempio.it",
            "reply-002@esempio.it",
            "reply-003@esempio.it"
        ]
    );
    assert_eq!(env.from.addr, "marco@esempio.it");
    assert_eq!(env.to.len(), 2);
    assert!(env.subject.contains("spezzato su due righe"));
    assert!(!env.subject.contains('\n'));
}

#[test]
fn forwarded_message_attachment() {
    let raw = fixture("12_forward_rfc822.eml");
    let body = parse_body(&raw);
    assert!(body.text.contains("Vedi sotto"));
    assert_eq!(body.attachments.len(), 1);
    assert_eq!(body.attachments[0].filename, "message.eml");
    let inner = extract_attachment(&raw, body.attachments[0].index).unwrap();
    assert!(String::from_utf8_lossy(&inner).contains("inoltrato come allegato"));
}

#[test]
fn missing_message_id_is_synthesized_stably() {
    let raw = fixture("13_no_message_id.eml");
    let a = parse_envelope(&raw);
    let b = parse_envelope(&common::header_part(&raw));
    assert!(a.message_id.starts_with("lettera-"));
    assert_eq!(a.message_id, b.message_id);
}

#[test]
fn outlook_quote_collapses() {
    let body = parse_body(&fixture("14_outlook_quote.eml"));
    let blocks = reading::blocks(&body.text);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].text, "Confermo.");
    assert_eq!(blocks[1].kind, BlockKind::Quote);
}

#[test]
fn recipients_field() {
    let r = parse_recipients("a@b.it, \"Rossi, Anna\" <anna@x.it>; Marco <m@x.it>,, nonvalido");
    let addrs: Vec<_> = r.iter().map(|a| a.addr.as_str()).collect();
    assert_eq!(addrs, ["a@b.it", "anna@x.it", "m@x.it"]);
    assert_eq!(r[1].name, "Rossi, Anna");
}

#[test]
fn entries_that_are_not_addresses_are_reported() {
    let f = "a@b.it, \"Rossi, Anna\" <anna@x.it>; Marco <m@x.it>,, nonvalido, mario(at)x.it";
    assert_eq!(invalid_recipients(f), ["nonvalido", "mario(at)x.it"]);
    assert!(invalid_recipients("a@b.it, ").is_empty());
}

#[test]
fn layout_tables_read_as_plain_text() {
    let html = r#"<table><tr><td><table><tr><td><h4>Ciao Anna</h4></td></tr>
        <tr><td>Hai pagato 10 &euro;</td><td>oggi</td></tr></table></td></tr>
        <tr><td><a href="https://esempio.it/ricevuta">Vedi la ricevuta</a></td></tr></table>"#;
    let text = html_to_text(html);
    assert!(!text.contains('─') && !text.contains('│'), "{text}");
    assert!(!text.contains("####"), "{text}");
    assert!(text.lines().any(|l| l == "Ciao Anna"), "{text}");
    assert!(text.contains("Hai pagato 10 €"), "{text}");
    assert!(text.contains("https://esempio.it/ricevuta"), "{text}");
}
