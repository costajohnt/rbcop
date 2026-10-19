//! libyaml's emitter (emitter.c) as Psych sets it up (unicode, 2-space indent, 80 columns, LF),
//! for the events Psych's YAMLTree produces: anchors on collections and aliases, no %YAML/%TAG
//! directives.

use super::ScalarStyle;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    StreamStart,
    StreamEnd,
    DocumentStart {
        implicit: bool,
    },
    DocumentEnd {
        implicit: bool,
    },
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
        implicit: bool,
        flow: bool,
    },
    SequenceEnd,
    MappingStart {
        anchor: Option<String>,
        tag: Option<String>,
        implicit: bool,
        flow: bool,
    },
    MappingEnd,
    Alias {
        anchor: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    StreamStart,
    FirstDocumentStart,
    DocumentStart,
    DocumentContent,
    DocumentEnd,
    FlowSequenceFirstItem,
    FlowSequenceItem,
    FlowMappingFirstKey,
    FlowMappingKey,
    FlowMappingSimpleValue,
    FlowMappingValue,
    BlockSequenceFirstItem,
    BlockSequenceItem,
    BlockMappingFirstKey,
    BlockMappingKey,
    BlockMappingSimpleValue,
    BlockMappingValue,
    End,
}

const BEST_INDENT: isize = 2;
const BEST_WIDTH: usize = 80;

/// yaml_emitter_analyze_scalar's findings.
#[derive(Default)]
struct Analysis {
    chars: Vec<char>,
    multiline: bool,
    flow_plain_allowed: bool,
    block_plain_allowed: bool,
    single_quoted_allowed: bool,
    block_allowed: bool,
    style: Option<ScalarStyle>,
}

struct Emitter<'a> {
    events: &'a [Event],
    head: usize,
    out: String,
    state: State,
    states: Vec<State>,
    indent: isize,
    indents: Vec<isize>,
    flow_level: usize,
    mapping_context: bool,
    simple_key_context: bool,
    column: usize,
    whitespace: bool,
    indention: bool,
    open_ended: u8,
    tag_handle: Option<String>,
    tag_suffix: Option<String>,
    /// The event's anchor, and whether it is an alias (`*`) rather than an anchor (`&`).
    anchor: Option<(String, bool)>,
    scalar: Analysis,
}

/// The text of the events, as yaml_emitter_emit writes it.
pub fn emit(events: &[Event]) -> String {
    let mut e = Emitter {
        events,
        head: 0,
        out: String::new(),
        state: State::StreamStart,
        states: Vec::new(),
        indent: -1,
        indents: Vec::new(),
        flow_level: 0,
        mapping_context: false,
        simple_key_context: false,
        column: 0,
        whitespace: true,
        indention: true,
        open_ended: 0,
        tag_handle: None,
        tag_suffix: None,
        anchor: None,
        scalar: Analysis::default(),
    };
    while e.head < events.len() {
        e.analyze_event();
        e.state_machine();
        e.head += 1;
    }
    e.out
}

/// IS_PRINTABLE: what libyaml writes unescaped (no C0/C1 controls but LF, no surrogates, BOM,
/// U+FFFE/U+FFFF, nor anything past the BMP).
fn is_printable(c: char) -> bool {
    matches!(c as u32, 0x0A | 0x20..=0x7E | 0xA0..=0xD7FF | 0xE000..=0xFEFE | 0xFF00..=0xFFFD)
}

fn is_break(c: char) -> bool {
    matches!(c, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn is_blankz(c: Option<&char>) -> bool {
    c.is_none_or(|&c| matches!(c, ' ' | '\t' | '\0') || is_break(c))
}

fn is_alpha(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

impl Emitter<'_> {
    fn event(&self) -> &Event {
        &self.events[self.head]
    }

    fn pop_state(&mut self) {
        self.state = self.states.pop().unwrap_or(State::End);
    }

    fn pop_indent(&mut self) {
        self.indent = self.indents.pop().unwrap_or(-1);
    }

    fn increase_indent(&mut self, flow: bool, indentless: bool) {
        self.indents.push(self.indent);
        if self.indent < 0 {
            self.indent = if flow { BEST_INDENT } else { 0 };
        } else if !indentless {
            self.indent += BEST_INDENT;
        }
    }

    fn state_machine(&mut self) {
        match self.state {
            State::StreamStart => self.state = State::FirstDocumentStart,
            State::FirstDocumentStart => self.emit_document_start(true),
            State::DocumentStart => self.emit_document_start(false),
            State::DocumentContent => {
                self.states.push(State::DocumentEnd);
                self.emit_node(false, false, false);
            }
            State::DocumentEnd => self.emit_document_end(),
            State::FlowSequenceFirstItem => self.emit_flow_sequence_item(true),
            State::FlowSequenceItem => self.emit_flow_sequence_item(false),
            State::FlowMappingFirstKey => self.emit_flow_mapping_key(true),
            State::FlowMappingKey => self.emit_flow_mapping_key(false),
            State::FlowMappingSimpleValue => self.emit_flow_mapping_value(true),
            State::FlowMappingValue => self.emit_flow_mapping_value(false),
            State::BlockSequenceFirstItem => self.emit_block_sequence_item(true),
            State::BlockSequenceItem => self.emit_block_sequence_item(false),
            State::BlockMappingFirstKey => self.emit_block_mapping_key(true),
            State::BlockMappingKey => self.emit_block_mapping_key(false),
            State::BlockMappingSimpleValue => self.emit_block_mapping_value(true),
            State::BlockMappingValue => self.emit_block_mapping_value(false),
            State::End => {}
        }
    }

    fn emit_document_start(&mut self, first: bool) {
        match *self.event() {
            Event::DocumentStart { implicit } => {
                let implicit = implicit && first;
                self.open_ended = 0;
                if !implicit {
                    self.write_indent();
                    self.write_indicator("---", true, false, false);
                }
                self.state = State::DocumentContent;
                self.open_ended = 0;
            }
            Event::StreamEnd => {
                if self.open_ended == 2 {
                    self.write_indicator("...", true, false, false);
                    self.open_ended = 0;
                    self.write_indent();
                }
                self.state = State::End;
            }
            _ => {}
        }
    }

    fn emit_document_end(&mut self) {
        if let Event::DocumentEnd { implicit } = *self.event() {
            self.write_indent();
            if !implicit {
                self.write_indicator("...", true, false, false);
                self.open_ended = 0;
                self.write_indent();
            } else if self.open_ended == 0 {
                self.open_ended = 1;
            }
            self.state = State::DocumentStart;
        }
    }

    fn emit_flow_sequence_item(&mut self, first: bool) {
        if first {
            self.write_indicator("[", true, true, false);
            self.increase_indent(true, false);
            self.flow_level += 1;
        }
        if *self.event() == Event::SequenceEnd {
            self.flow_level -= 1;
            self.pop_indent();
            self.write_indicator("]", false, false, false);
            self.pop_state();
            return;
        }
        if !first {
            self.write_indicator(",", false, false, false);
        }
        if self.column > BEST_WIDTH {
            self.write_indent();
        }
        self.states.push(State::FlowSequenceItem);
        self.emit_node(true, false, false);
    }

    fn emit_flow_mapping_key(&mut self, first: bool) {
        if first {
            self.write_indicator("{", true, true, false);
            self.increase_indent(true, false);
            self.flow_level += 1;
        }
        if *self.event() == Event::MappingEnd {
            self.flow_level -= 1;
            self.pop_indent();
            self.write_indicator("}", false, false, false);
            self.pop_state();
            return;
        }
        if !first {
            self.write_indicator(",", false, false, false);
        }
        if self.column > BEST_WIDTH {
            self.write_indent();
        }
        if self.check_simple_key() {
            self.states.push(State::FlowMappingSimpleValue);
            self.emit_node(false, true, true);
        } else {
            self.write_indicator("?", true, false, false);
            self.states.push(State::FlowMappingValue);
            self.emit_node(false, true, false);
        }
    }

    fn emit_flow_mapping_value(&mut self, simple: bool) {
        if simple {
            self.write_indicator(":", false, false, false);
        } else {
            if self.column > BEST_WIDTH {
                self.write_indent();
            }
            self.write_indicator(":", true, false, false);
        }
        self.states.push(State::FlowMappingKey);
        self.emit_node(false, true, false);
    }

    fn emit_block_sequence_item(&mut self, first: bool) {
        if first {
            let indentless = self.mapping_context && !self.indention;
            self.increase_indent(false, indentless);
        }
        if *self.event() == Event::SequenceEnd {
            self.pop_indent();
            self.pop_state();
            return;
        }
        self.write_indent();
        self.write_indicator("-", true, false, true);
        self.states.push(State::BlockSequenceItem);
        self.emit_node(true, false, false);
    }

    fn emit_block_mapping_key(&mut self, first: bool) {
        if first {
            self.increase_indent(false, false);
        }
        if *self.event() == Event::MappingEnd {
            self.pop_indent();
            self.pop_state();
            return;
        }
        self.write_indent();
        if self.check_simple_key() {
            self.states.push(State::BlockMappingSimpleValue);
            self.emit_node(false, true, true);
        } else {
            self.write_indicator("?", true, false, true);
            self.states.push(State::BlockMappingValue);
            self.emit_node(false, true, false);
        }
    }

    fn emit_block_mapping_value(&mut self, simple: bool) {
        if simple {
            self.write_indicator(":", false, false, false);
        } else {
            self.write_indent();
            self.write_indicator(":", true, false, true);
        }
        self.states.push(State::BlockMappingKey);
        self.emit_node(false, true, false);
    }

    fn emit_node(&mut self, _sequence: bool, mapping: bool, simple_key: bool) {
        self.mapping_context = mapping;
        self.simple_key_context = simple_key;
        match self.event() {
            Event::Alias { .. } => {
                self.process_anchor();
                if self.simple_key_context {
                    self.put(' ');
                }
                self.pop_state();
            }
            Event::Scalar { .. } => {
                self.select_scalar_style();
                self.process_anchor();
                self.process_tag();
                self.increase_indent(true, false);
                self.process_scalar();
                self.pop_indent();
                self.pop_state();
            }
            Event::SequenceStart { flow, .. } => {
                let flow = *flow;
                self.process_anchor();
                self.process_tag();
                self.state = if self.flow_level > 0 || flow || self.check_empty(&Event::SequenceEnd) {
                    State::FlowSequenceFirstItem
                } else {
                    State::BlockSequenceFirstItem
                };
            }
            Event::MappingStart { flow, .. } => {
                let flow = *flow;
                self.process_anchor();
                self.process_tag();
                self.state = if self.flow_level > 0 || flow || self.check_empty(&Event::MappingEnd) {
                    State::FlowMappingFirstKey
                } else {
                    State::BlockMappingFirstKey
                };
            }
            _ => {}
        }
    }

    fn check_empty(&self, end: &Event) -> bool {
        self.events.get(self.head + 1) == Some(end)
    }

    /// Simple keys are single-line and at most 128 bytes with their tag.
    fn check_simple_key(&self) -> bool {
        let tag_len = self.tag_handle.as_ref().map_or(0, String::len)
            + self.tag_suffix.as_ref().map_or(0, String::len)
            + self.anchor.as_ref().map_or(0, |(a, _)| a.len());
        let length = match self.event() {
            Event::Alias { .. } => tag_len,
            Event::Scalar { value, .. } => {
                if self.scalar.multiline {
                    return false;
                }
                tag_len + value.len()
            }
            Event::SequenceStart { .. } if self.check_empty(&Event::SequenceEnd) => tag_len,
            Event::MappingStart { .. } if self.check_empty(&Event::MappingEnd) => tag_len,
            _ => return false,
        };
        length <= 128
    }

    fn select_scalar_style(&mut self) {
        let Event::Scalar { plain_implicit, quoted_implicit, style, .. } = *self.event() else { return };
        let no_tag = self.tag_handle.is_none() && self.tag_suffix.is_none();
        let mut style = if style == ScalarStyle::Any { ScalarStyle::Plain } else { style };
        let a = &self.scalar;
        if self.simple_key_context && a.multiline {
            style = ScalarStyle::DoubleQuoted;
        }
        if style == ScalarStyle::Plain {
            if (self.flow_level > 0 && !a.flow_plain_allowed) || (self.flow_level == 0 && !a.block_plain_allowed) {
                style = ScalarStyle::SingleQuoted;
            }
            if a.chars.is_empty() && (self.flow_level > 0 || self.simple_key_context) {
                style = ScalarStyle::SingleQuoted;
            }
            if no_tag && !plain_implicit {
                style = ScalarStyle::SingleQuoted;
            }
        }
        if style == ScalarStyle::SingleQuoted && !a.single_quoted_allowed {
            style = ScalarStyle::DoubleQuoted;
        }
        if matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
            && (!a.block_allowed || self.flow_level > 0 || self.simple_key_context)
        {
            style = ScalarStyle::DoubleQuoted;
        }
        if no_tag && !quoted_implicit && style != ScalarStyle::Plain {
            self.tag_handle = Some("!".into());
        }
        self.scalar.style = Some(style);
    }

    fn analyze_event(&mut self) {
        self.tag_handle = None;
        self.tag_suffix = None;
        self.anchor = None;
        self.scalar = Analysis::default();
        match self.event() {
            Event::Alias { anchor } => self.anchor = Some((anchor.clone(), true)),
            Event::SequenceStart { anchor: Some(a), .. }
            | Event::MappingStart { anchor: Some(a), .. }
            | Event::Scalar { anchor: Some(a), .. } => {
                self.anchor = Some((a.clone(), false));
            }
            _ => {}
        }
        match self.event().clone() {
            Event::Scalar { tag, value, plain_implicit, quoted_implicit, .. } => {
                if let Some(t) = tag
                    && !plain_implicit
                    && !quoted_implicit
                {
                    self.analyze_tag(&t);
                }
                self.analyze_scalar(&value);
            }
            Event::SequenceStart { tag: Some(t), implicit: false, .. }
            | Event::MappingStart { tag: Some(t), implicit: false, .. } => self.analyze_tag(&t),
            _ => {}
        }
    }

    fn analyze_tag(&mut self, tag: &str) {
        for (handle, prefix) in [("!", "!"), ("!!", "tag:yaml.org,2002:")] {
            if prefix.len() < tag.len() && tag.starts_with(prefix) {
                self.tag_handle = Some(handle.into());
                self.tag_suffix = Some(tag[prefix.len()..].into());
                return;
            }
        }
        self.tag_suffix = Some(tag.into());
    }

    fn analyze_scalar(&mut self, value: &str) {
        let cs: Vec<char> = value.chars().collect();
        let a = &mut self.scalar;
        if cs.is_empty() {
            *a = Analysis { block_plain_allowed: true, single_quoted_allowed: true, ..Analysis::default() };
            return;
        }
        let (mut block_indicators, mut flow_indicators) = (false, false);
        let (mut line_breaks, mut special_characters) = (false, false);
        let (mut leading_space, mut leading_break, mut trailing_space, mut trailing_break) =
            (false, false, false, false);
        let (mut break_space, mut space_break) = (false, false);
        let (mut previous_space, mut previous_break) = (false, false);
        if value.starts_with("---") || value.starts_with("...") {
            block_indicators = true;
            flow_indicators = true;
        }
        let mut preceded_by_whitespace = true;
        let mut followed_by_whitespace = is_blankz(cs.get(1));
        let n = cs.len();
        for (i, &c) in cs.iter().enumerate() {
            if i == 0 {
                if matches!(
                    c,
                    '#' | ',' | '[' | ']' | '{' | '}' | '&' | '*' | '!' | '|' | '>' | '\'' | '"' | '%' | '@' | '`'
                ) {
                    flow_indicators = true;
                    block_indicators = true;
                }
                if matches!(c, '?' | ':') {
                    flow_indicators = true;
                    if followed_by_whitespace {
                        block_indicators = true;
                    }
                }
                if c == '-' && followed_by_whitespace {
                    flow_indicators = true;
                    block_indicators = true;
                }
            } else {
                if matches!(c, ',' | '?' | '[' | ']' | '{' | '}') {
                    flow_indicators = true;
                }
                if c == ':' {
                    flow_indicators = true;
                    if followed_by_whitespace {
                        block_indicators = true;
                    }
                }
                if c == '#' && preceded_by_whitespace {
                    flow_indicators = true;
                    block_indicators = true;
                }
            }
            if !is_printable(c) {
                special_characters = true;
            }
            if is_break(c) {
                line_breaks = true;
            }
            if c == ' ' {
                leading_space |= i == 0;
                trailing_space |= i == n - 1;
                break_space |= previous_break;
                previous_space = true;
                previous_break = false;
            } else if is_break(c) {
                leading_break |= i == 0;
                trailing_break |= i == n - 1;
                space_break |= previous_space;
                previous_space = false;
                previous_break = true;
            } else {
                previous_space = false;
                previous_break = false;
            }
            preceded_by_whitespace = is_blankz(Some(&c));
            if i + 1 < n {
                followed_by_whitespace = is_blankz(cs.get(i + 2));
            }
        }
        a.multiline = line_breaks;
        a.flow_plain_allowed = true;
        a.block_plain_allowed = true;
        a.single_quoted_allowed = true;
        a.block_allowed = true;
        if leading_space || leading_break || trailing_space || trailing_break {
            a.flow_plain_allowed = false;
            a.block_plain_allowed = false;
        }
        if trailing_space {
            a.block_allowed = false;
        }
        if break_space {
            a.flow_plain_allowed = false;
            a.block_plain_allowed = false;
            a.single_quoted_allowed = false;
        }
        if space_break || special_characters {
            a.flow_plain_allowed = false;
            a.block_plain_allowed = false;
            a.single_quoted_allowed = false;
            a.block_allowed = false;
        }
        if line_breaks {
            a.flow_plain_allowed = false;
            a.block_plain_allowed = false;
        }
        if flow_indicators {
            a.flow_plain_allowed = false;
        }
        if block_indicators {
            a.block_plain_allowed = false;
        }
        a.chars = cs;
    }

    fn process_anchor(&mut self) {
        let Some((anchor, alias)) = self.anchor.clone() else { return };
        self.write_indicator(if alias { "*" } else { "&" }, true, false, false);
        for c in anchor.chars() {
            self.put(c);
        }
        self.whitespace = false;
        self.indention = false;
    }

    fn process_tag(&mut self) {
        match (self.tag_handle.clone(), self.tag_suffix.clone()) {
            (None, None) => {}
            (Some(h), suffix) => {
                self.write_tag_handle(&h);
                if let Some(s) = suffix {
                    self.write_tag_content(&s, false);
                }
            }
            (None, Some(s)) => {
                self.write_indicator("!<", true, false, false);
                self.write_tag_content(&s, false);
                self.write_indicator(">", false, false, false);
            }
        }
    }

    fn process_scalar(&mut self) {
        let chars = std::mem::take(&mut self.scalar.chars);
        let allow_breaks = !self.simple_key_context;
        match self.scalar.style {
            Some(ScalarStyle::SingleQuoted) => self.write_single_quoted(&chars, allow_breaks),
            Some(ScalarStyle::DoubleQuoted) => self.write_double_quoted(&chars, allow_breaks),
            // Psych asks for a folded scalar only with a line width set, which `to_yaml` does not.
            Some(ScalarStyle::Literal | ScalarStyle::Folded) => self.write_literal(&chars),
            _ => self.write_plain(&chars, allow_breaks),
        }
    }

    // ---- writers ----

    fn put(&mut self, c: char) {
        self.out.push(c);
        self.column += 1;
    }

    fn put_break(&mut self) {
        self.out.push('\n');
        self.column = 0;
    }

    fn write_break(&mut self, c: char) {
        if c == '\n' {
            self.put_break();
        } else {
            self.out.push(c);
            self.column = 0;
        }
    }

    fn write_indent(&mut self) {
        let indent = self.indent.max(0) as usize;
        if !self.indention || self.column > indent || (self.column == indent && !self.whitespace) {
            self.put_break();
        }
        while self.column < indent {
            self.put(' ');
        }
        self.whitespace = true;
        self.indention = true;
    }

    fn write_indicator(&mut self, indicator: &str, need_whitespace: bool, is_whitespace: bool, is_indention: bool) {
        if need_whitespace && !self.whitespace {
            self.put(' ');
        }
        for c in indicator.chars() {
            self.put(c);
        }
        self.whitespace = is_whitespace;
        self.indention = self.indention && is_indention;
    }

    fn write_tag_handle(&mut self, handle: &str) {
        if !self.whitespace {
            self.put(' ');
        }
        for c in handle.chars() {
            self.put(c);
        }
        self.whitespace = false;
        self.indention = false;
    }

    fn write_tag_content(&mut self, value: &str, need_whitespace: bool) {
        if need_whitespace && !self.whitespace {
            self.put(' ');
        }
        for c in value.chars() {
            if is_alpha(c)
                || matches!(
                    c,
                    ';' | '/'
                        | '?'
                        | ':'
                        | '@'
                        | '&'
                        | '='
                        | '+'
                        | '$'
                        | ','
                        | '.'
                        | '~'
                        | '*'
                        | '\''
                        | '('
                        | ')'
                        | '['
                        | ']'
                )
            {
                self.put(c);
            } else {
                let mut buf = [0; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    for ch in format!("%{b:02X}").chars() {
                        self.put(ch);
                    }
                }
            }
        }
        self.whitespace = false;
        self.indention = false;
    }

    fn write_plain(&mut self, s: &[char], allow_breaks: bool) {
        let (mut spaces, mut breaks) = (false, false);
        if !self.whitespace && (!s.is_empty() || self.flow_level > 0) {
            self.put(' ');
        }
        let mut i = 0;
        while i < s.len() {
            let c = s[i];
            if c == ' ' {
                if allow_breaks && !spaces && self.column > BEST_WIDTH && s.get(i + 1) != Some(&' ') {
                    self.write_indent();
                } else {
                    self.put(c);
                }
                spaces = true;
            } else if is_break(c) {
                if !breaks && c == '\n' {
                    self.put_break();
                }
                self.write_break(c);
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                }
                self.put(c);
                self.indention = false;
                spaces = false;
                breaks = false;
            }
            i += 1;
        }
        self.whitespace = false;
        self.indention = false;
    }

    fn write_single_quoted(&mut self, s: &[char], allow_breaks: bool) {
        let (mut spaces, mut breaks) = (false, false);
        self.write_indicator("'", true, false, false);
        for (i, &c) in s.iter().enumerate() {
            if c == ' ' {
                if allow_breaks
                    && !spaces
                    && self.column > BEST_WIDTH
                    && i != 0
                    && i != s.len() - 1
                    && s.get(i + 1) != Some(&' ')
                {
                    self.write_indent();
                } else {
                    self.put(c);
                }
                spaces = true;
            } else if is_break(c) {
                if !breaks && c == '\n' {
                    self.put_break();
                }
                self.write_break(c);
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                }
                if c == '\'' {
                    self.put('\'');
                }
                self.put(c);
                self.indention = false;
                spaces = false;
                breaks = false;
            }
        }
        if breaks {
            self.write_indent();
        }
        self.write_indicator("'", false, false, false);
        self.whitespace = false;
        self.indention = false;
    }

    fn write_double_quoted(&mut self, s: &[char], allow_breaks: bool) {
        let mut spaces = false;
        self.write_indicator("\"", true, false, false);
        for (i, &c) in s.iter().enumerate() {
            if !is_printable(c) || c == '\u{feff}' || is_break(c) || c == '"' || c == '\\' {
                self.put('\\');
                let escape = match c {
                    '\0' => Some('0'),
                    '\x07' => Some('a'),
                    '\x08' => Some('b'),
                    '\t' => Some('t'),
                    '\n' => Some('n'),
                    '\x0B' => Some('v'),
                    '\x0C' => Some('f'),
                    '\r' => Some('r'),
                    '\x1B' => Some('e'),
                    '"' => Some('"'),
                    '\\' => Some('\\'),
                    '\u{85}' => Some('N'),
                    '\u{A0}' => Some('_'),
                    '\u{2028}' => Some('L'),
                    '\u{2029}' => Some('P'),
                    _ => None,
                };
                match escape {
                    Some(e) => self.put(e),
                    None => {
                        let v = c as u32;
                        let (letter, width) = if v <= 0xFF {
                            ('x', 2)
                        } else if v <= 0xFFFF {
                            ('u', 4)
                        } else {
                            ('U', 8)
                        };
                        self.put(letter);
                        for d in format!("{v:0width$X}").chars() {
                            self.put(d);
                        }
                    }
                }
                spaces = false;
            } else if c == ' ' {
                if allow_breaks && !spaces && self.column > BEST_WIDTH && i != 0 && i != s.len() - 1 {
                    self.write_indent();
                    if s.get(i + 1) == Some(&' ') {
                        self.put('\\');
                    }
                } else {
                    self.put(c);
                }
                spaces = true;
            } else {
                self.put(c);
                spaces = false;
            }
        }
        self.write_indicator("\"", false, false, false);
        self.whitespace = false;
        self.indention = false;
    }

    fn write_block_scalar_hints(&mut self, s: &[char]) {
        if s.first().is_some_and(|&c| c == ' ' || is_break(c)) {
            self.write_indicator("2", false, false, false);
        }
        self.open_ended = 0;
        let chomp = match s {
            [] => Some("-"),
            [.., last] if !is_break(*last) => Some("-"),
            [_] => {
                self.open_ended = 2;
                Some("+")
            }
            [.., before, _] if is_break(*before) => {
                self.open_ended = 2;
                Some("+")
            }
            _ => None,
        };
        if let Some(h) = chomp {
            self.write_indicator(h, false, false, false);
        }
    }

    fn write_literal(&mut self, s: &[char]) {
        let mut breaks = true;
        self.write_indicator("|", true, false, false);
        self.write_block_scalar_hints(s);
        self.put_break();
        self.indention = true;
        self.whitespace = true;
        for &c in s {
            if is_break(c) {
                self.write_break(c);
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                }
                self.put(c);
                self.indention = false;
                breaks = false;
            }
        }
    }
}
