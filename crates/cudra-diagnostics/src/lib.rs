use std::collections::HashMap;
use std::fmt;

use ariadne::{Cache, Color, Config, IndexType, Label as AriadneLabel, Report, ReportKind, Source};
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
        // `render` filtra os labels de ficheiro desconhecido antes de chamar o
        // ariadne, por isso isto nunca devia acontecer; o ariadne escreveria
        // no stderr (em inglês) se acontecesse.
        debug_assert!(
            self.files.contains_key(&id.0),
            "ariadne pediu um ficheiro que devia ter sido filtrado: {}",
            id.0
        );
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
        if files.contains_key(&id.0) {
            continue;
        }
        // `render` só chama isto com ficheiros já confirmados no SourceMap.
        if let (Some(name), Some(src)) = (sources.get_name(id), sources.get_source(id)) {
            files.insert(id.0, (name.to_string(), Source::from(src.to_string())));
        }
    }
    FileCache { files }
}

fn level_style(level: Level) -> (&'static str, Color) {
    match level {
        Level::Error => ("erro", Color::Red),
        Level::Warning => ("aviso", Color::Yellow),
        Level::Note => ("nota", Color::Blue),
    }
}

fn append_notes(out: &mut String, notes: &[String]) {
    for note in notes {
        out.push_str("   = nota: ");
        out.push_str(note);
        out.push('\n');
    }
}

// O ariadne rebenta se um span cair a meio de um char UTF-8, e perde a
// localização se o span sair da fonte. Encosta `lo`/`hi` às fronteiras de
// char válidas mais próximas, sem nunca passar do fim do ficheiro.
fn clamp_span(span: Span, sources: &SourceMap) -> Span {
    let Some(src) = sources.get_source(span.file) else {
        return span;
    };
    let len = src.len() as u32;
    let mut lo = span.lo.min(len);
    let mut hi = span.hi.min(len);
    while lo > 0 && !src.is_char_boundary(lo as usize) {
        lo -= 1;
    }
    while hi < len && !src.is_char_boundary(hi as usize) {
        hi += 1;
    }
    if hi < lo {
        hi = lo;
    }
    Span::new(span.file, lo, hi)
}

// Insere `lines` logo a seguir à primeira linha (o cabeçalho) de `text`.
fn insert_after_header(text: &str, lines: &[String]) -> String {
    let mut out =
        String::with_capacity(text.len() + lines.iter().map(|l| l.len() + 1).sum::<usize>());
    match text.find('\n') {
        Some(pos) => {
            out.push_str(&text[..=pos]);
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str(&text[pos + 1..]);
        }
        None => {
            out.push_str(text);
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

pub fn render(diag: &Diagnostic, sources: &SourceMap, color: bool) -> String {
    let (header, header_color) = level_style(diag.level);

    // Nenhum label pode desaparecer sem aviso. Classificamos cada um:
    // - `drawable`: ficheiro conhecido e com conteúdo — o ariadne desenha-o;
    // - `empty_file_lines`: ficheiro conhecido mas vazio — vira
    //   uma linha `--> nome:l:c (mensagem)` logo depois do cabeçalho;
    // - `unresolved_notes`: `FileId` que não existe no SourceMap —
    //   nunca é passado ao ariadne, vira uma nota.
    let mut drawable: Vec<Label> = Vec::new();
    let mut empty_file_lines: Vec<String> = Vec::new();
    let mut unresolved_notes: Vec<String> = Vec::new();

    for l in &diag.labels {
        match sources.get_source(l.span.file) {
            Some(src) if !src.is_empty() => drawable.push(Label {
                span: clamp_span(l.span, sources),
                message: l.message.clone(),
                primary: l.primary,
            }),
            Some(_) => {
                let name = sources.get_name(l.span.file).unwrap_or("<desconhecido>");
                let (line, col) = sources
                    .get_line_col(l.span.file, l.span.lo)
                    .unwrap_or((1, 1));
                empty_file_lines.push(format!(" --> {name}:{line}:{col} ({})", l.message));
            }
            None => unresolved_notes.push(format!("{} (localização indisponível)", l.message)),
        }
    }

    if drawable.is_empty() {
        let mut out = format!("{header}: {}\n", diag.message);
        for line in &empty_file_lines {
            out.push_str(line);
            out.push('\n');
        }
        append_notes(&mut out, &unresolved_notes);
        append_notes(&mut out, &diag.notes);
        return out;
    }

    // A âncora do relatório passa a ser o primeiro label desenhável (o
    // primário, se algum primário sobreviver à filtragem acima).
    let anchor = drawable
        .iter()
        .find(|l| l.primary)
        .or_else(|| drawable.first())
        .expect("verificado acima: drawable não está vazio");
    let anchor_file = anchor.span.file;
    let anchor_offset = anchor.span.lo;

    let mut builder = Report::build(
        ReportKind::Custom(header, header_color),
        anchor_file,
        anchor_offset as usize,
    )
    .with_config(
        Config::default()
            .with_color(color)
            .with_index_type(IndexType::Byte),
    )
    .with_message(&diag.message);

    for label in &drawable {
        let range = label.span.lo as usize..label.span.hi as usize;
        let mut al = AriadneLabel::new((label.span.file, range)).with_message(&label.message);
        al = if label.primary {
            al.with_order(0).with_color(header_color)
        } else {
            al.with_order(1).with_color(Color::Blue)
        };
        builder = builder.with_label(al);
    }

    let report = builder.finish();
    let file_ids = drawable.iter().map(|l| l.span.file).chain([anchor_file]);
    let mut cache = build_cache(sources, file_ids);

    let mut buf = Vec::new();
    report
        .write(&mut cache, &mut buf)
        .expect("escrita em memória não falha");
    let text = String::from_utf8(buf).expect("ariadne produz UTF-8 válido");

    // ReportKind::Custom não respeita Config::with_color para a cor do cabeçalho
    // (limitação do ariadne 0.4); garantimos saída sem ANSI aqui.
    let text = if color { text } else { strip_ansi(&text) };

    let mut out = if empty_file_lines.is_empty() {
        text
    } else {
        insert_after_header(&text, &empty_file_lines)
    };

    append_notes(&mut out, &unresolved_notes);
    append_notes(&mut out, &diag.notes);
    out
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

    #[test]
    fn renders_correctly_with_multibyte_chars_before_label_same_line() {
        let mut sources = SourceMap::new();
        let src = "é = ç + zzz\n".to_string();
        let start = src.find("zzz").expect("fixture contém `zzz`") as u32;
        let file = sources.add("p.cdr".to_string(), src);

        let diag = Diagnostic::error("nome desconhecido: `zzz`")
            .with_primary(Span::new(file, start, start + 3), "não está definida");

        let output = render(&diag, &sources, false);
        assert!(output.contains("p.cdr:1:9"));
        insta::assert_snapshot!(output);
    }

    #[test]
    fn renders_correctly_with_multibyte_chars_before_label_multiline() {
        let mut sources = SourceMap::new();
        let src = "# café é bom\nx = 1 + zzz\n".to_string();
        let start = src.find("zzz").expect("fixture contém `zzz`") as u32;
        let file = sources.add("p.cdr".to_string(), src);

        let diag = Diagnostic::error("nome desconhecido: `zzz`")
            .with_primary(Span::new(file, start, start + 3), "não está definida");

        let output = render(&diag, &sources, false);
        assert!(output.contains("p.cdr:2:9"));
        insta::assert_snapshot!(output);
    }

    #[test]
    fn renders_empty_label_at_end_of_file_without_panic() {
        let mut sources = SourceMap::new();
        let file = sources.add("p.cdr".to_string(), "x = 1".to_string());

        let diag = Diagnostic::error("fim de ficheiro inesperado")
            .with_primary(Span::new(file, 5, 5), "esperava-se algo aqui");

        let output = render(&diag, &sources, false);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn crlf_source_output_has_no_carriage_return() {
        let mut sources = SourceMap::new();
        let src = "x = 1\r\ny = zzz\r\n".to_string();
        let start = src.find("zzz").expect("fixture contém `zzz`") as u32;
        let file = sources.add("p.cdr".to_string(), src);

        let diag = Diagnostic::error("nome desconhecido: `zzz`")
            .with_primary(Span::new(file, start, start + 3), "não está definida");

        let output = render(&diag, &sources, false);
        assert!(!output.contains('\r'));
    }

    #[test]
    fn render_without_labels_prints_plain_message_with_notes() {
        let sources = SourceMap::new();

        let diag = Diagnostic::error("ficheiro não encontrado: x.cdr")
            .with_note("verifica o caminho passado a `cudrac`");

        let output = render(&diag, &sources, false);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_with_label_pointing_to_unknown_file_reports_exact_output() {
        let sources = SourceMap::new();

        let diag = Diagnostic::error("erro interno")
            .with_primary(Span::new(FileId(9), 0, 1), "aqui")
            .with_note("nota extra");

        let output = render(&diag, &sources, false);
        assert_eq!(
            output,
            "erro: erro interno\n   = nota: aqui (localização indisponível)\n   = nota: nota extra\n"
        );
    }

    #[test]
    fn render_clamps_label_that_starts_mid_char() {
        let mut sources = SourceMap::new();
        // offsets 1..3 caem a meio do "é" (bytes 1-2).
        let file = sources.add("p.cdr".to_string(), "é = 1\n".to_string());

        let diag = Diagnostic::error("tipo inesperado").with_primary(Span::new(file, 1, 3), "aqui");

        let output = render(&diag, &sources, false);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_clamps_label_far_past_end_shows_last_char() {
        let mut sources = SourceMap::new();
        let file = sources.add("p.cdr".to_string(), "x = 1\n".to_string());

        let diag = Diagnostic::error("fim de ficheiro inesperado")
            .with_primary(Span::new(file, 4, 999), "esperava-se mais");

        let output = render(&diag, &sources, false);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_clamps_label_entirely_past_end_to_empty_at_eof() {
        let mut sources = SourceMap::new();
        let file = sources.add("p.cdr".to_string(), "x = 1\n".to_string());

        let diag = Diagnostic::error("fim de ficheiro inesperado")
            .with_primary(Span::new(file, 500, 600), "esperava-se mais");

        let output = render(&diag, &sources, false);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_on_empty_source_never_loses_file_name_and_position() {
        let mut sources = SourceMap::new();
        let file = sources.add("p.cdr".to_string(), String::new());

        let diag = Diagnostic::error("ficheiro vazio").with_primary(Span::new(file, 0, 0), "aqui");

        let output = render(&diag, &sources, false);
        assert!(output.contains("p.cdr:1:1"));
        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_on_empty_source_adds_location_even_if_message_mentions_file_name() {
        let mut sources = SourceMap::new();
        let file = sources.add("p.cdr".to_string(), String::new());

        let diag = Diagnostic::error("empty source, mensagem cita p.cdr")
            .with_primary(Span::new(file, 0, 0), "aqui");

        let output = render(&diag, &sources, false);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_on_empty_source_with_file_name_substring_of_header() {
        let mut sources = SourceMap::new();
        // "e" é uma substring de "erro"; o atalho antigo (procurar o nome no
        // texto já gerado) marcava isto como "já presente" por engano.
        let file = sources.add("e".to_string(), String::new());

        let diag = Diagnostic::error("qualquer coisa").with_primary(Span::new(file, 0, 0), "aqui");

        let output = render(&diag, &sources, false);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_keeps_labels_in_other_files_when_anchor_file_is_empty() {
        let mut sources = SourceMap::new();
        let empty_file = sources.add("e".to_string(), String::new());
        let valid_file = sources.add("p.cdr".to_string(), "x = zzz\n".to_string());

        let diag = Diagnostic::error("nome desconhecido: `zzz`")
            .with_primary(Span::new(empty_file, 0, 0), "vazio")
            .with_secondary(Span::new(valid_file, 4, 7), "válido");

        let output = render(&diag, &sources, false);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn render_never_drops_a_label_message_regardless_of_file_kind() {
        let mut sources = SourceMap::new();
        let empty_file = sources.add("empty.cdr".to_string(), String::new());
        let valid_file = sources.add("valid.cdr".to_string(), "x = zzz\n".to_string());
        let unknown_file = FileId(9999);

        // Um label em ficheiro vazio, um em ficheiro com conteúdo e um em
        // FileId inexistente. Testamos todas as combinações não vazias.
        let kinds: [(&str, Span); 3] = [
            ("mensagem-vazia", Span::new(empty_file, 0, 0)),
            ("mensagem-valida", Span::new(valid_file, 4, 7)),
            ("mensagem-desconhecida", Span::new(unknown_file, 0, 1)),
        ];

        for mask in 1u32..(1 << kinds.len()) {
            let mut diag = Diagnostic::error("erro de teste");
            let mut first = true;
            for (i, (msg, span)) in kinds.iter().enumerate() {
                if mask & (1 << i) == 0 {
                    continue;
                }
                diag = if first {
                    first = false;
                    diag.with_primary(*span, *msg)
                } else {
                    diag.with_secondary(*span, *msg)
                };
            }

            let output = render(&diag, &sources, false);
            for (i, (msg, _)) in kinds.iter().enumerate() {
                if mask & (1 << i) != 0 {
                    assert!(
                        output.contains(msg),
                        "mask={mask:#05b}: falta a mensagem '{msg}' no output:\n{output}"
                    );
                }
            }
        }
    }

    #[test]
    fn render_never_panics_for_any_span_within_extended_bounds() {
        let mut sources = SourceMap::new();
        // Mistura chars multi-byte (é, 🙂), \r\n, \r sozinho e tab.
        let src = "a é\tb🙂\r\nc\rd\n".to_string();
        let len = src.len() as u32;
        let file = sources.add("p.cdr".to_string(), src);

        for lo in 0..=(len + 3) {
            for hi in lo..=(len + 3) {
                let diag = Diagnostic::error("erro de teste")
                    .with_primary(Span::new(file, lo, hi), "aqui");
                let output = render(&diag, &sources, false);
                assert!(
                    !output.contains('\u{1b}'),
                    "lo={lo} hi={hi}: output tem ANSI"
                );
                assert!(!output.contains('\r'), "lo={lo} hi={hi}: output tem CR");
            }
        }
    }
}
