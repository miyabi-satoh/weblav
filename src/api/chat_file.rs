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

/// Office の文書の XML を、全部を合わせて展開して読む上限。小さな zip が展開で膨らむもの (zip bomb) や、
/// 中身の無い XML をたくさん持つもので、メモリと時間を使い切らないため。
/// 文字は 20 万字までしか送らないので、ふつうの文書はこれに届かない。
const MAX_UNZIPPED_BYTES: u64 = 64 * 1024 * 1024;

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
/// 改行とタブのほかの制御文字は除く。読む役に立たず、窓口へ送る JSON では1文字が6バイトに膨らむため。
fn decode_text(data: &[u8]) -> String {
    let text = match std::str::from_utf8(data) {
        Ok(text) => std::borrow::Cow::Borrowed(text.strip_prefix('\u{feff}').unwrap_or(text)),
        Err(_) => encoding_rs::SHIFT_JIS.decode(data).0,
    };
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
        .collect()
}

/// Office の文書 (Office Open XML) の文字。Word は段落ごと、PowerPoint はスライドごと、
/// Excel はシートの行ごとにセルをタブで区切る。窓口の上限を超えたら、そこで読むのをやめて `AiTooLarge`。
fn office_text(ext: &str, data: &[u8]) -> Result<String, AppError> {
    let mut book = Book {
        zip: ZipArchive::new(Cursor::new(data)).map_err(|_| unreadable())?,
        left: MAX_UNZIPPED_BYTES,
    };
    let mut text = Text::default();
    if ext == "docx" {
        paragraphs(&book.read("word/document.xml")?, &mut text)?;
    } else if ext == "pptx" {
        for (number, part) in numbered_parts(&book.zip, "ppt/slides/slide") {
            text.heading(&format!("Slide {number}"))?;
            paragraphs(&book.read(&part)?, &mut text)?;
        }
    } else {
        let shared = shared_strings(&mut book)?;
        let sheets = sheets(&mut book)?;
        // シートが1つも無いのは壊れたブック。空の文字を送って、読めたように見せない。
        if sheets.is_empty() {
            return Err(unreadable());
        }
        for (name, part) in sheets {
            text.heading(&name)?;
            cells(&book.read(&part)?, &shared, &mut text)?;
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

/// 開いた Office の文書。展開した大きさを、全部の XML を合わせて `MAX_UNZIPPED_BYTES` までに抑える。
struct Book<'a> {
    zip: ZipArchive<Cursor<&'a [u8]>>,
    left: u64,
}

impl Book<'_> {
    fn has(&mut self, part: &str) -> bool {
        self.zip.by_name(part).is_ok()
    }

    /// `part` の XML。展開の残りを超えるものは、途中で切って読み違えるより断る (`AiTooLarge`)。
    fn read(&mut self, part: &str) -> Result<String, AppError> {
        let file = self.zip.by_name(part).map_err(|_| unreadable())?;
        let mut xml = Vec::new();
        file.take(self.left + 1)
            .read_to_end(&mut xml)
            .map_err(|_| unreadable())?;
        let size = u64::try_from(xml.len()).unwrap_or(u64::MAX);
        if size > self.left {
            return Err(AppError::AiTooLarge);
        }
        self.left -= size;
        String::from_utf8(xml).map_err(|_| unreadable())
    }
}

/// `prefix` に続く番号の付いた XML (`slide1.xml`・`sheet2.xml` …) を、番号の順に。
fn numbered_parts(zip: &ZipArchive<Cursor<&[u8]>>, prefix: &str) -> Vec<(u32, String)> {
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

/// XML を読み、`each` に渡す。読まない要素の中 (`skip`) は飛ばす。
fn xml_events(
    xml: &str,
    skip: &[&[u8]],
    mut each: impl FnMut(XmlPart<'_>) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let mut reader = Reader::from_str(xml);
    let mut skipping = 0usize;
    loop {
        match reader.read_event().map_err(|_| unreadable())? {
            Event::Start(tag) if skipping > 0 || skip.contains(&tag.local_name().as_ref()) => {
                skipping += 1;
            }
            Event::End(_) if skipping > 0 => skipping -= 1,
            _ if skipping > 0 => {}
            Event::Start(tag) => each(XmlPart::Start(&tag))?,
            Event::Empty(tag) => {
                each(XmlPart::Start(&tag))?;
                each(XmlPart::End(tag.local_name().as_ref()))?;
            }
            Event::End(tag) => each(XmlPart::End(tag.local_name().as_ref()))?,
            Event::Text(raw) => each(XmlPart::Text(&raw.decode().map_err(|_| unreadable())?))?,
            Event::GeneralRef(entity) => {
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
    Start(&'a quick_xml::events::BytesStart<'a>),
    End(&'a [u8]),
    Text(&'a str),
}

/// 要素の属性の値。名前は接頭辞を除いて比べる (`r:id` は `id`)。
fn attribute(tag: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    tag.attributes()
        .flatten()
        .find(|attr| attr.key.local_name().as_ref() == name)
        .and_then(|attr| {
            attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned())
        })
}

/// 同じ中身を2つ持つ所 (`mc:AlternateContent` の古い版向けの写し) は、片方だけを読む。
const DUPLICATE_COPIES: &[u8] = b"Fallback";

/// Word と PowerPoint の段落 (`p`) を1行ずつ。文字は `t` の中だけを読み、タブ (`tab`) と改行 (`br`) も写す。
fn paragraphs(xml: &str, text: &mut Text) -> Result<(), AppError> {
    let mut in_text = false;
    xml_events(xml, &[DUPLICATE_COPIES], |part| match part {
        XmlPart::Start(tag) => {
            match tag.local_name().as_ref() {
                b"t" => in_text = true,
                b"tab" => text.push("\t")?,
                b"br" => text.push("\n")?,
                _ => {}
            }
            Ok(())
        }
        XmlPart::End(b"t") => {
            in_text = false;
            Ok(())
        }
        XmlPart::End(b"p") => text.push("\n"),
        XmlPart::Text(chars) if in_text => text.push(chars),
        _ => Ok(()),
    })
}

/// Excel の共有の文字列 (`sharedStrings.xml` の `si`)。セルは番号でこれを指す。無いブックもある。
/// ふりがな (`rPh`) は、元の文字に続けて読むと別の言葉になるので読まない。
fn shared_strings(book: &mut Book<'_>) -> Result<Vec<String>, AppError> {
    if !book.has("xl/sharedStrings.xml") {
        return Ok(Vec::new());
    }
    let xml = book.read("xl/sharedStrings.xml")?;
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    xml_events(&xml, &[b"rPh", DUPLICATE_COPIES], |part| {
        match part {
            XmlPart::Start(tag) if tag.local_name().as_ref() == b"t" => in_text = true,
            XmlPart::End(b"t") => in_text = false,
            XmlPart::Text(chars) if in_text => current.push_str(chars),
            XmlPart::End(b"si") => strings.push(std::mem::take(&mut current)),
            _ => {}
        }
        Ok(())
    })?;
    Ok(strings)
}

/// シートの名前と XML を、ブックに並ぶ順に (`workbook.xml` と、その関係の `workbook.xml.rels`)。
/// 読めなければ、番号の付いた XML を「Sheet 番号」の名前で並べる。
fn sheets(book: &mut Book<'_>) -> Result<Vec<(String, String)>, AppError> {
    let listed = (|| -> Option<Vec<(String, String)>> {
        let mut targets = std::collections::HashMap::new();
        let rels = book.read("xl/_rels/workbook.xml.rels").ok()?;
        xml_events(&rels, &[], |part| {
            if let XmlPart::Start(tag) = part
                && let (Some(id), Some(target)) = (attribute(tag, b"Id"), attribute(tag, b"Target"))
            {
                // 関係の先は `xl/` からの相対か、`/` で始まる絶対の場所。
                let path = target
                    .strip_prefix('/')
                    .map_or_else(|| format!("xl/{target}"), str::to_string);
                targets.insert(id, path);
            }
            Ok(())
        })
        .ok()?;
        let workbook = book.read("xl/workbook.xml").ok()?;
        let mut sheets = Vec::new();
        xml_events(&workbook, &[], |part| {
            if let XmlPart::Start(tag) = part
                && tag.local_name().as_ref() == b"sheet"
                && let (Some(name), Some(id)) = (attribute(tag, b"name"), attribute(tag, b"id"))
                && let Some(path) = targets.get(&id)
            {
                sheets.push((name, path.clone()));
            }
            Ok(())
        })
        .ok()?;
        (!sheets.is_empty()).then_some(sheets)
    })();
    if let Some(sheets) = listed {
        return Ok(sheets);
    }
    Ok(numbered_parts(&book.zip, "xl/worksheets/sheet")
        .into_iter()
        .map(|(number, part)| (format!("Sheet {number}"), part))
        .collect())
}

/// セルの場所 (`C12`) の列の番号 (A が 0)。
fn column_of(reference: &str) -> Option<usize> {
    let letters: Vec<u8> = reference
        .bytes()
        .take_while(u8::is_ascii_alphabetic)
        .collect();
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    Some(
        letters.iter().fold(0usize, |column, letter| {
            column * 26 + usize::from(letter.to_ascii_uppercase() - b'A' + 1)
        }) - 1,
    )
}

/// Excel のシートの行 (`row`) ごとに、セルをタブで区切る。空のセルも列を数え、見出しと値の列がずれないようにする。
/// 共有の文字列 (`t="s"`) は番号を引き、そのほかは値 (`v`) か、セルに直に書いた文字列 (`is`) をそのまま出す。
fn cells(xml: &str, shared: &[String], text: &mut Text) -> Result<(), AppError> {
    let mut row: Vec<String> = Vec::new();
    let mut column = 0usize;
    let mut shared_cell = false;
    let mut value: Option<String> = None;
    let mut in_value = false;
    xml_events(xml, &[b"rPh", DUPLICATE_COPIES], |part| {
        match part {
            XmlPart::Start(tag) => match tag.local_name().as_ref() {
                b"c" => {
                    shared_cell = attribute(tag, b"t").as_deref() == Some("s");
                    column = attribute(tag, b"r")
                        .as_deref()
                        .and_then(column_of)
                        .unwrap_or(row.len());
                    value = None;
                }
                b"v" | b"t" => in_value = true,
                _ => {}
            },
            XmlPart::Text(chars) if in_value => value.get_or_insert_default().push_str(chars),
            XmlPart::End(b"v" | b"t") => in_value = false,
            XmlPart::End(b"c") => {
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
                    // 飛んだ列は空で埋める。場所が前に戻るセルは、壊れた並びとして後ろに足す。
                    if column >= row.len() {
                        row.resize(column, String::new());
                    }
                    row.push(shown);
                }
            }
            XmlPart::End(b"row") => {
                while row.last().is_some_and(String::is_empty) {
                    row.pop();
                }
                if !row.is_empty() {
                    text.push(&row.join("\t"))?;
                    text.push("\n")?;
                }
                row.clear();
            }
            _ => {}
        }
        Ok(())
    })
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
        assert_eq!(text_of("log.txt", b"a\x1b[0m\tb\r\n"), "a[0m\tb\r\n");
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
    fn sheets_follow_the_workbook_order_and_keep_columns_without_furigana() {
        let xlsx = office(&[
            (
                "xl/workbook.xml",
                r#"<workbook xmlns:r="r"><sheets>
                    <sheet name="予定" sheetId="2" r:id="rId2"/>
                    <sheet name="名簿" sheetId="1" r:id="rId1"/>
                </sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships>
                    <Relationship Id="rId1" Target="worksheets/sheet1.xml"/>
                    <Relationship Id="rId2" Target="/xl/worksheets/sheet2.xml"/>
                </Relationships>"#,
            ),
            (
                "xl/sharedStrings.xml",
                r#"<sst><si><t>漢字</t><rPh sb="0" eb="2"><t>カンジ</t></rPh></si></sst>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row><c r="A1" t="s"><v>0</v></c></row></sheetData></worksheet>"#,
            ),
            (
                "xl/worksheets/sheet2.xml",
                r#"<worksheet><sheetData><row><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c></row></sheetData></worksheet>"#,
            ),
        ]);
        assert_eq!(
            text_of("ブック.xlsx", &xlsx),
            "## 予定\n1\t\t3\n\n## 名簿\n漢字"
        );
    }

    #[test]
    fn column_of_reads_the_letters_of_a_cell_reference() {
        assert_eq!(column_of("A1"), Some(0));
        assert_eq!(column_of("c12"), Some(2));
        assert_eq!(column_of("AA3"), Some(26));
        assert_eq!(column_of("12"), None);
    }

    #[test]
    fn a_word_text_box_is_read_once_without_its_fallback_copy() {
        let docx = office(&[(
            "word/document.xml",
            r#"<w:document xmlns:w="w" xmlns:mc="mc"><w:p><mc:AlternateContent>
                <mc:Choice><w:t>枠の文字</w:t></mc:Choice>
                <mc:Fallback><w:t>枠の文字</w:t></mc:Fallback>
            </mc:AlternateContent></w:p></w:document>"#,
        )]);
        assert_eq!(text_of("たより.docx", &docx), "枠の文字");
    }

    #[test]
    fn office_parts_beyond_the_unzipped_budget_are_refused() {
        let filler =
            "<x/>".repeat(usize::try_from(MAX_UNZIPPED_BYTES).expect("収まる") / 4 / 2 + 1);
        let pptx = office(&[
            ("ppt/slides/slide1.xml", &filler),
            ("ppt/slides/slide2.xml", &filler),
        ]);
        assert!(matches!(
            convert("大きい.pptx", None, Some(&pptx)),
            Err(AppError::AiTooLarge)
        ));
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
    fn a_workbook_without_sheets_is_a_validation_error() {
        let xlsx = office(&[("xl/sharedStrings.xml", "<sst><si><t>a</t></si></sst>")]);
        assert!(matches!(
            convert("空.xlsx", None, Some(&xlsx)),
            Err(AppError::Validation(_))
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
