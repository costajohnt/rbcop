//! libyaml's parser (parser.c): tokens to events.

use super::scanner::{Scanner, TokenKind};
use super::{Error, Mark, ScalarStyle};

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    StreamStart,
    StreamEnd,
    DocumentStart,
    DocumentEnd,
    Alias(String),
    Scalar {
        anchor: Option<String>,
        tag: Option<String>,
        value: String,
        plain_implicit: bool,
        quoted_implicit: bool,
        style: ScalarStyle,
    },
    SequenceStart {
        anchor: Option<String>,
        tag: Option<String>,
    },
    SequenceEnd,
    MappingStart {
        anchor: Option<String>,
        tag: Option<String>,
    },
    MappingEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    StreamStart,
    ImplicitDocumentStart,
    DocumentStart,
    DocumentContent,
    DocumentEnd,
    BlockNode,
    BlockSequenceFirstEntry,
    BlockSequenceEntry,
    IndentlessSequenceEntry,
    BlockMappingFirstKey,
    BlockMappingKey,
    BlockMappingValue,
    FlowSequenceFirstEntry,
    FlowSequenceEntry,
    FlowSequenceEntryMappingKey,
    FlowSequenceEntryMappingValue,
    FlowSequenceEntryMappingEnd,
    FlowMappingFirstKey,
    FlowMappingKey,
    FlowMappingValue,
    FlowMappingEmptyValue,
    End,
}

type R<T = Event> = Result<T, Error>;

pub struct Parser {
    scanner: Scanner,
    state: State,
    states: Vec<State>,
    marks: Vec<Mark>,
    tag_directives: Vec<(String, String)>,
}

fn parser_error(problem: &'static str) -> Error {
    Error { problem, context: None, context_mark: Mark::default() }
}

fn context_error(context: &'static str, context_mark: Mark, problem: &'static str) -> Error {
    Error { problem, context: Some(context), context_mark }
}

fn empty_scalar() -> Event {
    Event::Scalar {
        anchor: None,
        tag: None,
        value: String::new(),
        plain_implicit: true,
        quoted_implicit: false,
        style: ScalarStyle::Plain,
    }
}

impl Parser {
    pub fn new(text: &[u8]) -> Parser {
        Parser {
            scanner: Scanner::new(text),
            state: State::StreamStart,
            states: Vec::new(),
            marks: Vec::new(),
            tag_directives: Vec::new(),
        }
    }

    /// The next event; StreamEnd again once the stream has ended.
    pub fn next_event(&mut self) -> R {
        if self.scanner.stream_end_produced || self.state == State::End {
            return Ok(Event::StreamEnd);
        }
        match self.state {
            State::StreamStart => self.parse_stream_start(),
            State::ImplicitDocumentStart => self.parse_document_start(true),
            State::DocumentStart => self.parse_document_start(false),
            State::DocumentContent => self.parse_document_content(),
            State::DocumentEnd => self.parse_document_end(),
            State::BlockNode => self.parse_node(true, false),
            State::BlockSequenceFirstEntry => self.parse_block_sequence_entry(true),
            State::BlockSequenceEntry => self.parse_block_sequence_entry(false),
            State::IndentlessSequenceEntry => self.parse_indentless_sequence_entry(),
            State::BlockMappingFirstKey => self.parse_block_mapping_key(true),
            State::BlockMappingKey => self.parse_block_mapping_key(false),
            State::BlockMappingValue => self.parse_block_mapping_value(),
            State::FlowSequenceFirstEntry => self.parse_flow_sequence_entry(true),
            State::FlowSequenceEntry => self.parse_flow_sequence_entry(false),
            State::FlowSequenceEntryMappingKey => self.parse_flow_sequence_entry_mapping_key(),
            State::FlowSequenceEntryMappingValue => self.parse_flow_sequence_entry_mapping_value(),
            State::FlowSequenceEntryMappingEnd => {
                self.scanner.peek()?;
                self.state = State::FlowSequenceEntry;
                Ok(Event::MappingEnd)
            }
            State::FlowMappingFirstKey => self.parse_flow_mapping_key(true),
            State::FlowMappingKey => self.parse_flow_mapping_key(false),
            State::FlowMappingValue => self.parse_flow_mapping_value(false),
            State::FlowMappingEmptyValue => self.parse_flow_mapping_value(true),
            State::End => unreachable!(),
        }
    }

    fn peek_kind(&mut self) -> R<TokenKind> {
        Ok(self.scanner.peek()?.kind.clone())
    }

    fn peek_start(&mut self) -> R<Mark> {
        Ok(self.scanner.peek()?.start)
    }

    fn pop_state(&mut self) -> State {
        self.states.pop().unwrap_or(State::End)
    }

    fn parse_stream_start(&mut self) -> R {
        if self.peek_kind()? != TokenKind::StreamStart {
            return Err(parser_error("did not find expected <stream-start>"));
        }
        self.state = State::ImplicitDocumentStart;
        self.scanner.skip_token();
        Ok(Event::StreamStart)
    }

    fn parse_document_start(&mut self, implicit: bool) -> R {
        let mut kind = self.peek_kind()?;
        if !implicit {
            while kind == TokenKind::DocumentEnd {
                self.scanner.skip_token();
                kind = self.peek_kind()?;
            }
        }
        let directive = matches!(kind, TokenKind::VersionDirective(..) | TokenKind::TagDirective(..));
        if implicit && !directive && kind != TokenKind::DocumentStart && kind != TokenKind::StreamEnd {
            self.process_directives()?;
            self.states.push(State::DocumentEnd);
            self.state = State::BlockNode;
            Ok(Event::DocumentStart)
        } else if kind != TokenKind::StreamEnd {
            self.process_directives()?;
            if self.peek_kind()? != TokenKind::DocumentStart {
                return Err(parser_error("did not find expected <document start>"));
            }
            self.states.push(State::DocumentEnd);
            self.state = State::DocumentContent;
            self.scanner.skip_token();
            Ok(Event::DocumentStart)
        } else {
            self.state = State::End;
            self.scanner.skip_token();
            Ok(Event::StreamEnd)
        }
    }

    fn parse_document_content(&mut self) -> R {
        match self.peek_kind()? {
            TokenKind::VersionDirective(..)
            | TokenKind::TagDirective(..)
            | TokenKind::DocumentStart
            | TokenKind::DocumentEnd
            | TokenKind::StreamEnd => {
                self.state = self.pop_state();
                Ok(empty_scalar())
            }
            _ => self.parse_node(true, false),
        }
    }

    fn parse_document_end(&mut self) -> R {
        if self.peek_kind()? == TokenKind::DocumentEnd {
            self.scanner.skip_token();
        }
        self.tag_directives.clear();
        self.state = State::DocumentStart;
        Ok(Event::DocumentEnd)
    }

    fn parse_node(&mut self, block: bool, indentless_sequence: bool) -> R {
        let token = self.scanner.peek()?;
        if let TokenKind::Alias(a) = &token.kind {
            let a = a.clone();
            self.state = self.pop_state();
            self.scanner.skip_token();
            return Ok(Event::Alias(a));
        }
        let start_mark = token.start;
        let mut anchor = None;
        let mut tag_parts = None;
        match token.kind.clone() {
            TokenKind::Anchor(a) => {
                anchor = Some(a);
                self.scanner.skip_token();
                if let TokenKind::Tag(h, s) = self.peek_kind()? {
                    tag_parts = Some((h, s));
                    self.scanner.skip_token();
                }
            }
            TokenKind::Tag(h, s) => {
                tag_parts = Some((h, s));
                self.scanner.skip_token();
                if let TokenKind::Anchor(a) = self.peek_kind()? {
                    anchor = Some(a);
                    self.scanner.skip_token();
                }
            }
            _ => {}
        }
        let tag = match tag_parts {
            None => None,
            Some((h, s)) if h.is_empty() => Some(s),
            Some((h, s)) => match self.tag_directives.iter().find(|(handle, _)| *handle == h) {
                Some((_, prefix)) => Some(format!("{prefix}{s}")),
                None => return Err(context_error("while parsing a node", start_mark, "found undefined tag handle")),
            },
        };
        let kind = self.peek_kind()?;
        if indentless_sequence && kind == TokenKind::BlockEntry {
            self.state = State::IndentlessSequenceEntry;
            return Ok(Event::SequenceStart { anchor, tag });
        }
        match kind {
            TokenKind::Scalar(value, style) => {
                let plain_implicit = (style == ScalarStyle::Plain && tag.is_none()) || tag.as_deref() == Some("!");
                let quoted_implicit = !plain_implicit && tag.is_none();
                self.state = self.pop_state();
                self.scanner.skip_token();
                Ok(Event::Scalar { anchor, tag, value, plain_implicit, quoted_implicit, style })
            }
            TokenKind::FlowSequenceStart => {
                self.state = State::FlowSequenceFirstEntry;
                Ok(Event::SequenceStart { anchor, tag })
            }
            TokenKind::FlowMappingStart => {
                self.state = State::FlowMappingFirstKey;
                Ok(Event::MappingStart { anchor, tag })
            }
            TokenKind::BlockSequenceStart if block => {
                self.state = State::BlockSequenceFirstEntry;
                Ok(Event::SequenceStart { anchor, tag })
            }
            TokenKind::BlockMappingStart if block => {
                self.state = State::BlockMappingFirstKey;
                Ok(Event::MappingStart { anchor, tag })
            }
            _ if anchor.is_some() || tag.is_some() => {
                self.state = self.pop_state();
                let plain_implicit = tag.as_deref().is_none_or(str::is_empty);
                Ok(Event::Scalar {
                    anchor,
                    tag,
                    value: String::new(),
                    plain_implicit,
                    quoted_implicit: false,
                    style: ScalarStyle::Plain,
                })
            }
            _ => {
                let context = if block { "while parsing a block node" } else { "while parsing a flow node" };
                Err(context_error(context, start_mark, "did not find expected node content"))
            }
        }
    }

    /// The `first` step of a collection: remember its start, skip its start token.
    fn first(&mut self) -> R<()> {
        let start = self.peek_start()?;
        self.marks.push(start);
        self.scanner.skip_token();
        Ok(())
    }

    fn pop_mark(&mut self) -> Mark {
        self.marks.pop().unwrap_or_default()
    }

    fn parse_block_sequence_entry(&mut self, first: bool) -> R {
        if first {
            self.first()?;
        }
        match self.peek_kind()? {
            TokenKind::BlockEntry => {
                self.scanner.skip_token();
                if matches!(self.peek_kind()?, TokenKind::BlockEntry | TokenKind::BlockEnd) {
                    self.state = State::BlockSequenceEntry;
                    Ok(empty_scalar())
                } else {
                    self.states.push(State::BlockSequenceEntry);
                    self.parse_node(true, false)
                }
            }
            TokenKind::BlockEnd => {
                self.state = self.pop_state();
                self.pop_mark();
                self.scanner.skip_token();
                Ok(Event::SequenceEnd)
            }
            _ => {
                let mark = self.pop_mark();
                Err(context_error("while parsing a block collection", mark, "did not find expected '-' indicator"))
            }
        }
    }

    fn parse_indentless_sequence_entry(&mut self) -> R {
        if self.peek_kind()? == TokenKind::BlockEntry {
            self.scanner.skip_token();
            if matches!(
                self.peek_kind()?,
                TokenKind::BlockEntry | TokenKind::Key | TokenKind::Value | TokenKind::BlockEnd
            ) {
                self.state = State::IndentlessSequenceEntry;
                Ok(empty_scalar())
            } else {
                self.states.push(State::IndentlessSequenceEntry);
                self.parse_node(true, false)
            }
        } else {
            self.state = self.pop_state();
            Ok(Event::SequenceEnd)
        }
    }

    fn parse_block_mapping_key(&mut self, first: bool) -> R {
        if first {
            self.first()?;
        }
        match self.peek_kind()? {
            TokenKind::Key => {
                self.scanner.skip_token();
                if matches!(self.peek_kind()?, TokenKind::Key | TokenKind::Value | TokenKind::BlockEnd) {
                    self.state = State::BlockMappingValue;
                    Ok(empty_scalar())
                } else {
                    self.states.push(State::BlockMappingValue);
                    self.parse_node(true, true)
                }
            }
            TokenKind::BlockEnd => {
                self.state = self.pop_state();
                self.pop_mark();
                self.scanner.skip_token();
                Ok(Event::MappingEnd)
            }
            _ => {
                let mark = self.pop_mark();
                Err(context_error("while parsing a block mapping", mark, "did not find expected key"))
            }
        }
    }

    fn parse_block_mapping_value(&mut self) -> R {
        if self.peek_kind()? == TokenKind::Value {
            self.scanner.skip_token();
            if matches!(self.peek_kind()?, TokenKind::Key | TokenKind::Value | TokenKind::BlockEnd) {
                self.state = State::BlockMappingKey;
                Ok(empty_scalar())
            } else {
                self.states.push(State::BlockMappingKey);
                self.parse_node(true, true)
            }
        } else {
            self.state = State::BlockMappingKey;
            Ok(empty_scalar())
        }
    }

    fn parse_flow_sequence_entry(&mut self, first: bool) -> R {
        if first {
            self.first()?;
        }
        let mut kind = self.peek_kind()?;
        if kind != TokenKind::FlowSequenceEnd {
            if !first {
                if kind == TokenKind::FlowEntry {
                    self.scanner.skip_token();
                    kind = self.peek_kind()?;
                } else {
                    let mark = self.pop_mark();
                    return Err(context_error(
                        "while parsing a flow sequence",
                        mark,
                        "did not find expected ',' or ']'",
                    ));
                }
            }
            if kind == TokenKind::Key {
                self.state = State::FlowSequenceEntryMappingKey;
                self.scanner.skip_token();
                return Ok(Event::MappingStart { anchor: None, tag: None });
            } else if kind != TokenKind::FlowSequenceEnd {
                self.states.push(State::FlowSequenceEntry);
                return self.parse_node(false, false);
            }
        }
        self.state = self.pop_state();
        self.pop_mark();
        self.scanner.skip_token();
        Ok(Event::SequenceEnd)
    }

    fn parse_flow_sequence_entry_mapping_key(&mut self) -> R {
        if matches!(self.peek_kind()?, TokenKind::Value | TokenKind::FlowEntry | TokenKind::FlowSequenceEnd) {
            self.scanner.skip_token();
            self.state = State::FlowSequenceEntryMappingValue;
            Ok(empty_scalar())
        } else {
            self.states.push(State::FlowSequenceEntryMappingValue);
            self.parse_node(false, false)
        }
    }

    fn parse_flow_sequence_entry_mapping_value(&mut self) -> R {
        if self.peek_kind()? == TokenKind::Value {
            self.scanner.skip_token();
            if !matches!(self.peek_kind()?, TokenKind::FlowEntry | TokenKind::FlowSequenceEnd) {
                self.states.push(State::FlowSequenceEntryMappingEnd);
                return self.parse_node(false, false);
            }
        }
        self.state = State::FlowSequenceEntryMappingEnd;
        Ok(empty_scalar())
    }

    fn parse_flow_mapping_key(&mut self, first: bool) -> R {
        if first {
            self.first()?;
        }
        let mut kind = self.peek_kind()?;
        if kind != TokenKind::FlowMappingEnd {
            if !first {
                if kind == TokenKind::FlowEntry {
                    self.scanner.skip_token();
                    kind = self.peek_kind()?;
                } else {
                    let mark = self.pop_mark();
                    return Err(context_error(
                        "while parsing a flow mapping",
                        mark,
                        "did not find expected ',' or '}'",
                    ));
                }
            }
            if kind == TokenKind::Key {
                self.scanner.skip_token();
                if matches!(self.peek_kind()?, TokenKind::Value | TokenKind::FlowEntry | TokenKind::FlowMappingEnd) {
                    self.state = State::FlowMappingValue;
                    return Ok(empty_scalar());
                }
                self.states.push(State::FlowMappingValue);
                return self.parse_node(false, false);
            } else if kind != TokenKind::FlowMappingEnd {
                self.states.push(State::FlowMappingEmptyValue);
                return self.parse_node(false, false);
            }
        }
        self.state = self.pop_state();
        self.pop_mark();
        self.scanner.skip_token();
        Ok(Event::MappingEnd)
    }

    fn parse_flow_mapping_value(&mut self, empty: bool) -> R {
        let kind = self.peek_kind()?;
        if empty {
            self.state = State::FlowMappingKey;
            return Ok(empty_scalar());
        }
        if kind == TokenKind::Value {
            self.scanner.skip_token();
            if !matches!(self.peek_kind()?, TokenKind::FlowEntry | TokenKind::FlowMappingEnd) {
                self.states.push(State::FlowMappingKey);
                return self.parse_node(false, false);
            }
        }
        self.state = State::FlowMappingKey;
        Ok(empty_scalar())
    }

    fn process_directives(&mut self) -> R<()> {
        let mut version = false;
        loop {
            match self.peek_kind()? {
                TokenKind::VersionDirective(major, minor) => {
                    if version {
                        return Err(parser_error("found duplicate %YAML directive"));
                    }
                    if major != 1 || (minor != 1 && minor != 2) {
                        return Err(parser_error("found incompatible YAML document"));
                    }
                    version = true;
                }
                TokenKind::TagDirective(handle, prefix) => {
                    if self.tag_directives.iter().any(|(h, _)| *h == handle) {
                        return Err(parser_error("found duplicate %TAG directive"));
                    }
                    self.tag_directives.push((handle, prefix));
                }
                _ => break,
            }
            self.scanner.skip_token();
        }
        for (handle, prefix) in [("!", "!"), ("!!", "tag:yaml.org,2002:")] {
            if !self.tag_directives.iter().any(|(h, _)| h == handle) {
                self.tag_directives.push((handle.into(), prefix.into()));
            }
        }
        Ok(())
    }
}
