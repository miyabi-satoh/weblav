//! 簡易チャットに付けるファイルを、窓口へ送る形に直す (→ docs/ai.md「簡易チャット」)。
//! PDF と画像は添付のまま、テキストと Office の文書は文字にする。Gemini は PDF 以外の文書をただの文字として読むため。

use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipArchive;

use crate::ai::{Attachment, MAX_INPUT_UNITS};
use crate::error::AppError;
use crate::file_ext::extension;

/// テキストかを見る先頭のバイト数。`text_files` (テキストのビューアー) と同じく、NUL があればテキストでない。
const SNIFF_BYTES: usize = 8000;

/// Office の文書の中の1つの XML を、展開して読む上限。小さな zip が展開で膨らむもの (zip bomb) で、
/// メモリを使い切らないため。文字は 20 万字までしか送らないので、ふつうの文書はこれに届かない。
const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;

/// 送る形に直したファイル。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Converted {
    /// 添付として送る。
    Attachment(Attachment),
    /// 取り出した文字。質問と一緒に文として送る。
    Text(String),
    /// 中身は送らない (音声・動画・読めない形式)。名前だけを伝える。
    NameOnly,
}

/// 種類 (`mime_type`。無ければ `name` の拡張子) と中身から、送る形を決める。中身が無ければ名前だけ。
/// 画像のビューアーの題名のように、名前が拡張子を持たないことがあるので、種類があればそちらを先に見る。
/// 文字にしたものが窓口の上限を超えるときは `AiTooLarge`。
/// 展開と文字の取り出しで重くなるので、`run_blocking` の中から呼ぶ。
pub(super) fn convert(
    name: &str,
    mime_type: Option<&str>,
    data: Option<&[u8]>,
) -> Result<Converted, AppError> {
    let Some(data) = data else {
        return Ok(Converted::NameOnly);
    };
    let ext = mime_type
        .and_then(extension_of_type)
        .or_else(|| extension(name).map(str::to_ascii_lowercase));
    let ext = ext.as_deref().unwrap_or_default();
    if let Some(mime_type) = attachment_type(ext) {
        use base64::Engine as _;
        return Ok(Converted::Attachment(Attachment {
            mime_type,
            data: base64::engine::general_purpose::STANDARD.encode(data),
        }));
    }
    let text = if matches!(ext, "docx" | "xlsx" | "pptx") {
        office_text(ext, data)?
    } else if is_text(data) {
        decode_text(data)
    } else {
        return Ok(Converted::NameOnly);
    };
    Ok(Converted::Text(text))
}

/// 種類 (MIME) に当たる拡張子。見る種類だけ。
fn extension_of_type(mime_type: &str) -> Option<String> {
    let essence = mime_type.split(';').next()?.trim().to_ascii_lowercase();
    let ext = match essence.as_str() {
        "application/pdf" => "pdf",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        _ => return None,
    };
    Some(ext.to_string())
}

/// 窓口が添付として受ける種類。Gemini が中身を読めるもの (窓口の `ATTACHMENT_TYPES` と揃える)。
fn attachment_type(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => return None,
    })
}

fn is_text(data: &[u8]) -> bool {
    !data[..data.len().min(SNIFF_BYTES)].contains(&0)
}

/// UTF-8 で読めなければ Shift_JIS で読む。テキストのビューアー (`text-view.svelte`) と同じ判定。
fn decode_text(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(text) => text.strip_prefix('\u{feff}').unwrap_or(text).to_string(),
        Err(_) => encoding_rs::SHIFT_JIS.decode(data).0.into_owned(),
    }
}

/// Office の文書 (Office Open XML) の文字。Word は段落ごと、PowerPoint はスライドごと、
/// Excel はシートの行ごとにセルをタブで区切る。窓口の上限を超えたら、そこで読むのをやめて `AiTooLarge`。
fn office_text(ext: &str, data: &[u8]) -> Result<String, AppError> {
    let mut zip = ZipArchive::new(Cursor::new(data)).map_err(|_| unreadable())?;
    let mut text = Text::default();
    if ext == "docx" {
        paragraphs(&read_xml(&mut zip, "word/document.xml")?, &mut text)?;
    } else if ext == "pptx" {
        for (number, part) in numbered_parts(&zip, "ppt/slides/slide") {
            text.heading(&format!("Slide {number}"))?;
            paragraphs(&read_xml(&mut zip, &part)?, &mut text)?;
        }
    } else {
        let shared = shared_strings(&mut zip)?;
        for (number, part) in numbered_parts(&zip, "xl/worksheets/sheet") {
            text.heading(&format!("Sheet {number}"))?;
            cells(&read_xml(&mut zip, &part)?, &shared, &mut text)?;
        }
    }
    Ok(text.finish())
}

fn unreadable() -> AppError {
    AppError::Validation("the file could not be read".to_string())
}

/// 取り出している文字。窓口の上限 (UTF-16 の単位) を超えたら止める。
#[derive(Default)]
struct Text {
    out: String,
    units: usize,
}

impl Text {
    fn push(&mut self, part: &str) -> Result<(), AppError> {
        self.units += part.encode_utf16().count();
        if self.units > MAX_INPUT_UNITS {
            return Err(AppError::AiTooLarge);
        }
        self.out.push_str(part);
        Ok(())
    }

    fn heading(&mut self, title: &str) -> Result<(), AppError> {
        let gap = if self.out.is_empty() { "" } else { "\n" };
        self.push(&format!("{gap}## {title}\n"))
    }

    fn finish(self) -> String {
        self.out.trim_end().to_string()
    }
}

fn read_xml<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    part: &str,
) -> Result<String, AppError> {
    let file = zip.by_name(part).map_err(|_| unreadable())?;
    let mut xml = String::new();
    file.take(MAX_XML_BYTES)
        .read_to_string(&mut xml)
        .map_err(|_| unreadable())?;
    Ok(xml)
}

/// `prefix` に続く番号の付いた XML (`slide1.xml`・`sheet2.xml` …) を、番号の順に。
fn numbered_parts<R: Read + std::io::Seek>(
    zip: &ZipArchive<R>,
    prefix: &str,
) -> Vec<(u32, String)> {
    let mut parts: Vec<(u32, String)> = zip
        .file_names()
        .filter_map(|name| {
            let number = name
                .strip_prefix(prefix)?
                .strip_suffix(".xml")?
                .parse()
                .ok()?;
            Some((number, name.to_string()))
        })
        .collect();
    parts.sort_unstable();
    parts
}

/// 文字の要素 (`w:t`・`a:t`) の中身と実体参照。`text` の要素の中だけを読む。
fn text_events(
    xml: &str,
    mut each: impl FnMut(XmlPart<'_>) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let mut reader = Reader::from_str(xml);
    let mut in_text = false;
    loop {
        match reader.read_event().map_err(|_| unreadable())? {
            Event::Start(tag) => {
                in_text = tag.local_name().as_ref() == b"t";
            }
            Event::Empty(tag) => each(XmlPart::Empty(tag.local_name().as_ref()))?,
            Event::End(tag) => {
                in_text = false;
                each(XmlPart::End(tag.local_name().as_ref()))?;
            }
            Event::Text(raw) if in_text => {
                each(XmlPart::Text(&raw.decode().map_err(|_| unreadable())?))?;
            }
            Event::GeneralRef(entity) if in_text => {
                let name = entity.decode().map_err(|_| unreadable())?;
                let resolved = match entity.resolve_char_ref().map_err(|_| unreadable())? {
                    Some(ch) => ch.to_string(),
                    None => quick_xml::escape::resolve_predefined_entity(&name)
                        .unwrap_or_default()
                        .to_string(),
                };
                each(XmlPart::Text(&resolved))?;
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

enum XmlPart<'a> {
    Empty(&'a [u8]),
    End(&'a [u8]),
    Text(&'a str),
}

/// Word と PowerPoint の段落 (`p`) を1行ずつ。タブ (`tab`) と改行 (`br`) も写す。
fn paragraphs(xml: &str, text: &mut Text) -> Result<(), AppError> {
    text_events(xml, |part| match part {
        XmlPart::Text(chars) => text.push(chars),
        XmlPart::Empty(b"tab") => text.push("\t"),
        XmlPart::Empty(b"br") => text.push("\n"),
        XmlPart::End(b"p") => text.push("\n"),
        _ => Ok(()),
    })
}

/// Excel の共有の文字列 (`sharedStrings.xml` の `si`)。セルは番号でこれを指す。無いブックもある。
fn shared_strings<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
) -> Result<Vec<String>, AppError> {
    if zip.by_name("xl/sharedStrings.xml").is_err() {
        return Ok(Vec::new());
    }
    let xml = read_xml(zip, "xl/sharedStrings.xml")?;
    let mut strings = Vec::new();
    let mut current = String::new();
    text_events(&xml, |part| {
        match part {
            XmlPart::Text(chars) => current.push_str(chars),
            XmlPart::End(b"si") => strings.push(std::mem::take(&mut current)),
            _ => {}
        }
        Ok(())
    })?;
    Ok(strings)
}

/// Excel のシートの行 (`row`) ごとに、値のあるセルをタブで区切る。
/// 共有の文字列 (`t="s"`) は番号を引き、そのほかは値 (`v`) か、セルに直に書いた文字列 (`is`) をそのまま出す。
fn cells(xml: &str, shared: &[String], text: &mut Text) -> Result<(), AppError> {
    let mut reader = Reader::from_str(xml);
    let mut row: Vec<String> = Vec::new();
    let mut shared_cell = false;
    let mut value: Option<String> = None;
    let mut in_value = false;
    loop {
        match reader.read_event().map_err(|_| unreadable())? {
            Event::Start(tag) => match tag.local_name().as_ref() {
                b"c" => {
                    shared_cell = tag
                        .try_get_attribute("t")
                        .ok()
                        .flatten()
                        .is_some_and(|kind| kind.value.as_ref() == b"s");
                    value = None;
                }
                b"v" | b"t" => in_value = true,
                _ => {}
            },
            Event::Text(raw) if in_value => {
                value
                    .get_or_insert_default()
                    .push_str(&raw.decode().map_err(|_| unreadable())?);
            }
            Event::GeneralRef(entity) if in_value => {
                let name = entity.decode().map_err(|_| unreadable())?;
                let resolved = match entity.resolve_char_ref().map_err(|_| unreadable())? {
                    Some(ch) => ch.to_string(),
                    None => quick_xml::escape::resolve_predefined_entity(&name)
                        .unwrap_or_default()
                        .to_string(),
                };
                value.get_or_insert_default().push_str(&resolved);
            }
            Event::End(tag) => match tag.local_name().as_ref() {
                b"v" | b"t" => in_value = false,
                b"c" => {
                    if let Some(raw) = value.take() {
                        let shown = if shared_cell {
                            raw.trim()
                                .parse::<usize>()
                                .ok()
                                .and_then(|index| shared.get(index))
                                .cloned()
                                .unwrap_or_default()
                        } else {
                            raw
                        };
                        row.push(shown);
                    }
                }
                b"row" => {
                    if row.iter().any(|cell| !cell.is_empty()) {
                        text.push(&row.join("\t"))?;
                        text.push("\n")?;
                    }
                    row.clear();
                }
                _ => {}
            },
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    /// `parts` (名前と中身) を入れた zip。Office の文書の代わり。
    fn office(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in parts {
            zip.start_file(*name, SimpleFileOptions::default())
                .expect("zip に入れられなかった");
            zip.write_all(body.as_bytes()).expect("zip に書けなかった");
        }
        zip.finish().expect("zip を閉じられなかった").into_inner()
    }

    fn text_of(name: &str, data: &[u8]) -> String {
        match convert(name, None, Some(data)).expect("変換できなかった") {
            Converted::Text(text) => text,
            other => panic!("文字にならなかった: {other:?}"),
        }
    }

    #[test]
    fn pdf_and_supported_images_are_attached_as_base64() {
        let Converted::Attachment(file) =
            convert("Report.PDF", None, Some(b"%PDF")).expect("変換できなかった")
        else {
            panic!("添付にならなかった");
        };
        assert_eq!(file.mime_type, "application/pdf");
        assert_eq!(file.data, "JVBERg==");
        assert!(matches!(
            convert("photo.jpeg", None, Some(b"x")),
            Ok(Converted::Attachment(Attachment {
                mime_type: "image/jpeg",
                ..
            }))
        ));
    }

    #[test]
    fn files_without_data_or_readable_text_send_only_the_name() {
        assert_eq!(
            convert("movie.mp4", None, None).ok(),
            Some(Converted::NameOnly)
        );
        assert_eq!(
            convert("photo.heic", Some("image/heic"), Some(b"\0\0binary")).ok(),
            Some(Converted::NameOnly)
        );
    }

    #[test]
    fn the_type_decides_when_the_name_has_no_extension() {
        assert!(matches!(
            convert("運動会の写真", Some("image/webp"), Some(b"x")),
            Ok(Converted::Attachment(Attachment {
                mime_type: "image/webp",
                ..
            }))
        ));
    }

    #[test]
    fn text_is_read_as_utf8_or_shift_jis() {
        assert_eq!(
            text_of("memo.txt", "\u{feff}こんにちは".as_bytes()),
            "こんにちは"
        );
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("お知らせ");
        assert_eq!(text_of("old.csv", &sjis), "お知らせ");
    }

    #[test]
    fn word_paragraphs_become_lines_with_entities_resolved() {
        let docx = office(&[(
            "word/document.xml",
            r#"<w:document xmlns:w="w"><w:body>
                <w:p><w:r><w:t>持ち物 &amp; 服装</w:t></w:r></w:p>
                <w:p><w:r><w:t>水筒</w:t><w:tab/><w:t xml:space="preserve">帽子</w:t></w:r></w:p>
            </w:body></w:document>"#,
        )]);
        assert_eq!(text_of("たより.docx", &docx), "持ち物 & 服装\n水筒\t帽子");
    }

    #[test]
    fn slides_are_read_in_number_order() {
        let pptx = office(&[
            (
                "ppt/slides/slide10.xml",
                r#"<p:sld xmlns:a="a" xmlns:p="p"><a:p><a:r><a:t>最後</a:t></a:r></a:p></p:sld>"#,
            ),
            (
                "ppt/slides/slide2.xml",
                r#"<p:sld xmlns:a="a" xmlns:p="p"><a:p><a:r><a:t>はじめ</a:t></a:r></a:p></p:sld>"#,
            ),
        ]);
        assert_eq!(
            text_of("説明会.pptx", &pptx),
            "## Slide 2\nはじめ\n\n## Slide 10\n最後"
        );
    }

    #[test]
    fn sheet_rows_join_shared_and_direct_values_with_tabs() {
        let xlsx = office(&[
            (
                "xl/sharedStrings.xml",
                r#"<sst><si><t>日付</t></si><si><r><t>行</t></r><r><t>事</t></r></si></sst>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData>
                    <row><c t="s"><v>0</v></c><c t="s"><v>1</v></c></row>
                    <row><c><v>46000</v></c><c t="inlineStr"><is><t>運動会</t></is></c></row>
                    <row><c t="s"></c></row>
                </sheetData></worksheet>"#,
            ),
        ]);
        assert_eq!(
            text_of("予定.xlsx", &xlsx),
            "## Sheet 1\n日付\t行事\n46000\t運動会"
        );
    }

    #[test]
    fn text_over_the_relay_limit_is_refused() {
        let long = "あ".repeat(MAX_INPUT_UNITS + 1);
        let docx = office(&[(
            "word/document.xml",
            &format!("<w:document><w:p><w:t>{long}</w:t></w:p></w:document>"),
        )]);
        assert!(matches!(
            convert("長い.docx", None, Some(&docx)),
            Err(AppError::AiTooLarge)
        ));
    }

    #[test]
    fn a_broken_office_file_is_a_validation_error() {
        assert!(matches!(
            convert("broken.xlsx", None, Some(b"not a zip")),
            Err(AppError::Validation(_))
        ));
    }
}
