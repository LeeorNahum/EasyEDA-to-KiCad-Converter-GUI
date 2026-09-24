//! Checks that written symbol libraries and footprints are well-formed
//! S-expressions, with a strict reader that rejects anything KiCad's reader
//! would not accept: unbalanced parentheses, unterminated strings, text
//! outside the one top-level list, and lists that do not start with a
//! lowercase keyword.
//!
//! `live_conversion_is_well_formed` converts real parts from EasyEDA and
//! needs the network, so it only runs when asked for:
//! `cargo test --test wellformed -- --ignored`. The folder check reads the
//! folder named by `WELLFORMED_DIR`.

use std::fs;
use std::path::{Path, PathBuf};

use easyeda_to_kicad_converter::convert::{self, LibraryMode, Request};
use easyeda_to_kicad_converter::easyeda::api::Client;

#[derive(Debug)]
enum Node {
    Atom,
    Text,
    List,
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn skip_space(&mut self) {
        while self.at < self.text.len() && self.text[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn node(&mut self) -> Result<Node, String> {
        self.skip_space();
        match self.text.get(self.at) {
            None => Err("unexpected end of file".into()),
            Some(b'(') => {
                let start = self.at;
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_space();
                    match self.text.get(self.at) {
                        None => return Err(format!("list opened at byte {start} is never closed")),
                        Some(b')') => {
                            self.at += 1;
                            break;
                        }
                        Some(_) => items.push(self.node()?),
                    }
                }
                match items.first() {
                    Some(Node::Atom) => {}
                    _ => {
                        return Err(format!(
                            "list at byte {start} does not start with a keyword"
                        ));
                    }
                }
                let keyword_end = self.text[start + 1..]
                    .iter()
                    .position(|c| c.is_ascii_whitespace() || *c == b')')
                    .map_or(self.text.len(), |p| start + 1 + p);
                let keyword = &self.text[start + 1..keyword_end];
                if !keyword
                    .iter()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
                {
                    return Err(format!(
                        "list at byte {start} starts with {:?}, not a keyword",
                        String::from_utf8_lossy(keyword)
                    ));
                }
                Ok(Node::List)
            }
            Some(b')') => Err(format!("unmatched ')' at byte {}", self.at)),
            Some(b'"') => {
                let start = self.at;
                self.at += 1;
                loop {
                    match self.text.get(self.at) {
                        None => return Err(format!("string at byte {start} is never closed")),
                        Some(b'\\') => {
                            match self.text.get(self.at + 1) {
                                Some(b'\\' | b'"' | b'n' | b'r' | b't') => {}
                                other => {
                                    return Err(format!(
                                        "unknown escape {other:?} at byte {}",
                                        self.at
                                    ));
                                }
                            }
                            self.at += 2;
                        }
                        Some(b'"') => {
                            self.at += 1;
                            return Ok(Node::Text);
                        }
                        Some(b'\n') => {
                            return Err(format!("string at byte {start} runs across a line"));
                        }
                        Some(_) => self.at += 1,
                    }
                }
            }
            Some(_) => {
                while let Some(c) = self.text.get(self.at) {
                    if c.is_ascii_whitespace() || matches!(c, b'(' | b')') {
                        break;
                    }
                    if *c == b'"' {
                        return Err(format!("quote inside a bare word at byte {}", self.at));
                    }
                    self.at += 1;
                }
                Ok(Node::Atom)
            }
        }
    }
}

/// Reads a whole file as one top-level list and returns its keyword.
fn read(text: &str) -> Result<String, String> {
    let mut reader = Reader {
        text: text.as_bytes(),
        at: 0,
    };
    let root = reader.node()?;
    reader.skip_space();
    if reader.at != text.len() {
        return Err(format!(
            "text after the top-level list at byte {}",
            reader.at
        ));
    }
    let Node::List = root else {
        return Err("the file is not a list".into());
    };
    let keyword: String = text
        .trim_start()
        .trim_start_matches('(')
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    Ok(keyword)
}

fn check_file(path: &Path) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let keyword = read(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let expected = match path.extension().and_then(|e| e.to_str()) {
        Some("kicad_sym") => "kicad_symbol_lib",
        Some("kicad_mod") => "module",
        _ => return Ok(()),
    };
    if keyword != expected {
        return Err(format!(
            "{}: starts with {keyword}, expected {expected}",
            path.display()
        ));
    }
    Ok(())
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("kicad_sym" | "kicad_mod")
            ) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn the_reader_is_strict() {
    assert!(read("(kicad_symbol_lib (version 1) (name \"a \\\"b\\\"\"))").is_ok());
    assert!(read("(module x (pad 1 smd rect))\n").is_ok());
    assert!(
        read("(module (\"x\"))").is_err(),
        "a list without a keyword"
    );
    assert!(read("(module x").is_err(), "an unclosed list");
    assert!(read("(module x))").is_err(), "an extra parenthesis");
    assert!(read("(module \"x)").is_err(), "an unclosed string");
    assert!(
        read("(module x) (module y)").is_err(),
        "two top-level lists"
    );
    assert!(read("(module a\"b)").is_err(), "a quote inside a word");
    assert!(
        read("(Module x)").is_err(),
        "a keyword that is not lowercase"
    );
}

#[test]
#[ignore = "reads the folder named by WELLFORMED_DIR"]
fn produced_files_are_well_formed() {
    let root = PathBuf::from(std::env::var("WELLFORMED_DIR").expect("set WELLFORMED_DIR"));
    let files = files_under(&root);
    assert!(
        !files.is_empty(),
        "no .kicad_sym or .kicad_mod files under {}",
        root.display()
    );
    let failures: Vec<String> = files.iter().filter_map(|f| check_file(f).err()).collect();
    println!("checked {} files", files.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[tokio::test]
#[ignore = "converts real parts from EasyEDA over the network"]
async fn live_conversion_is_well_formed() {
    let output = std::env::temp_dir().join(format!("easyeda-to-kicad-live-{}", std::process::id()));
    fs::create_dir_all(&output).unwrap();
    let client = Client::new();
    for part in ["C25804", "C2040"] {
        let component = client
            .component(part)
            .await
            .expect("EasyEDA returns the part");
        let request = Request {
            lcsc_id: part.to_string(),
            output_folder: output.to_string_lossy().into_owned(),
            mode: LibraryMode::CustomLibrary,
            library_name: "Live".to_string(),
            symbol: true,
            footprint: true,
            model: true,
            overwrite: true,
            project_relative: true,
        };
        let report = convert::convert(&request, &component, &client)
            .await
            .expect("the part converts");
        assert!(
            report.written.iter().any(|p| p.ends_with(".wrl")),
            "{part} has a WRL model"
        );
        for path in &report.written {
            let path = Path::new(path);
            match path.extension().and_then(|e| e.to_str()) {
                Some("step") => assert!(fs::read(path).unwrap().starts_with(b"ISO-10303-21;")),
                Some("wrl") => assert!(
                    fs::read_to_string(path)
                        .unwrap()
                        .starts_with("#VRML V2.0 utf8\n")
                ),
                _ => check_file(path).unwrap(),
            }
        }
    }
    fs::remove_dir_all(&output).ok();
}
