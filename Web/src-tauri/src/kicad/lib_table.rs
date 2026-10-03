//! Reads a KiCad project's `sym-lib-table` or `fp-lib-table` and adds a
//! library to it.
//!
//! The reader accepts exactly what KiCad 10's own library table grammar
//! accepts (`include/libraries/library_table_grammar.h`), so a table it
//! refuses is one KiCad would refuse too. A row is added by inserting one
//! line before the table's closing parenthesis, which leaves every existing
//! byte of the table as it was. The row is written the way KiCad writes one.

/// Which library table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Symbol,
    Footprint,
}

impl Kind {
    /// The table's file name in the project folder.
    pub fn file_name(self) -> &'static str {
        match self {
            Kind::Symbol => "sym-lib-table",
            Kind::Footprint => "fp-lib-table",
        }
    }

    fn keyword(self) -> &'static str {
        match self {
            Kind::Symbol => "sym_lib_table",
            Kind::Footprint => "fp_lib_table",
        }
    }
}

/// One library in a table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    pub name: Option<String>,
    pub uri: Option<String>,
    /// The library's format. This app's libraries are `KiCad`.
    pub format: Option<String>,
    /// Turned off with the Enable checkbox: KiCad does not load it.
    pub disabled: bool,
    /// Turned off with the Show checkbox: KiCad loads it but does not list it.
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub rows: Vec<Row>,
    /// The byte position of the table's closing parenthesis.
    close: usize,
}

/// The characters PEGTL's `space` rule matches.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn spaces(&mut self) -> usize {
        let start = self.at;
        while self.text.get(self.at).copied().is_some_and(is_space) {
            self.at += 1;
        }
        self.at - start
    }

    fn literal(&mut self, word: &str) -> bool {
        if self.text[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            true
        } else {
            false
        }
    }

    /// `QUOTED_TEXT` or `TOKEN`. A quoted value has no escapes: it ends at
    /// the next quote.
    fn value(&mut self) -> Option<String> {
        let start = self.at;
        if self.text.get(self.at) == Some(&b'"') {
            let length = self.text[start + 1..].iter().position(|&c| c == b'"')?;
            self.at = start + 1 + length + 1;
            return Some(
                String::from_utf8_lossy(&self.text[start + 1..start + 1 + length]).into_owned(),
            );
        }
        while self
            .text
            .get(self.at)
            .is_some_and(|c| !matches!(c, b'(' | b')' | b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.at += 1;
        }
        (self.at > start).then(|| String::from_utf8_lossy(&self.text[start..self.at]).into_owned())
    }

    /// `(key value)` with no space inside either parenthesis.
    fn property(&mut self, key: &str) -> Option<String> {
        let start = self.at;
        let found = (|| {
            if !self.literal("(") || !self.literal(key) || self.spaces() == 0 {
                return None;
            }
            let value = self.value()?;
            self.literal(")").then_some(value)
        })();
        if found.is_none() {
            self.at = start;
        }
        found
    }

    fn marker(&mut self, word: &str) -> bool {
        let start = self.at;
        if self.literal("(") && self.literal(word) && self.literal(")") {
            true
        } else {
            self.at = start;
            false
        }
    }

    /// One `LIB_ROW_MEMBER` into `row`, or `false` when none is here.
    fn member(&mut self, row: &mut Row) -> bool {
        if let Some(name) = self.property("name") {
            row.name = Some(name);
        } else if let Some(uri) = self.property("uri") {
            row.uri = Some(uri);
        } else if let Some(format) = self.property("type") {
            row.format = Some(format);
        } else if self.marker("hidden") {
            row.hidden = true;
        } else if self.marker("disabled") {
            row.disabled = true;
        } else {
            return ["options", "descr"]
                .iter()
                .any(|key| self.property(key).is_some());
        }
        true
    }

    /// The rest of a `LIB_ROW` after its opening parenthesis.
    fn row(&mut self) -> Option<Row> {
        self.spaces();
        if !self.literal("lib") || self.spaces() == 0 {
            return None;
        }
        let mut row = Row::default();
        if !self.member(&mut row) {
            return None;
        }
        loop {
            self.spaces();
            if !self.member(&mut row) {
                break;
            }
        }
        self.literal(")").then_some(row)
    }
}

/// The libraries in a table of this kind, or `None` when the text is not a
/// table of this kind that KiCad can read.
pub fn read(text: &str, kind: Kind) -> Option<Table> {
    let mut reader = Reader {
        text: text.as_bytes(),
        at: 0,
    };
    if !reader.literal("(") || !reader.literal(kind.keyword()) {
        return None;
    }
    reader.spaces();
    if reader.property("version").is_some() {
        reader.spaces();
    }
    let mut rows = Vec::new();
    loop {
        reader.spaces();
        if !reader.literal("(") {
            break;
        }
        rows.push(reader.row()?);
    }
    let close = reader.at;
    if !reader.literal(")") {
        return None;
    }
    reader.spaces();
    (reader.at == text.len()).then_some(Table { rows, close })
}

/// A library row as KiCad writes it. Neither value may hold a quote: KiCad's
/// reader ends a value at the next quote, and Windows allows none in a file
/// or folder name.
fn row_text(nickname: &str, uri: &str) -> String {
    format!(
        "(lib (name \"{nickname}\") (type \"KiCad\") (uri \"{uri}\") (options \"\") (descr \"\"))"
    )
}

/// A new table holding one library, laid out as KiCad lays one out.
pub fn new_table(kind: Kind, nickname: &str, uri: &str) -> String {
    format!(
        "({}\n\t(version 7)\n\t{}\n)\n",
        kind.keyword(),
        row_text(nickname, uri)
    )
}

/// `text`, read as `table`, with one more library. The new line uses the
/// line endings the table already has.
pub fn with_row(text: &str, table: &Table, nickname: &str, uri: &str) -> String {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let row = format!("\t{}{newline}", row_text(nickname, uri));
    let before = &text[..table.close];
    let line_start = before.rfind('\n').map_or(0, |at| at + 1);
    let (at, row) = if before[line_start..]
        .bytes()
        .all(|c| c == b' ' || c == b'\t')
    {
        // The closing parenthesis is on a line of its own.
        (line_start, row)
    } else {
        (table.close, format!("{newline}{row}"))
    };
    format!("{}{row}{}", &text[..at], &text[at..])
}

#[cfg(test)]
mod tests {
    use super::*;

    const KICAD: &str = "(sym_lib_table\n\t(version 7)\n\t(lib (name \"Part_C1\") (type \"KiCad\") (uri \"${KIPRJMOD}/lib/Part_C1/Part_C1.kicad_sym\") (options \"\") (descr \"\"))\n)\n";

    #[test]
    fn tables_kicad_writes_are_read() {
        let table = read(KICAD, Kind::Symbol).unwrap();
        assert_eq!(
            table.rows,
            [Row {
                name: Some("Part_C1".into()),
                uri: Some("${KIPRJMOD}/lib/Part_C1/Part_C1.kicad_sym".into()),
                format: Some("KiCad".into()),
                disabled: false,
                hidden: false,
            }]
        );
        assert!(read(KICAD, Kind::Footprint).is_none(), "the other kind");
        let crlf = KICAD.replace('\n', "\r\n");
        assert_eq!(read(&crlf, Kind::Symbol).unwrap().rows, table.rows);
        let flags = "(fp_lib_table (lib (name a)(uri b)(disabled)(hidden)))";
        let row = &read(flags, Kind::Footprint).unwrap().rows[0];
        assert!(row.disabled && row.hidden);
        assert_eq!(row.name.as_deref(), Some("a"));
        assert!(
            read("(fp_lib_table)", Kind::Footprint)
                .unwrap()
                .rows
                .is_empty()
        );
    }

    #[test]
    fn tables_kicad_would_refuse_are_refused() {
        for text in [
            "",
            " (sym_lib_table)",
            "(sym_lib_table",
            "(sym_lib_table (version 7)",
            "(sym_lib_table (lib))",
            "(sym_lib_table (lib(name a)))",
            "(sym_lib_table (lib (name a) (colour b)))",
            "(sym_lib_table (lib ( name a)))",
            "(sym_lib_table (lib (name \"a)))",
            "(sym_lib_table (other (name a)))",
            "(sym_lib_table (lib (name a)) (version 7))",
            "(sym_lib_table) x",
            "(sym_lib_table))",
        ] {
            assert!(read(text, Kind::Symbol).is_none(), "{text:?}");
        }
    }

    #[test]
    fn a_row_is_added_without_touching_the_rest() {
        let uri = "${KIPRJMOD}/lib/Part_C2/Part_C2.kicad_sym";
        let added = with_row(KICAD, &read(KICAD, Kind::Symbol).unwrap(), "Part_C2", uri);
        assert_eq!(
            added,
            KICAD.replace(
                "\n)\n",
                "\n\t(lib (name \"Part_C2\") (type \"KiCad\") (uri \"${KIPRJMOD}/lib/Part_C2/Part_C2.kicad_sym\") (options \"\") (descr \"\"))\n)\n"
            )
        );
        assert_eq!(read(&added, Kind::Symbol).unwrap().rows.len(), 2);

        let crlf = KICAD.replace('\n', "\r\n");
        let added = with_row(&crlf, &read(&crlf, Kind::Symbol).unwrap(), "Part_C2", uri);
        assert!(added.starts_with(&crlf[..crlf.len() - 3]));
        assert!(
            !added.replace("\r\n", "").contains('\n'),
            "only CRLF line endings"
        );

        let inline = "(sym_lib_table (version 7))";
        let added = with_row(inline, &read(inline, Kind::Symbol).unwrap(), "Part_C2", uri);
        assert!(added.starts_with("(sym_lib_table (version 7)\n\t(lib (name \"Part_C2\")"));
        assert!(added.ends_with("\"\"))\n)"));
        assert_eq!(read(&added, Kind::Symbol).unwrap().rows.len(), 1);
    }

    #[test]
    fn a_new_table_reads_back() {
        let text = new_table(
            Kind::Footprint,
            "Part_C1",
            "${KIPRJMOD}/Part_C1/Part_C1.pretty",
        );
        let table = read(&text, Kind::Footprint).unwrap();
        assert_eq!(table.rows[0].name.as_deref(), Some("Part_C1"));
        assert_eq!(
            table.rows[0].uri.as_deref(),
            Some("${KIPRJMOD}/Part_C1/Part_C1.pretty")
        );
    }
}
