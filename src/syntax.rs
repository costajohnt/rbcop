//! Lint/Syntax as RuboCop reports it through Prism::Translation::Parser (prism 1.9):
//! some Prism error types are re-worded with the parser gem's message templates.

/// Prism errors the parser gem reports from semantic actions or its lexer (its messages are the
/// templates below) rather than as a grammar failure (`unexpected token ...`).
pub fn is_semantic(kind: &str) -> bool {
    kind.starts_with("argument_formal_")
        || kind.starts_with("argument_no_forwarding")
        || kind.starts_with("incomplete_variable_")
        || kind.starts_with("invalid_variable_global")
        || kind.starts_with("numbered_parameter_")
        || matches!(
            kind,
            "argument_block_multi"
                | "begin_lonely_else"
                | "class_name"
                | "module_name"
                | "class_in_method"
                | "module_in_method"
                | "def_endless_setter"
                | "embdoc_term"
                | "parameter_circular"
                | "parameter_name_repeat"
                | "parameter_numbered_reserved"
                | "regexp_unknown_options"
                | "singleton_for_literals"
                | "string_literal_eof"
                | "write_target_in_method"
        )
}

/// The `translate` kind for an error the parser gem itself reports (its own wording throughout).
pub fn parser_gem_kind(kind: &str) -> &str {
    if kind == "argument_no_forwarding_ampersand" { "argument_no_forwarding_amp" } else { kind }
}

/// (start, end, message) of a Prism error after the translation layer's rewrites.
pub fn translate(kind: &str, prism_message: &str, start: usize, end: usize, text: &str) -> (usize, usize, String) {
    let slice = |s: usize, e: usize| text.get(s..e.min(text.len())).unwrap_or("").to_string();
    let fixed = |m: &str| m.to_string();
    let (start, end, msg) = match kind {
        "argument_block_multi" => (start, end, fixed("both block argument and literal block are passed")),
        "argument_formal_constant" => (start, end, fixed("formal argument cannot be a constant")),
        "argument_formal_class" => (start, end, fixed("formal argument cannot be a class variable")),
        "argument_formal_global" => (start, end, fixed("formal argument cannot be a global variable")),
        "argument_formal_ivar" => (start, end, fixed("formal argument cannot be an instance variable")),
        // prism 1.9 calls it `argument_no_forwarding_ampersand`, which the translation layer
        // (still matching `_amp`) passes through with Prism's message; the parser gem (targets
        // below 3.3, `parser_gem_kind`) has its own wording.
        "argument_no_forwarding_amp" => (start, end, fixed("no anonymous block parameter")),
        "argument_no_forwarding_star" => (start, end, fixed("no anonymous rest parameter")),
        "argument_no_forwarding_star_star" => (start, end, fixed("no anonymous keyword rest parameter")),
        "begin_lonely_else" => (start, start + 4, fixed("else without rescue is useless")),
        "class_name" | "module_name" => (start, end, fixed("class or module name must be a constant literal")),
        "class_in_method" => (start, end, fixed("class definition in method body")),
        "def_endless_setter" => (start, end, fixed("setter method cannot be defined in an endless method definition")),
        "embdoc_term" => {
            (start, end, fixed("embedded document meets end of file (and they embark on a romantic journey)"))
        }
        "incomplete_variable_class" | "incomplete_variable_class_3_3" => {
            let e = end + 1;
            (start, e, format!("`{}' is not allowed as a class variable name", slice(start, e)))
        }
        "incomplete_variable_instance" | "incomplete_variable_instance_3_3" => {
            let e = end + 1;
            (start, e, format!("`{}' is not allowed as an instance variable name", slice(start, e)))
        }
        "invalid_variable_global" | "invalid_variable_global_3_3" => {
            (start, end, format!("`{}' is not allowed as a global variable name", slice(start, end)))
        }
        "module_in_method" => (start, end, fixed("module definition in method body")),
        "numbered_parameter_ordinary" => (start, end, fixed("ordinary parameter is defined")),
        "numbered_parameter_outer_scope" => (start, end, fixed("numbered parameter is already used in an outer scope")),
        "parameter_circular" => (start, end, format!("circular argument reference {}", slice(start, end))),
        "parameter_name_repeat" => (start, end, fixed("duplicate argument name")),
        "parameter_numbered_reserved" => {
            (start, end, format!("{} is reserved for numbered parameter", slice(start, end)))
        }
        "regexp_unknown_options" => {
            let s = slice(start, end);
            (start, end, format!("unknown regexp options: {}", s.get(1..).unwrap_or("")))
        }
        "singleton_for_literals" => (start, end, fixed("cannot define a singleton method for a literal")),
        "string_literal_eof" => (start, end, fixed("unterminated string meets end of file")),
        "unexpected_token_ignore" => (start, end, format!("unexpected token {}", slice(start, end))),
        "write_target_in_method" => (start, end, fixed("dynamic constant assignment")),
        _ => (start, end, prism_message.to_string()),
    };
    // Lint/Syntax#diagnostic_location: zero-width locations grow one character to the right,
    // or (at end of file) one to the left.
    if start == end {
        if end < text.len() {
            let len = text[start..].chars().next().map_or(1, char::len_utf8);
            return (start, start + len, msg);
        }
        if start > 0 {
            let len = text[..start].chars().next_back().map_or(1, char::len_utf8);
            return (start - len, end, msg);
        }
    }
    (start, end, msg)
}

static LVAR_NAME: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"\A[\p{Lowercase}_][\p{Alphabetic}\p{Nd}_]*\z").unwrap());

/// The parser gem builder's `check_lvar_name` on pattern variables (`in x`, `in {x:}`, `in [*x]`,
/// `in a => x`): Ruby's `[[:lower:]]`/`[[:alnum:]]` reject names Prism accepts, such as one starting
/// with a non-lowercase letter (`in 变量`) or containing U+3000. (start, end, message) of each.
pub fn pattern_variable_errors(program: &ruby_prism::Node) -> Vec<(usize, usize, String)> {
    use ruby_prism::Visit;
    struct Walk {
        in_pattern: bool,
        out: Vec<(usize, usize, String)>,
    }
    impl Walk {
        fn pattern(&mut self, node: &ruby_prism::Node) {
            let outer = std::mem::replace(&mut self.in_pattern, true);
            self.visit(node);
            self.in_pattern = outer;
        }
    }
    impl<'pr> Visit<'pr> for Walk {
        fn visit_in_node(&mut self, n: &ruby_prism::InNode<'pr>) {
            // A guard (`in x if y`) wraps the pattern in an IfNode/UnlessNode; only its statements are the pattern.
            let pat = n.pattern();
            if let Some(i) = pat.as_if_node() {
                if let Some(s) = i.statements() {
                    self.pattern(&s.as_node());
                }
                self.visit(&i.predicate());
            } else if let Some(u) = pat.as_unless_node() {
                if let Some(s) = u.statements() {
                    self.pattern(&s.as_node());
                }
                self.visit(&u.predicate());
            } else {
                self.pattern(&pat);
            }
            if let Some(s) = n.statements() {
                self.visit(&s.as_node());
            }
        }
        fn visit_match_predicate_node(&mut self, n: &ruby_prism::MatchPredicateNode<'pr>) {
            self.visit(&n.value());
            self.pattern(&n.pattern());
        }
        fn visit_match_required_node(&mut self, n: &ruby_prism::MatchRequiredNode<'pr>) {
            self.visit(&n.value());
            self.pattern(&n.pattern());
        }
        fn visit_pinned_expression_node(&mut self, n: &ruby_prism::PinnedExpressionNode<'pr>) {
            let outer = std::mem::replace(&mut self.in_pattern, false);
            self.visit(&n.expression());
            self.in_pattern = outer;
        }
        fn visit_local_variable_target_node(&mut self, n: &ruby_prism::LocalVariableTargetNode<'pr>) {
            let name = String::from_utf8_lossy(n.name().as_slice());
            if self.in_pattern && !LVAR_NAME.is_match(&name) {
                let loc = n.location();
                let msg = format!("`{name}' is not allowed as a local variable name");
                self.out.push((loc.start_offset(), loc.end_offset(), msg));
            }
        }
    }
    let mut w = Walk { in_pattern: false, out: Vec::new() };
    w.visit(program);
    w.out
}
