//! Lossless authoring-surface support.
//!
//! This module deliberately stops at the boundary between source text and a
//! semantic compiler.  It is a small, tolerant concrete syntax tree (CST):
//! every byte is retained, trivia is represented by tokens, and malformed or
//! future syntax is represented by a recoverable node instead of being thrown
//! away.  The existing domain parser remains the place where a supported
//! subset is given meaning.
//!
//! There are two formatting operations:
//!
//! * [`SurfaceFile::lossless`] (also [`format_lossless`]) returns the exact
//!   source bytes supplied to the parser.
//! * [`SurfaceFile::canonical`] (also [`canonical_format`]) emits a stable,
//!   compact, journal-like spelling while retaining all non-trivia tokens,
//!   comments, holes, and unknown syntax.
//!
//! No numeric token is converted to a floating-point value here.  Numbers,
//! strings, and unknown syntax remain source spellings until a later typed
//! phase chooses how to interpret them.

use std::fmt;

/// A half-open byte span into the original UTF-8 source.
///
/// Offsets are bytes rather than characters so a span can always slice the
/// source without re-encoding it.  `line` and `column` are one-based and are
/// calculated by the lexer for diagnostics and editor clients.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize, line: usize, column: usize) -> Self {
        Self {
            start,
            end,
            line,
            column,
        }
    }

    pub const fn empty_at(offset: usize, line: usize, column: usize) -> Self {
        Self::new(offset, offset, line, column)
    }

    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Return the exact source covered by this span when it is valid for the
    /// supplied source.  Public spans can be constructed by editor clients,
    /// so this deliberately does not panic on forged offsets or boundaries.
    pub fn text(self, source: &str) -> Option<&str> {
        (self.start <= self.end
            && self.end <= source.len()
            && source.is_char_boundary(self.start)
            && source.is_char_boundary(self.end))
        .then(|| &source[self.start..self.end])
    }
}

/// A typed existential hole in the authoring surface.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Hole {
    /// `_` is an anonymous hole.  It may be solved but cannot be referred to
    /// by a later clause.
    Anonymous,
    /// `?name` is a named hole whose identity is visible to diagnostics and
    /// later elaboration.
    Named(String),
}

impl Hole {
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Anonymous => None,
            Self::Named(name) => Some(name),
        }
    }

    pub const fn is_anonymous(&self) -> bool {
        matches!(self, Self::Anonymous)
    }
}

impl fmt::Display for Hole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anonymous => formatter.write_str("_"),
            Self::Named(name) => write!(formatter, "?{name}"),
        }
    }
}

/// A token's broad lexical class.  The original spelling is always available
/// through [`Token::lexeme`], including for unknown characters.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TokenKind {
    Whitespace,
    Newline,
    Comment,
    Identifier,
    Number,
    String,
    Hole(Hole),
    Punctuation(char),
    Unknown,
}

impl TokenKind {
    pub const fn is_trivia(&self) -> bool {
        matches!(self, Self::Whitespace | Self::Newline | Self::Comment)
    }

    pub const fn is_hole(&self) -> bool {
        matches!(self, Self::Hole(_))
    }
}

/// One lossless lexical item.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// Exact bytes from the source, never a normalized spelling.
    pub lexeme: String,
}

impl Token {
    pub fn text<'a>(&self, source: &'a str) -> Option<&'a str> {
        self.span.text(source)
    }

    pub fn is_trivia(&self) -> bool {
        self.kind.is_trivia()
    }

    pub fn is_hole(&self) -> bool {
        self.kind.is_hole()
    }

    pub fn hole(&self) -> Option<&Hole> {
        match &self.kind {
            TokenKind::Hole(hole) => Some(hole),
            _ => None,
        }
    }
}

/// The recoverable severity of a surface diagnostic.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    Warning,
    Error,
}

/// A source diagnostic.  Diagnostics never replace the source or its CST.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            span,
            message: message.into(),
        }
    }

    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "line {}, column {}: {}",
            self.span.line, self.span.column, self.message
        )
    }
}

/// A stable identity for a semantic surface node.
///
/// The ID is derived from the node kind, its non-trivia token spellings, and
/// its occurrence among equal siblings.  Consequently inserting or changing
/// whitespace/comments does not change an ID.  Equal duplicate nodes use the
/// occurrence number to remain distinguishable; moving a duplicate past its
/// sibling is intentionally outside this guarantee.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeId([u8; 16]);

impl NodeId {
    pub const ZERO: Self = Self([0; 16]);

    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        use std::fmt::Write as _;
        let mut text = String::with_capacity(32);
        for byte in self.0 {
            let _ = write!(text, "{byte:02x}");
        }
        text
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

/// Broad CST categories.  `Unknown` is valid future syntax, while `Error`
/// marks a known shape which is incomplete or malformed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NodeKind {
    Document,
    Form,
    JournalEntry,
    Directive,
    Unknown,
    Error,
}

impl NodeKind {
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }

    pub const fn is_semantic(self) -> bool {
        !matches!(self, Self::Document)
    }
}

/// A top-level semantic node and its exact token range.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SyntaxNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub span: Span,
    pub token_start: usize,
    pub token_end: usize,
    /// The first non-trivia token's spelling, useful to editor clients and
    /// deliberately not interpreted as a full HIR node.
    pub head: Option<String>,
    pub error: Option<Diagnostic>,
}

impl SyntaxNode {
    pub fn is_error(&self) -> bool {
        self.kind.is_error()
    }

    pub fn token_range(&self) -> Option<std::ops::Range<usize>> {
        (self.token_start <= self.token_end).then_some(self.token_start..self.token_end)
    }
}

/// A borrowed view over a generic package-authored `form` node.
///
/// This is intentionally a view over the existing lossless CST rather than a
/// second parsed AST.  Header names and field values remain [`Token`]s, so an
/// editor or a package compiler can diagnose malformed input without first
/// losing its spelling.
#[derive(Clone, Copy, Debug)]
pub struct FormView<'a> {
    file: &'a SurfaceFile,
    node: &'a SyntaxNode,
}

impl<'a> FormView<'a> {
    fn new(file: &'a SurfaceFile, node: &'a SyntaxNode) -> Self {
        Self { file, node }
    }

    /// The underlying top-level node.
    pub fn node(&self) -> &'a SyntaxNode {
        self.node
    }

    /// The identifier occurrence immediately following `form`, if present.
    /// Generic-form syntax deliberately uses identifier spellings (including
    /// `/` and `-` name bytes); `:` is reserved for the schema delimiter.
    /// Malformed or non-identifier occurrences return `None` without
    /// fabricating an identity.
    pub fn occurrence(&self) -> Option<&'a Token> {
        form_header_parts(self.file.tokens(), self.node).occurrence
    }

    /// The schema token slice after the header's `:` delimiter.  Whitespace
    /// and comments are retained; use [`Self::schema_parts`] when only
    /// meaningful tokens are wanted.
    pub fn schema_tokens(&self) -> &'a [Token] {
        let parts = form_header_parts(self.file.tokens(), self.node);
        &self.file.tokens()[parts.schema_start..parts.schema_end]
    }

    /// Meaningful schema pieces, in source order, without coercing or joining
    /// their source spellings.
    pub fn schema_parts(&self) -> impl Iterator<Item = &'a Token> {
        self.schema_tokens()
            .iter()
            .filter(|token| !token.is_trivia())
    }

    /// Every non-trivia indented physical line belonging to this form,
    /// including lines whose first token is malformed. Duplicate field names
    /// are deliberately not collapsed.
    pub fn fields(&self) -> impl Iterator<Item = FormField<'a>> + 'a {
        FormFieldIter::new(self.file, self.node)
    }
}

/// An iterator over the physical field lines in a generic form.
#[derive(Clone, Debug)]
struct FormFieldIter<'a> {
    file: &'a SurfaceFile,
    node: &'a SyntaxNode,
    next: usize,
}

impl<'a> FormFieldIter<'a> {
    fn new(file: &'a SurfaceFile, node: &'a SyntaxNode) -> Self {
        let (_, header_end) = form_header_range(file.tokens(), node);
        Self {
            file,
            node,
            next: header_end,
        }
    }
}

impl<'a> Iterator for FormFieldIter<'a> {
    type Item = FormField<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let tokens = self.file.tokens();
        loop {
            while self.next < self.node.token_end
                && (tokens[self.next].kind == TokenKind::Newline
                    || tokens[self.next].span.line == self.node.span.line)
            {
                self.next += 1;
            }
            if self.next >= self.node.token_end {
                return None;
            }
            let start = self.next;
            let line = tokens[start].span.line;
            while self.next < self.node.token_end
                && tokens[self.next].kind != TokenKind::Newline
                && tokens[self.next].span.line == line
            {
                self.next += 1;
            }
            let end = self.next;
            if tokens[start..end].iter().any(|token| !token.is_trivia()) {
                return Some(FormField {
                    file: self.file,
                    node: self.node,
                    token_start: start,
                    token_end: end,
                });
            }
        }
    }
}

/// One generic form field occurrence.  It is a physical occurrence, not a
/// map entry: repeated names and malformed lines remain separately visible.
#[derive(Clone, Copy, Debug)]
pub struct FormField<'a> {
    file: &'a SurfaceFile,
    node: &'a SyntaxNode,
    token_start: usize,
    token_end: usize,
}

impl<'a> FormField<'a> {
    pub fn span(&self) -> Span {
        let tokens = &self.file.tokens()[self.token_start..self.token_end];
        let start = tokens
            .first()
            .map_or(self.node.span.end, |token| token.span.start);
        let end = tokens.last().map_or(start, |token| token.span.end);
        Span::new(
            start,
            end,
            tokens
                .first()
                .map_or(self.node.span.line, |token| token.span.line),
            tokens
                .first()
                .map_or(self.node.span.column, |token| token.span.column),
        )
    }

    /// The exact token slice for this physical line, including indentation,
    /// separators, comments, and malformed punctuation.
    pub fn tokens(&self) -> &'a [Token] {
        &self.file.tokens()[self.token_start..self.token_end]
    }

    /// The first meaningful field-name token, when one exists.  A line which
    /// starts with punctuation or a comment is still returned as a field with
    /// no name, preserving it for later diagnostics.
    pub fn name(&self) -> Option<&'a Token> {
        self.tokens()
            .iter()
            .find(|token| !token.is_trivia())
            .filter(|token| token.kind == TokenKind::Identifier)
    }

    /// The exact token slice after the first meaningful name token.  No value
    /// conversion or validation happens here.
    pub fn value_tokens(&self) -> &'a [Token] {
        let Some((offset, _)) = self
            .tokens()
            .iter()
            .enumerate()
            .find(|(_, token)| !token.is_trivia())
            .filter(|(_, token)| token.kind == TokenKind::Identifier)
        else {
            return &[];
        };
        &self.tokens()[offset + 1..]
    }

    pub fn value_parts(&self) -> impl Iterator<Item = &'a Token> {
        self.value_tokens()
            .iter()
            .filter(|token| !token.is_trivia())
    }
}

/// A lossless parsed source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceFile {
    source: String,
    tokens: Vec<Token>,
    nodes: Vec<SyntaxNode>,
    diagnostics: Vec<Diagnostic>,
}

impl SurfaceFile {
    /// Parse any source into a recoverable CST.  This function intentionally
    /// does not return `Result`: an editor must be able to inspect and format
    /// an incomplete file without destroying its contents.
    pub fn parse(source: impl Into<String>) -> Self {
        let source = source.into();
        let tokens = lex(&source);
        let (nodes, diagnostics) = build_nodes(&source, &tokens);
        Self {
            source,
            tokens,
            nodes,
            diagnostics,
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// Exact lossless source spelling.
    pub fn lossless(&self) -> &str {
        self.source()
    }

    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    pub fn nodes(&self) -> &[SyntaxNode] {
        &self.nodes
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
    }

    pub fn error_nodes(&self) -> impl Iterator<Item = &SyntaxNode> {
        self.nodes.iter().filter(|node| node.is_error())
    }

    pub fn node(&self, id: NodeId) -> Option<&SyntaxNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Borrow each generic package-authored form without building another AST.
    pub fn forms(&self) -> impl Iterator<Item = FormView<'_>> {
        self.nodes
            .iter()
            .filter(|node| node.head.as_deref() == Some("form"))
            .map(|node| FormView::new(self, node))
    }

    pub fn form(&self, id: NodeId) -> Option<FormView<'_>> {
        self.nodes
            .iter()
            .find(|node| node.id == id && node.head.as_deref() == Some("form"))
            .map(|node| FormView::new(self, node))
    }

    /// Stable source identity for a semantic node.
    pub fn node_id(&self, index: usize) -> Option<NodeId> {
        self.nodes.get(index).map(|node| node.id)
    }

    /// The original source, by design.
    pub fn format_lossless(&self) -> String {
        self.source.clone()
    }

    /// Stable compact formatting for display or generated output.  All
    /// non-trivia token spellings and comments survive; only indentation and
    /// spaces between tokens are normalized.
    pub fn canonical(&self) -> String {
        canonical_source(&self.source)
    }

    pub fn canonical_format(&self) -> String {
        self.canonical()
    }

    /// Parse the canonical spelling again.  This is useful to callers that
    /// want a cheap structural round-trip assertion without losing the first
    /// file's exact source.
    pub fn reparsed_canonical(&self) -> Self {
        Self::parse(self.canonical())
    }
}

/// Parse source without rejecting malformed or future syntax.
pub fn parse_surface(source: impl Into<String>) -> SurfaceFile {
    SurfaceFile::parse(source)
}

/// Minimal spelling for integrations that simply need a tolerant surface
/// parse.
pub fn parse(source: impl Into<String>) -> SurfaceFile {
    parse_surface(source)
}

/// Short alias suitable for editor and formatter integrations.
pub fn parse_lossless(source: impl Into<String>) -> SurfaceFile {
    parse_surface(source)
}

/// Return the exact input spelling after a lossless parse.
pub fn format_lossless(file: &SurfaceFile) -> String {
    file.format_lossless()
}

/// Return canonical compact journal-like formatting.
pub fn canonical_format(source: &str) -> String {
    SurfaceFile::parse(source).canonical()
}

/// Exact lossless round-trip predicate.
pub fn lossless_round_trip(source: &str) -> bool {
    SurfaceFile::parse(source).format_lossless() == source
}

/// Canonical parse/format/parse predicate.  Node IDs are compared because
/// IDs intentionally ignore trivia while diagnostics remain recoverable.
pub fn canonical_round_trip(source: &str) -> bool {
    let first = SurfaceFile::parse(source);
    let second = SurfaceFile::parse(first.canonical());
    first
        .nodes
        .iter()
        .map(|node| (node.kind, node.id))
        .eq(second.nodes.iter().map(|node| (node.kind, node.id)))
}

fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1;
    let mut column = 1;

    while index < bytes.len() {
        let start = index;
        let start_line = line;
        let start_column = column;
        let byte = bytes[index];

        if byte == b'\r' || byte == b'\n' {
            if byte == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 2;
            } else {
                index += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Newline,
                span: Span::new(start, index, start_line, start_column),
                lexeme: source[start..index].to_owned(),
            });
            line += 1;
            column = 1;
            continue;
        }

        if byte == b' ' || byte == b'\t' || byte == 0x0b || byte == 0x0c {
            index += 1;
            while index < bytes.len() && matches!(bytes[index], b' ' | b'\t' | 0x0b | 0x0c) {
                index += 1;
            }
            let length = index - start;
            tokens.push(Token {
                kind: TokenKind::Whitespace,
                span: Span::new(start, index, start_line, start_column),
                lexeme: source[start..index].to_owned(),
            });
            column += length;
            continue;
        }

        if byte == b';' || byte == b'#' {
            index += 1;
            while index < bytes.len() && bytes[index] != b'\r' && bytes[index] != b'\n' {
                index += 1;
            }
            let length = index - start;
            tokens.push(Token {
                kind: TokenKind::Comment,
                span: Span::new(start, index, start_line, start_column),
                lexeme: source[start..index].to_owned(),
            });
            column += length;
            continue;
        }

        if byte == b'"' || byte == b'\'' {
            let quote = byte;
            index += 1;
            let mut escaped = false;
            while index < bytes.len() {
                let current = bytes[index];
                if current == b'\r' || current == b'\n' {
                    break;
                }
                index += 1;
                if escaped {
                    escaped = false;
                } else if current == b'\\' {
                    escaped = true;
                } else if current == quote {
                    break;
                }
            }
            tokens.push(Token {
                kind: TokenKind::String,
                span: Span::new(start, index, start_line, start_column),
                lexeme: source[start..index].to_owned(),
            });
            column += index - start;
            continue;
        }

        // `:` is normally a legal name byte in ledger account spellings
        // (`Assets:Checking`).  The generic-form header is the one place
        // where it is a delimiter, so recognize that delimiter from the
        // already-emitted `form` and occurrence tokens without changing the
        // tokenization of ordinary journal lines.
        if byte == b':' && is_form_header_delimiter(&tokens, start_line) {
            index += 1;
            tokens.push(Token {
                kind: TokenKind::Punctuation(':'),
                span: Span::new(start, index, start_line, start_column),
                lexeme: ":".to_owned(),
            });
            column += 1;
            continue;
        }

        if byte == b'?' {
            index += 1;
            if bytes.get(index) == Some(&b'?') {
                while bytes.get(index) == Some(&b'?') {
                    index += 1;
                }
                tokens.push(Token {
                    kind: TokenKind::Unknown,
                    span: Span::new(start, index, start_line, start_column),
                    lexeme: source[start..index].to_owned(),
                });
                column += index - start;
                continue;
            }
            while index < bytes.len() && is_name_byte(bytes[index]) {
                index += 1;
            }
            let spelling = &source[start..index];
            let hole = if spelling == "?" {
                Hole::Anonymous
            } else {
                Hole::Named(spelling[1..].to_owned())
            };
            tokens.push(Token {
                kind: TokenKind::Hole(hole),
                span: Span::new(start, index, start_line, start_column),
                lexeme: spelling.to_owned(),
            });
            column += index - start;
            continue;
        }

        if byte == b'_' {
            index += 1;
            // `_` is a hole only as a standalone token.  A larger name such
            // as `_tmp` remains an ordinary unknown/identifier spelling.
            while index < bytes.len() && is_name_byte(bytes[index]) {
                index += 1;
            }
            let spelling = &source[start..index];
            let kind = if spelling == "_" {
                TokenKind::Hole(Hole::Anonymous)
            } else {
                TokenKind::Identifier
            };
            tokens.push(Token {
                kind,
                span: Span::new(start, index, start_line, start_column),
                lexeme: spelling.to_owned(),
            });
            column += index - start;
            continue;
        }

        if is_name_byte(byte) {
            index += 1;
            while index < bytes.len()
                && is_name_byte(bytes[index])
                && !(bytes[index] == b':' && is_form_header_occurrence(&tokens, start_line))
            {
                index += 1;
            }
            let spelling = &source[start..index];
            let kind = if is_number(spelling) {
                TokenKind::Number
            } else {
                TokenKind::Identifier
            };
            tokens.push(Token {
                kind,
                span: Span::new(start, index, start_line, start_column),
                lexeme: spelling.to_owned(),
            });
            column += index - start;
            continue;
        }

        // Every remaining Unicode scalar or ASCII punctuation is emitted as a
        // token.  Keeping it is more useful to a future package than
        // prematurely deciding that it is invalid.
        let Some(character) = source[index..].chars().next() else {
            break;
        };
        let character_len = character.len_utf8();
        index += character_len;
        let kind = if character.is_ascii_punctuation() {
            TokenKind::Punctuation(character)
        } else {
            TokenKind::Unknown
        };
        tokens.push(Token {
            kind,
            span: Span::new(start, index, start_line, start_column),
            lexeme: source[start..index].to_owned(),
        });
        column += 1;
    }

    tokens
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(byte, b'/' | b'_' | b'-' | b'.' | b':' | b'@' | b'+' | b'%')
}

fn form_line_meaningful_reverse(tokens: &[Token], line: usize) -> impl Iterator<Item = &Token> {
    tokens
        .iter()
        .rev()
        .take_while(move |token| token.span.line == line)
        .filter(|token| !token.is_trivia())
}

fn is_form_header_occurrence(tokens: &[Token], line: usize) -> bool {
    let mut meaningful = form_line_meaningful_reverse(tokens, line);
    meaningful
        .next()
        .is_some_and(|token| token.lexeme == "form")
        && meaningful.next().is_none()
}

fn is_form_header_delimiter(tokens: &[Token], line: usize) -> bool {
    let mut meaningful = form_line_meaningful_reverse(tokens, line);
    meaningful.next().is_some()
        && meaningful
            .next()
            .is_some_and(|token| token.lexeme == "form")
        && meaningful.next().is_none()
}

fn is_number(spelling: &str) -> bool {
    let mut digits = 0;
    let mut dots = 0;
    for (index, byte) in spelling.bytes().enumerate() {
        if index == 0 && matches!(byte, b'+' | b'-') {
            continue;
        }
        if byte.is_ascii_digit() {
            digits += 1;
        } else if byte == b'.' {
            dots += 1;
        } else {
            return false;
        }
    }
    digits > 0 && dots <= 1
}

fn build_nodes(source: &str, tokens: &[Token]) -> (Vec<SyntaxNode>, Vec<Diagnostic>) {
    let mut lines = Vec::<(usize, usize)>::new();
    let mut line_start = 0;
    for token in tokens {
        if token.kind == TokenKind::Newline {
            lines.push((line_start, token.span.end));
            line_start = token.span.end;
        }
    }
    if line_start < source.len() || source.is_empty() {
        lines.push((line_start, source.len()));
    }

    #[derive(Clone, Debug)]
    struct Group {
        token_start: usize,
        token_end: usize,
        head: usize,
    }

    // First collect line groups.  A leading-indented line belongs to the
    // preceding top-level form, which is enough structure for both the V0
    // journal idiom and richer future blocks without pretending to parse HIR.
    let mut groups = Vec::<Group>::new();
    for (line_start, line_end) in lines {
        let first = tokens.partition_point(|token| token.span.end <= line_start);
        let last = tokens.partition_point(|token| token.span.start < line_end);
        let line_tokens = &tokens[first..last];
        let meaningful: Vec<usize> = line_tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| !token.is_trivia())
            .map(|(index, _)| first + index)
            .collect();
        if meaningful.is_empty() {
            continue;
        }

        let first_meaningful = meaningful[0];
        let first_token = &tokens[first_meaningful];
        let line_text = &source[line_start..line_end];
        let indented = line_text
            .as_bytes()
            .first()
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'));
        if indented
            && !matches!(first_token.kind, TokenKind::Comment)
            && let Some(group) = groups.last_mut()
        {
            group.token_end = last;
            continue;
        }
        groups.push(Group {
            token_start: first,
            token_end: last,
            head: first_meaningful,
        });
    }

    let mut nodes: Vec<SyntaxNode> = Vec::with_capacity(groups.len());
    let mut diagnostics = Vec::new();
    let mut occurrences = std::collections::BTreeMap::<String, usize>::new();

    for group in groups {
        let first_token = &tokens[group.head];
        let meaningful = (group.token_start..group.token_end)
            .filter(|index| !tokens[*index].is_trivia())
            .collect::<Vec<_>>();
        let head = Some(first_token.lexeme.clone());
        let mut kind = classify_node(head.as_deref(), &meaningful);
        let node_span = Span::new(
            first_token.span.start,
            last_significant_end(tokens, group.token_end, first_token.span.end),
            first_token.span.line,
            first_token.span.column,
        );
        let error = node_error(kind, &node_span, head.as_deref(), &meaningful, tokens);
        if error.is_some() {
            kind = NodeKind::Error;
        }
        let signature = semantic_signature(kind, &meaningful, tokens);
        let ordinal = occurrences.entry(signature.clone()).or_insert(0);
        let id = make_node_id(kind, &signature, *ordinal);
        *ordinal += 1;
        if let Some(diagnostic) = error.clone() {
            diagnostics.push(diagnostic);
        }
        nodes.push(SyntaxNode {
            id,
            kind,
            span: node_span,
            token_start: group.token_start,
            token_end: group.token_end,
            head,
            error,
        });
    }

    (nodes, diagnostics)
}

fn last_significant_end(tokens: &[Token], end: usize, fallback: usize) -> usize {
    tokens[..end]
        .iter()
        .rev()
        .find(|token| !token.is_trivia())
        .map_or(fallback, |token| token.span.end)
}

fn classify_node(head: Option<&str>, meaningful: &[usize]) -> NodeKind {
    let Some(head) = head else {
        return NodeKind::Directive;
    };
    if is_date_like(head) {
        return NodeKind::JournalEntry;
    }
    match head {
        "form" => NodeKind::Form,
        "book" | "buy" | "sell" | "quote" | "observe" | "use" | "decide" | "event" | "entity"
        | "instrument" | "account" | "view" | "obligation" | "settlement" | "satisfy" | "check"
        | "scenario" | "complete" | "report" => {
            if known_head_is_malformed(head, meaningful) {
                NodeKind::Error
            } else if matches!(
                head,
                "book"
                    | "buy"
                    | "sell"
                    | "quote"
                    | "observe"
                    | "use"
                    | "decide"
                    | "event"
                    | "obligation"
                    | "settlement"
                    | "satisfy"
            ) {
                NodeKind::Form
            } else {
                NodeKind::Directive
            }
        }
        _ => NodeKind::Unknown,
    }
}

fn known_head_is_malformed(head: &str, meaningful: &[usize]) -> bool {
    let count = meaningful.len();
    match head {
        "book" => count < 2,
        "buy" | "sell" | "quote" => count < 2,
        "observe" => count < 2,
        "use" => count < 2,
        "decide" => count < 2,
        // A declaration/event may be a header followed by an indented body;
        // only an absent name is definitely malformed at this layer.
        "event" | "entity" | "instrument" | "account" | "view" | "obligation" | "settlement"
        | "satisfy" | "check" | "scenario" | "complete" | "report" => count < 2,
        _ => false,
    }
}

fn node_error(
    kind: NodeKind,
    span: &Span,
    head: Option<&str>,
    meaningful: &[usize],
    tokens: &[Token],
) -> Option<Diagnostic> {
    let unterminated_string = meaningful.iter().any(|index| {
        let token = &tokens[*index];
        token.kind == TokenKind::String && !is_closed_string(&token.lexeme)
    });
    let incomplete_form = head
        .is_some_and(|head| kind == NodeKind::Form && form_is_incomplete(head, meaningful, tokens));
    if !kind.is_error() && !unterminated_string && !incomplete_form {
        return None;
    }
    let message = match head {
        Some(head) => format!("incomplete `{head}` form; source retained for recovery"),
        None => "malformed source; source retained for recovery".to_owned(),
    };
    Some(Diagnostic::error(*span, message))
}

fn form_is_incomplete(head: &str, meaningful: &[usize], tokens: &[Token]) -> bool {
    let has = |spelling: &str| {
        meaningful
            .iter()
            .any(|index| tokens[*index].lexeme == spelling)
    };
    match head {
        "form" => !form_header_is_complete(meaningful, tokens),
        "buy" => !has("into") || !has("for"),
        "sell" => !has("from") || !has("for") || !has("lot"),
        "quote" => !has("="),
        "obligation" => !has("debtor") || !has("creditor") || !has("performance"),
        "settlement" => {
            !has("kind") || !has("from") || !has("to") || !has("amount") || !has("state")
        }
        "satisfy" => !has("obligation") || !has("settlement") || !has("amount") || !has("state"),
        _ => false,
    }
}

fn form_header_is_complete(meaningful: &[usize], tokens: &[Token]) -> bool {
    let Some(first) = meaningful.first() else {
        return false;
    };
    let line = tokens[*first].span.line;
    let mut header = meaningful
        .iter()
        .copied()
        .filter(|index| tokens[*index].span.line == line);
    let (Some(head), Some(occurrence), Some(delimiter)) =
        (header.next(), header.next(), header.next())
    else {
        return false;
    };
    if tokens[head].lexeme != "form"
        || tokens[occurrence].kind != TokenKind::Identifier
        || tokens[occurrence].lexeme.contains(':')
        || tokens[delimiter].kind != TokenKind::Punctuation(':')
        || tokens[delimiter].lexeme != ":"
    {
        return false;
    }
    header.next().is_some()
}

#[derive(Clone, Copy, Debug)]
struct FormHeaderParts<'a> {
    occurrence: Option<&'a Token>,
    schema_start: usize,
    schema_end: usize,
}

fn form_header_range(tokens: &[Token], node: &SyntaxNode) -> (usize, usize) {
    let end = (node.token_start..node.token_end)
        .find(|index| {
            tokens[*index].kind == TokenKind::Newline || tokens[*index].span.line != node.span.line
        })
        .unwrap_or(node.token_end);
    (node.token_start, end)
}

fn form_header_parts<'a>(tokens: &'a [Token], node: &SyntaxNode) -> FormHeaderParts<'a> {
    let (_, end) = form_header_range(tokens, node);
    let mut meaningful = (node.token_start..end).filter(|index| !tokens[*index].is_trivia());
    let _head = meaningful.next();
    let occurrence = meaningful
        .next()
        .map(|index| &tokens[index])
        .filter(|token| token.kind == TokenKind::Identifier);
    let schema_start = meaningful
        .find(|index| tokens[*index].lexeme == ":")
        .map_or(end, |index| index + 1);
    let schema_end = (schema_start..end)
        .find(|index| tokens[*index].kind == TokenKind::Comment)
        .unwrap_or(end);
    FormHeaderParts {
        occurrence,
        schema_start,
        schema_end,
    }
}

fn is_closed_string(spelling: &str) -> bool {
    spelling.len() >= 2
        && matches!(spelling.as_bytes().first(), Some(b'"' | b'\''))
        && spelling.as_bytes().last() == spelling.as_bytes().first()
}

fn is_date_like(value: &str) -> bool {
    let pieces: Vec<_> = value.split('-').collect();
    pieces.len() == 3
        && pieces[0].len() == 4
        && pieces[1].len() == 2
        && pieces[2].len() == 2
        && pieces
            .iter()
            .all(|piece| piece.bytes().all(|byte| byte.is_ascii_digit()))
}

fn semantic_signature(kind: NodeKind, meaningful: &[usize], tokens: &[Token]) -> String {
    let mut signature = format!("{kind:?}\0");
    for index in meaningful {
        signature.push_str(&tokens[*index].lexeme);
        signature.push('\0');
    }
    signature
}

fn make_node_id(kind: NodeKind, signature: &str, ordinal: usize) -> NodeId {
    // Two independent FNV-style streams are intentionally boring and
    // dependency-free.  This is an editor identity, not a content-addressed
    // security hash; the semantic model must assign its own content hashes.
    let mut left = 0xcbf29ce484222325u64 ^ kind as u64;
    let mut right = 0x84222325cbf29ce4u64 ^ (ordinal as u64).rotate_left(17);
    for byte in signature
        .as_bytes()
        .iter()
        .copied()
        .chain((ordinal as u64).to_le_bytes())
    {
        left ^= u64::from(byte);
        left = left.wrapping_mul(0x100000001b3);
        right ^= u64::from(byte).rotate_left(3);
        right = right.wrapping_mul(0x100000001b3);
    }
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&left.to_le_bytes());
    bytes[8..].copy_from_slice(&right.to_le_bytes());
    NodeId(bytes)
}

fn canonical_source(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut offset = 0;
    while offset < source.len() {
        let line_start = offset;
        while offset < source.len() && !matches!(source.as_bytes()[offset], b'\r' | b'\n') {
            offset += 1;
        }
        let line = &source[line_start..offset];
        output.push_str(&canonical_line(line));
        if offset < source.len() {
            if source.as_bytes()[offset] == b'\r'
                && source.as_bytes().get(offset + 1) == Some(&b'\n')
            {
                output.push_str("\r\n");
                offset += 2;
            } else {
                output.push(source.as_bytes()[offset] as char);
                offset += 1;
            }
        }
    }
    if source.is_empty() {
        String::new()
    } else {
        output
    }
}

fn canonical_line(line: &str) -> String {
    if line.trim().is_empty() {
        return String::new();
    }
    let leading = line
        .as_bytes()
        .iter()
        .take_while(|byte| matches!(**byte, b' ' | b'\t'))
        .count();
    let tokens = lex(line);
    let meaningful = tokens
        .iter()
        .filter(|token| !matches!(token.kind, TokenKind::Whitespace | TokenKind::Newline))
        .collect::<Vec<_>>();
    if meaningful.is_empty() {
        return String::new();
    }
    if meaningful[0].kind == TokenKind::Comment {
        return meaningful[0].lexeme.clone();
    }
    let mut out = String::new();
    if leading > 0 {
        out.push_str("  ");
    }
    let mut previous = None;
    for token in meaningful {
        if token.kind == TokenKind::Comment {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&token.lexeme);
            break;
        }
        if let Some(previous) = previous
            && needs_space(previous, token)
            && !out.ends_with(' ')
        {
            out.push(' ');
        }
        out.push_str(&token.lexeme);
        previous = Some(token);
    }
    out
}

fn needs_space(left: &Token, right: &Token) -> bool {
    // Insert a separator whenever concatenating the exact spellings would
    // change the pair's lexical tokenization.  This is slightly more work than a
    // punctuation allow-list, but it matters for punctuation which is also a
    // legal name byte (`:` and `@`, in particular): `foo : bar` must not turn
    // into the single identifier `foo:bar` during canonical formatting.
    let mut joined = String::with_capacity(left.lexeme.len() + right.lexeme.len());
    joined.push_str(&left.lexeme);
    joined.push_str(&right.lexeme);
    let joined_tokens = lex(&joined);
    let same_tokens = joined_tokens.len() == 2
        && joined_tokens[0].lexeme == left.lexeme
        && joined_tokens[0].kind == left.kind
        && joined_tokens[1].lexeme == right.lexeme
        && joined_tokens[1].kind == right.kind;
    !same_tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_round_trip_keeps_all_trivia_and_unknown_bytes() {
        let source = "# heading\r\n\nbook tax-us\r\n  ?lot   _ USD  ???\r\n";
        let file = SurfaceFile::parse(source);
        assert_eq!(file.lossless(), source);
        assert_eq!(format_lossless(&file), source);
        assert_eq!(
            file.tokens()
                .iter()
                .map(|token| token.lexeme.as_str())
                .collect::<String>(),
            source
        );
        assert!(lossless_round_trip(source));
        assert!(
            file.tokens()
                .iter()
                .any(|token| token.kind == TokenKind::Comment)
        );
        assert!(file.tokens().iter().any(Token::is_hole));
        assert!(file.tokens().iter().any(|token| token.lexeme == "???"));
    }

    #[test]
    fn holes_are_typed_and_spelled_without_numeric_interpretation() {
        let file = SurfaceFile::parse("book _\nvalue ?lot\nvalue ?\n");
        let holes = file
            .tokens()
            .iter()
            .filter_map(Token::hole)
            .collect::<Vec<_>>();
        assert_eq!(
            holes,
            [
                &Hole::Anonymous,
                &Hole::Named("lot".to_owned()),
                &Hole::Anonymous
            ]
        );
        assert!(file.tokens().iter().any(|token| token.lexeme == "book"));
    }

    #[test]
    fn semantic_ids_ignore_comments_and_whitespace() {
        let compact =
            SurfaceFile::parse("book tax-us\nbuy one on 2026-01-01\n  1 ABC into checking\n");
        let decorated = SurfaceFile::parse(
            "# added\nbook   tax-us\n\nbuy one on 2026-01-01 ; note\n    1 ABC   into checking\n",
        );
        let first = compact
            .nodes()
            .iter()
            .map(|node| (node.kind, node.id))
            .collect::<Vec<_>>();
        let second = decorated
            .nodes()
            .iter()
            .filter(|node| node.kind != NodeKind::Unknown)
            .map(|node| (node.kind, node.id))
            .collect::<Vec<_>>();
        assert_eq!(first, second);
    }

    #[test]
    fn malformed_known_forms_recover_as_error_nodes() {
        let source = "book\nbuy one on 2026-01-01\n  1 ABC\nwat ???\n# still here\n";
        let file = SurfaceFile::parse(source);
        assert_eq!(file.lossless(), source);
        assert_eq!(file.error_nodes().count(), 2);
        assert_eq!(file.errors().count(), 2);
        assert!(
            file.diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.message.contains("incomplete `book`"))
        );
        assert!(
            file.diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.message.contains("incomplete `buy`"))
        );
        assert!(
            file.nodes()
                .iter()
                .any(|node| node.kind == NodeKind::Unknown)
        );
    }

    #[test]
    fn canonical_format_is_compact_but_preserves_holes_comments_and_unknowns() {
        let source = "2026-09-20   Groceries\n  Expenses:Food       48.32 USD\n  Assets:Checking    -48.32 USD # keep\n  ???   ?lot\n";
        let canonical = canonical_format(source);
        assert_eq!(
            canonical,
            "2026-09-20 Groceries\n  Expenses:Food 48.32 USD\n  Assets:Checking -48.32 USD # keep\n  ??? ?lot\n"
        );
        assert!(canonical_round_trip(source));
    }

    #[test]
    fn canonical_format_preserves_lexical_boundaries_and_unicode_unknowns() {
        let source = "value foo : bar @ baz\n\u{00a0}???\n";
        let canonical = canonical_format(source);
        assert_eq!(canonical, "value foo : bar @ baz\n\u{00a0}???\n");
        assert_eq!(canonical_format(&canonical), canonical);
        assert!(canonical_round_trip(source));
    }

    #[test]
    fn ontology_headers_are_forms_and_incomplete_blocks_recover() {
        let source = "book tax-us\nobligation invoice\n  debtor alice\nsettlement payment\n  state issued\nsatisfy allocation\n";
        let file = SurfaceFile::parse(source);
        assert_eq!(
            file.nodes()
                .iter()
                .map(|node| node.kind)
                .collect::<Vec<_>>(),
            [
                NodeKind::Form,
                NodeKind::Error,
                NodeKind::Error,
                NodeKind::Error
            ]
        );
        assert_eq!(file.error_nodes().count(), 3);
        assert!(
            file.errors()
                .any(|diagnostic| diagnostic.message.contains("incomplete `obligation`"))
        );
        assert!(
            file.errors()
                .any(|diagnostic| diagnostic.message.contains("incomplete `settlement`"))
        );
        assert!(
            file.errors()
                .any(|diagnostic| diagnostic.message.contains("incomplete `satisfy`"))
        );
    }

    #[test]
    fn spans_are_byte_stable_and_point_at_source_text() {
        let source = "book tax-us\n\nobserve ?row\n";
        let file = SurfaceFile::parse(source);
        let node = file.nodes().last();
        assert_eq!(
            node.and_then(|node| node.span.text(source)),
            Some("observe ?row")
        );
        assert_eq!(node.map(|node| node.span.line), Some(3));
        assert_eq!(node.map(|node| node.span.column), Some(1));
        assert_eq!(
            file.tokens()
                .iter()
                .find(|token| token.is_hole())
                .map(|token| token.lexeme.as_str()),
            Some("?row")
        );
    }

    #[test]
    fn generic_forms_expose_lossless_header_and_repeated_field_occurrences() {
        let source = "form invoice/1 : billing::Invoice ; header\n  amount  100 USD\n  state issued\n  state settled\n  ??? raw ?amount\n\nbook tax-us\n";
        let file = SurfaceFile::parse(source);
        assert_eq!(file.nodes()[0].kind, NodeKind::Form);
        let form = file.forms().next().expect("generic form");
        assert_eq!(
            form.occurrence().map(|token| token.lexeme.as_str()),
            Some("invoice/1")
        );
        assert_eq!(
            form.schema_parts()
                .map(|token| token.lexeme.as_str())
                .collect::<Vec<_>>(),
            ["billing::Invoice"]
        );
        let fields = form.fields().collect::<Vec<_>>();
        assert_eq!(fields.len(), 4);
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name().map(|token| token.lexeme.as_str()))
                .collect::<Vec<_>>(),
            [Some("amount"), Some("state"), Some("state"), None]
        );
        assert_eq!(
            fields[1]
                .value_parts()
                .map(|token| token.lexeme.as_str())
                .collect::<Vec<_>>(),
            ["issued"]
        );
        assert_eq!(fields[2].span().text(source), Some("  state settled"));
        assert_eq!(
            fields[3]
                .tokens()
                .iter()
                .map(|token| token.lexeme.as_str())
                .collect::<String>(),
            "  ??? raw ?amount"
        );
        assert!(fields[3].value_tokens().is_empty());
        assert_eq!(file.lossless(), source);
        assert_eq!(
            canonical_format(source),
            canonical_format(&file.canonical())
        );
    }

    #[test]
    fn incomplete_generic_form_header_recovers_without_dropping_field_bytes() {
        let source = "form invoice/1\n  amount 100 USD\n";
        let file = SurfaceFile::parse(source);
        assert_eq!(file.nodes()[0].kind, NodeKind::Error);
        let recovered = file.forms().next().expect("recoverable form view");
        assert_eq!(recovered.fields().count(), 1);
        assert_eq!(
            file.nodes()[0].span.text(source),
            Some("form invoice/1\n  amount 100 USD")
        );
        assert_eq!(file.lossless(), source);
        assert_eq!(
            file.tokens()
                .iter()
                .map(|token| token.lexeme.as_str())
                .collect::<String>(),
            source
        );
        assert!(
            file.errors()
                .any(|diagnostic| diagnostic.message.contains("incomplete `form`"))
        );
    }

    #[test]
    fn generic_form_header_rejects_tokens_between_occurrence_and_schema_delimiter() {
        let source = "form invoice/1 extra : billing::Invoice\n  amount 100 USD\n";
        let file = SurfaceFile::parse(source);
        assert_eq!(file.nodes()[0].kind, NodeKind::Error);
        assert_eq!(file.lossless(), source);
        assert_eq!(file.nodes()[0].span.text(source), Some(source.trim_end()));
        let diagnostic = file.errors().next().expect("header diagnostic");
        assert_eq!(diagnostic.span.line, 1);
        assert!(diagnostic.message.contains("incomplete `form`"));
    }

    #[test]
    fn generic_form_delimiter_spacing_is_semantically_equivalent() {
        for header in [
            "form invoice/1 : billing::Invoice",
            "form invoice/1: billing::Invoice",
            "form invoice/1 :billing::Invoice",
            "form invoice/1:billing::Invoice",
        ] {
            let file = SurfaceFile::parse(format!("{header}\n  amount 1\n"));
            let form = file.forms().next().expect("generic form");
            assert_eq!(form.node().kind, NodeKind::Form, "{header}");
            assert_eq!(
                form.schema_parts()
                    .map(|token| token.lexeme.as_str())
                    .collect::<String>(),
                "billing::Invoice",
                "{header}"
            );
        }
    }

    #[test]
    fn generic_form_fields_skip_trivia_only_lines_consistently() {
        let file = SurfaceFile::parse(
            "form invoice/1 : billing::Invoice\n  ; explanation\n  amount 1\n  # note\n  state issued\n",
        );
        assert_eq!(
            file.forms()
                .next()
                .unwrap()
                .fields()
                .filter_map(|field| field.name().map(|token| token.lexeme.as_str()))
                .collect::<Vec<_>>(),
            ["amount", "state"]
        );
    }
}
