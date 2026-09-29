//! Small lossless block syntax. This parser knows structure, not economics.

use crate::Diagnostic;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub source: String,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub head: String,
    pub args: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub value: String,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_BLOCKS: usize = 20_000;
const MAX_FIELDS: usize = 100_000;

/// Parse top-level `head args` blocks and exactly two-space-indented fields.
/// The original UTF-8 source is retained byte for byte in `Document::source`.
pub(crate) fn parse(source: &str) -> Result<Document, Vec<Diagnostic>> {
    if source.len() > MAX_SOURCE_BYTES {
        let first_line = source.split('\n').next().unwrap_or("");
        let first_line = first_line.strip_suffix('\r').unwrap_or(first_line);
        return Err(vec![
            Diagnostic::new(1, "source exceeds the one-megabyte syntax limit").with_span(Span {
                start: 0,
                end: first_line.len(),
                line: 1,
            }),
        ]);
    }

    let mut blocks: Vec<Block> = Vec::new();
    let mut diagnostics = Vec::new();
    let mut offset = 0usize;
    let mut line_number = 1usize;
    let mut field_count = 0usize;

    for chunk in source.split_inclusive('\n') {
        let mut line = chunk.strip_suffix('\n').unwrap_or(chunk);
        if let Some(without_cr) = line.strip_suffix('\r') {
            line = without_cr;
        }
        let line_end = offset + line.len();
        let (uncommented, unmatched_quote) = strip_comment(line);
        if let Some(quote_start) = unmatched_quote {
            diagnostics.push(
                Diagnostic::new(line_number, "unterminated quoted text").with_span(Span {
                    start: offset + quote_start,
                    end: line_end,
                    line: line_number,
                }),
            );
        }
        let content = uncommented.trim_end();
        if content.trim().is_empty() {
            offset += chunk.len();
            line_number += 1;
            continue;
        }

        let indentation = content
            .bytes()
            .take_while(|byte| *byte == b' ' || *byte == b'\t')
            .count();
        let has_tab_indent = content
            .bytes()
            .take_while(|byte| *byte == b' ' || *byte == b'\t')
            .any(|byte| byte == b'\t');
        if has_tab_indent {
            let tab_indent_end = offset
                + content
                    .bytes()
                    .take_while(|byte| *byte == b' ' || *byte == b'\t')
                    .count();
            diagnostics.push(
                Diagnostic::new(line_number, "indentation must use two spaces, not tabs")
                    .with_span(Span {
                        start: offset,
                        end: tab_indent_end,
                        line: line_number,
                    }),
            );
        } else if indentation == 0 {
            match parse_head(content) {
                Ok((head, args)) => {
                    if blocks.len() >= MAX_BLOCKS {
                        diagnostics.push(line_diagnostic(
                            line_number,
                            "source contains too many blocks",
                            offset,
                            line_end,
                        ));
                    } else {
                        blocks.push(Block {
                            head,
                            args,
                            fields: Vec::new(),
                            span: Span {
                                start: offset,
                                end: line_end,
                                line: line_number,
                            },
                        });
                    }
                }
                Err(message) => {
                    diagnostics.push(line_diagnostic(line_number, message, offset, line_end))
                }
            }
        } else if indentation == 2 {
            if let Some(block) = blocks.last_mut() {
                match parse_field(&content[2..]) {
                    Ok((name, value)) => {
                        field_count += 1;
                        if field_count > MAX_FIELDS {
                            diagnostics.push(line_diagnostic(
                                line_number,
                                "source contains too many fields",
                                offset,
                                line_end,
                            ));
                        } else {
                            block.fields.push(Field {
                                name,
                                value,
                                span: Span {
                                    start: offset,
                                    end: line_end,
                                    line: line_number,
                                },
                            });
                            block.span.end = line_end;
                        }
                    }
                    Err(issue) => diagnostics.push(
                        Diagnostic::new(line_number, issue.message).with_span(Span {
                            start: offset + 2 + issue.start,
                            end: offset + 2 + issue.end,
                            line: line_number,
                        }),
                    ),
                }
            } else {
                diagnostics.push(line_diagnostic(
                    line_number,
                    "field appears before any top-level block",
                    offset,
                    line_end,
                ));
            }
        } else {
            diagnostics.push(line_diagnostic(
                line_number,
                "fields must be indented by exactly two spaces",
                offset,
                line_end,
            ));
        }

        offset += chunk.len();
        line_number += 1;
    }

    if diagnostics.is_empty() {
        Ok(Document {
            source: source.to_owned(),
            blocks,
        })
    } else {
        Err(diagnostics)
    }
}

fn line_diagnostic(
    line: usize,
    message: impl Into<String>,
    start: usize,
    end: usize,
) -> Diagnostic {
    Diagnostic::new(line, message).with_span(Span { start, end, line })
}

fn parse_head(line: &str) -> Result<(String, String), String> {
    let line = line.trim();
    let split = line.find(char::is_whitespace).unwrap_or(line.len());
    let head = &line[..split];
    let args = line[split..].trim();
    if head.is_empty() {
        return Err("expected a top-level head".into());
    }
    Ok((head.to_owned(), args.to_owned()))
}

struct FieldIssue {
    message: String,
    start: usize,
    end: usize,
}

fn parse_field(source: &str) -> Result<(String, String), FieldIssue> {
    let leading = source.len() - source.trim_start().len();
    let line = source.trim();
    let split = line.find(char::is_whitespace).unwrap_or(line.len());
    let name = &line[..split];
    let value = line[split..].trim();
    if name.is_empty() {
        return Err(FieldIssue {
            message: "expected a field name".into(),
            start: leading,
            end: leading + source.trim_end().len().saturating_sub(leading),
        });
    }
    if value.is_empty() {
        return Err(FieldIssue {
            message: format!("field '{name}' requires a value"),
            start: leading,
            end: leading + name.len(),
        });
    }
    Ok((name.to_owned(), value.to_owned()))
}

/// Strip an unquoted comment while preserving `#` inside quoted strings.
fn strip_comment(line: &str) -> (&str, Option<usize>) {
    let mut quoted = false;
    let mut escaped = false;
    let mut quote_start = None;
    for (index, ch) in line.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
                quote_start = None;
            }
        } else if ch == '"' {
            quoted = true;
            quote_start = Some(index);
        } else if ch == '#' {
            return (&line[..index], None);
        }
    }
    (line, if quoted { quote_start } else { None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_source_and_byte_spans_with_crlf_and_comments() {
        let source = "form buy\r\n  note \"x # y\" # comment\r\n\r\n# tail\r\n";
        let document = parse(source).unwrap();
        assert_eq!(document.source, source);
        assert_eq!(document.blocks.len(), 1);
        assert_eq!(document.blocks[0].fields[0].value, "\"x # y\"");
        assert_eq!(document.blocks[0].fields[0].span.line, 2);
        assert_eq!(
            document.blocks[0].fields[0].span.start,
            source.find("  note").unwrap()
        );
        let note_start = source.find("  note").unwrap();
        assert_eq!(
            document.blocks[0].span.end,
            note_start + source[note_start..].find("\r\n").unwrap()
        );
    }

    #[test]
    fn keeps_duplicate_fields_for_the_schema_phase() {
        let document = parse("form thing\n  name text\n  name text\n").unwrap();
        assert_eq!(
            document.blocks[0]
                .fields
                .iter()
                .filter(|field| field.name == "name")
                .count(),
            2
        );
    }

    #[test]
    fn allows_argumentless_entry_heads() {
        let document = parse("buy\n  amount 4.50 USD\nparty\n  name \"Coffee shop\"\n").unwrap();
        assert_eq!(document.blocks[0].head, "buy");
        assert!(document.blocks[0].args.is_empty());
        assert_eq!(document.blocks[1].head, "party");
    }

    #[test]
    fn rejects_bad_indentation_or_unclosed_quotes() {
        assert!(parse("form thing\n   name text\n").is_err());
        assert!(parse("form thing\n \tname text\n").is_err());
        assert!(parse("form thing\n  note \"open # not comment\n").is_err());
        assert!(parse("  name text\n").is_err());
    }
}
