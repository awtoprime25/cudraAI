use std::collections::HashMap;
use std::fmt;

use ariadne::{Cache, Color, Config, Label as AriadneLabel, Report, ReportKind, Source};
use cudra_span::{FileId, SourceMap, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
    Note,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub span: Span,
    pub message: String,
    pub primary: bool,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Diagnostic {
            level: Level::Error,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Diagnostic {
            level: Level::Warning,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn with_primary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
            primary: true,
        });
        self
    }

    pub fn with_secondary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
            primary: false,
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

struct FileCache {
    files: HashMap<u32, (String, Source<String>)>,
}

impl Cache<FileId> for FileCache {
    type Storage = String;

    fn fetch(&mut self, id: &FileId) -> Result<&Source<String>, Box<dyn fmt::Debug + '_>> {
        self.files.get(&id.0).map(|(_, src)| src).ok_or_else(|| {
            Box::new(format!("ficheiro desconhecido: {}", id.0)) as Box<dyn fmt::Debug>
        })
    }

    fn display<'a>(&self, id: &'a FileId) -> Option<Box<dyn fmt::Display + 'a>> {
        self.files
            .get(&id.0)
            .map(|(name, _)| Box::new(name.clone()) as Box<dyn fmt::Display>)
    }
}

fn build_cache(sources: &SourceMap, file_ids: impl IntoIterator<Item = FileId>) -> FileCache {
    let mut files = HashMap::new();
    for id in file_ids {
        files.entry(id.0).or_insert_with(|| {
            let name = sources.name(id).to_string();
            let src = Source::from(sources.source(id).to_string());
            (name, src)
        });
    }
    FileCache { files }
}

pub fn render(diag: &Diagnostic, sources: &SourceMap, color: bool) -> String {
    let (kind, header_color) = match diag.level {
        Level::Error => (ReportKind::Custom("erro", Color::Red), Color::Red),
        Level::Warning => (ReportKind::Custom("aviso", Color::Yellow), Color::Yellow),
        Level::Note => (ReportKind::Custom("nota", Color::Blue), Color::Blue),
    };

    let anchor = diag
        .labels
        .iter()
        .find(|l| l.primary)
        .or_else(|| diag.labels.first());

    let anchor_file = anchor.map(|l| l.span.file).unwrap_or(FileId(0));
    let anchor_offset = anchor.map(|l| l.span.lo as usize).unwrap_or(0);

    let mut builder = Report::build(kind, anchor_file, anchor_offset)
        .with_config(Config::default().with_color(color))
        .with_message(&diag.message);

    for label in &diag.labels {
        let range = label.span.lo as usize..label.span.hi as usize;
        let mut al = AriadneLabel::new((label.span.file, range)).with_message(&label.message);
        al = if label.primary {
            al.with_order(0).with_color(header_color)
        } else {
            al.with_order(1).with_color(Color::Blue)
        };
        builder = builder.with_label(al);
    }

    for note in &diag.notes {
        builder = builder.with_note(note);
    }

    let report = builder.finish();
    let file_ids = diag.labels.iter().map(|l| l.span.file).chain([anchor_file]);
    let mut cache = build_cache(sources, file_ids);

    let mut buf = Vec::new();
    report
        .write(&mut cache, &mut buf)
        .expect("escrita em memória não falha");
    let text = String::from_utf8(buf).expect("ariadne produz UTF-8 válido");

    // ReportKind::Custom não respeita Config::with_color para a cor do cabeçalho
    // (limitação do ariadne 0.4); garantimos saída sem ANSI aqui.
    if color {
        text
    } else {
        strip_ansi(&text)
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            let mut lookahead = chars.clone();
            if lookahead.next() == Some('[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_error_with_primary_and_secondary_labels() {
        let mut sources = SourceMap::new();
        let file = sources.add(
            "prog.cdr".to_string(),
            "x = 1\ny = x + verdadeiro\n".to_string(),
        );

        let diag = Diagnostic::error("tipos incompatíveis")
            .with_primary(Span::new(file, 14, 24), "aqui é `bool`")
            .with_secondary(Span::new(file, 0, 1), "`x` foi definida como `int` aqui")
            .with_note("promove um dos dois operandos para resolver");

        let output = render(&diag, &sources, false);
        insta::assert_snapshot!(output);
    }
}
