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
                | "expression_not_writable_numbered"
                | "begin_lonely_else"
                | "class_name"
                | "module_name"
                | "class_in_method"
                | "module_in_method"
                | "def_endless_setter"
                | "embdoc_term"
                | "parameter_circular"
                | "parameter_name_repeat"
                | "return_invalid"
                | "parameter_numbered_reserved"
                | "regexp_parse_error"
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

/// Numbered-parameter errors as the parser gem (2.7 to 3.2) reports them, one per identifier:
/// Ruby27#try_declare_numparam for a read of `_1`..`_9` and Builders::Default#assignable for a
/// write, each against the static environment and the max_numparam_stack (a dynamic scope per
/// block or lambda, a static one per def/class/module/sclass). An error's yyerror skips the
/// declaration, so every later read or write of the same name errs again. Prism reports
/// `numbered_parameter_ordinary` once per block, `numbered_parameter_outer_block` where the parser
/// gem sees an inherited local, and assignments with its own kinds and wording (#164); its
/// reserved-name errors on parameters (`|_1|`) have no counterpart here. (start, end, message).
pub fn numbered_parameter_errors(program: &ruby_prism::Node, target: f64) -> Vec<(usize, usize, String)> {
    use ruby_prism::Visit;
    struct Scope {
        dynamic: bool,
        /// Inside a block or lambda (the parser gem's `context.in_dynamic_block?`), which a
        /// def/class/module body nested in it does not reset.
        in_block: bool,
        ordinary: bool,
        max: u8,
        declared: Vec<u8>,
    }
    struct Walk {
        target: f64,
        /// `accepted_numbered_targets` (3.0+): declared without the reserved-name error.
        accepted: Vec<usize>,
        scopes: Vec<Scope>,
        out: Vec<(usize, usize, String)>,
    }
    fn numparam(name: &[u8]) -> Option<u8> {
        match name {
            [b'_', d @ b'1'..=b'9'] => Some(d - b'0'),
            _ => None,
        }
    }
    impl Walk {
        fn scoped(&mut self, dynamic: bool, ordinary: bool, f: impl FnOnce(&mut Self)) {
            let declared =
                if dynamic { self.scopes.last().map_or(Vec::new(), |s| s.declared.clone()) } else { Vec::new() };
            let in_block = dynamic || self.scopes.last().is_some_and(|s| s.in_block);
            self.scopes.push(Scope { dynamic, in_block, ordinary, max: 0, declared });
            f(self);
            self.scopes.pop();
        }
        fn read(&mut self, name: &[u8], loc: ruby_prism::Location) {
            let Some(n) = numparam(name) else { return };
            let top = self.scopes.len() - 1;
            // A def/class body in a block takes `_1` as a numbered parameter too, checked against
            // the scopes outside it (`foo { _1; def x; _1; end }` is an error).
            if !self.scopes[top].in_block || self.scopes[top].declared.contains(&n) {
                return;
            }
            let (s, e) = (loc.start_offset(), loc.end_offset());
            if self.scopes[top].ordinary {
                self.out.push((s, e, "ordinary parameter is defined".to_string()));
            } else if self.scopes[..top].iter().rev().take_while(|s| s.dynamic).any(|s| s.max > 0) {
                self.out.push((s, e, "numbered parameter is already used in an outer scope".to_string()));
            } else {
                self.scopes[top].declared.push(n);
                self.scopes[top].max = self.scopes[top].max.max(n);
            }
        }
        fn declare(&mut self, name: &[u8]) {
            let top = self.scopes.last_mut().unwrap();
            if let Some(n) = numparam(name).filter(|n| !top.declared.contains(n)) {
                top.declared.push(n);
            }
        }
        fn write(&mut self, name: &[u8], loc: ruby_prism::Location) {
            let Some(n) = numparam(name) else { return };
            let top = self.scopes.last_mut().unwrap();
            let (s, e) = (loc.start_offset(), loc.end_offset());
            if self.accepted.binary_search(&s).is_ok() {
                if !top.declared.contains(&n) {
                    top.declared.push(n);
                }
            } else if top.dynamic && top.max > 0 {
                self.out.push((s, e, format!("cannot assign to numbered parameter _{n}")));
            } else if self.target >= 3.0 {
                self.out.push((s, e, format!("_{n} is reserved for numbered parameter")));
            } else if !top.declared.contains(&n) {
                top.declared.push(n);
            }
        }
    }
    impl<'pr> Visit<'pr> for Walk {
        fn visit_block_node(&mut self, n: &ruby_prism::BlockNode<'pr>) {
            let ordinary = n.parameters().is_some_and(|p| p.as_block_parameters_node().is_some());
            self.scoped(true, ordinary, |w| ruby_prism::visit_block_node(w, n));
        }
        fn visit_lambda_node(&mut self, n: &ruby_prism::LambdaNode<'pr>) {
            let ordinary = n.parameters().is_some_and(|p| p.as_block_parameters_node().is_some());
            self.scoped(true, ordinary, |w| ruby_prism::visit_lambda_node(w, n));
        }
        // A def's `f_norm_arg`s and keyword arguments (not `*`, `**`, `&` or `...`) mark its scope as
        // having ordinary parameters (#363): a `_1` in it, inside a block, is an error.
        fn visit_def_node(&mut self, n: &ruby_prism::DefNode<'pr>) {
            let ordinary = n.parameters().is_some_and(|p| {
                !(p.requireds().is_empty()
                    && p.optionals().is_empty()
                    && p.posts().is_empty()
                    && p.keywords().is_empty())
            });
            self.scoped(false, ordinary, |w| ruby_prism::visit_def_node(w, n));
        }
        fn visit_class_node(&mut self, n: &ruby_prism::ClassNode<'pr>) {
            self.scoped(false, false, |w| ruby_prism::visit_class_node(w, n));
        }
        fn visit_module_node(&mut self, n: &ruby_prism::ModuleNode<'pr>) {
            self.scoped(false, false, |w| ruby_prism::visit_module_node(w, n));
        }
        fn visit_singleton_class_node(&mut self, n: &ruby_prism::SingletonClassNode<'pr>) {
            self.scoped(false, false, |w| ruby_prism::visit_singleton_class_node(w, n));
        }
        fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
            self.read(n.name().as_slice(), n.location());
        }
        fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
            if n.is_variable_call() {
                self.read(n.name().as_slice(), n.location());
            } else {
                ruby_prism::visit_call_node(self, n);
            }
        }
        // The target is reduced (and checked) before the value is parsed.
        fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode<'pr>) {
            self.write(n.name().as_slice(), n.name_loc());
            self.visit(&n.value());
        }
        fn visit_local_variable_operator_write_node(&mut self, n: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
            self.write(n.name().as_slice(), n.name_loc());
            self.visit(&n.value());
        }
        fn visit_local_variable_or_write_node(&mut self, n: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
            self.write(n.name().as_slice(), n.name_loc());
            self.visit(&n.value());
        }
        fn visit_local_variable_and_write_node(&mut self, n: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
            self.write(n.name().as_slice(), n.name_loc());
            self.visit(&n.value());
        }
        fn visit_local_variable_target_node(&mut self, n: &ruby_prism::LocalVariableTargetNode<'pr>) {
            self.write(n.name().as_slice(), n.location());
        }
        // A parameter named `_1` (2.7: `foo { |_1| _1 }`) is a declared local, so reads of it are no
        // numbered parameters; from 3.0 Prism reports the name as reserved.
        fn visit_required_parameter_node(&mut self, n: &ruby_prism::RequiredParameterNode<'pr>) {
            self.declare(n.name().as_slice());
        }
        fn visit_optional_parameter_node(&mut self, n: &ruby_prism::OptionalParameterNode<'pr>) {
            self.declare(n.name().as_slice());
            ruby_prism::visit_optional_parameter_node(self, n);
        }
        fn visit_block_local_variable_node(&mut self, n: &ruby_prism::BlockLocalVariableNode<'pr>) {
            self.declare(n.name().as_slice());
        }
        fn visit_rest_parameter_node(&mut self, n: &ruby_prism::RestParameterNode<'pr>) {
            if let Some(name) = n.name() {
                self.declare(name.as_slice());
            }
        }
        fn visit_keyword_rest_parameter_node(&mut self, n: &ruby_prism::KeywordRestParameterNode<'pr>) {
            if let Some(name) = n.name() {
                self.declare(name.as_slice());
            }
        }
        fn visit_block_parameter_node(&mut self, n: &ruby_prism::BlockParameterNode<'pr>) {
            if let Some(name) = n.name() {
                self.declare(name.as_slice());
            }
        }
        fn visit_required_keyword_parameter_node(&mut self, n: &ruby_prism::RequiredKeywordParameterNode<'pr>) {
            self.declare(n.name().as_slice());
        }
        fn visit_optional_keyword_parameter_node(&mut self, n: &ruby_prism::OptionalKeywordParameterNode<'pr>) {
            self.declare(n.name().as_slice());
            ruby_prism::visit_optional_keyword_parameter_node(self, n);
        }
    }
    let mut w = Walk {
        target,
        accepted: if target >= 3.0 { accepted_numbered_targets(program) } else { Vec::new() },
        scopes: vec![Scope { dynamic: false, in_block: false, ordinary: false, max: 0, declared: Vec::new() }],
        out: Vec::new(),
    };
    w.visit(program);
    w.out.sort_unstable();
    w.out
}

/// Below Ruby 3.0 `_1 = 1` declares a local for the parser gem, whose later reads are `lvar`s
/// (#364); Prism refuses to declare a `_1`..`_9` local and gives those reads as variable calls.
/// The start offsets (sorted) of the `_N` variable calls the parser gem reads as locals: declared
/// by an assignment or parameter in the same scope, or around a block.
/// From 3.0 only the declarations `accepted_numbered_targets` gives count (`accept`); the others
/// are syntax errors. Also the starts (sorted) of the blocks and lambdas whose numbered parameters
/// (2.7+) are such declared reads: blocks without parameters for the parser gem. And the blocks
/// (sorted, with the highest N) with an undeclared `_N` read: the parser gem's numblocks, also
/// where Prism gave none for an inner block's read of the local (#553).
#[allow(clippy::type_complexity)]
pub fn assigned_numbered_reads(
    program: &ruby_prism::Node,
    accept: Option<Vec<usize>>,
) -> (Vec<usize>, Vec<usize>, Vec<(usize, u8)>) {
    numbered_walk(program, false, accept)
}

/// Below Ruby 2.7 the start offsets (sorted) of the `_N` local reads (Prism's numbered parameter
/// reads) no parameter or assignment declares: the parser gem's `(send nil :_1)` (#111, #362).
/// A declared one (`|_1|`, `_1 = x`, #520) is an `lvar` like any local.
pub fn legacy_numbered_sends(program: &ruby_prism::Node) -> Vec<usize> {
    numbered_walk(program, true, None).0
}

/// From Ruby 3.0 the parser gem reports a `_1`..`_9` name as reserved where its builder calls
/// `assignable` or declares a parameter, but not for a named capture (`/(?<_1>a)/ =~ s`, match_op)
/// or a pattern variable it declares without `assignable`: a hash pattern key (`in {_1:}`,
/// `in {"_1":}`), `in [*_1]`, `in {**_1}` (#533). The start offsets (sorted) of those targets.
pub fn accepted_numbered_targets(program: &ruby_prism::Node) -> Vec<usize> {
    use ruby_prism::Visit;
    struct Walk(Vec<usize>);
    impl Walk {
        fn target(&mut self, n: Option<ruby_prism::Node>) {
            if let Some(t) = n.and_then(|n| n.as_local_variable_target_node())
                && matches!(t.name().as_slice(), [b'_', b'1'..=b'9'])
            {
                self.0.push(t.location().start_offset());
            }
        }
    }
    impl<'pr> Visit<'pr> for Walk {
        fn visit_match_write_node(&mut self, n: &ruby_prism::MatchWriteNode<'pr>) {
            for t in n.targets().iter() {
                self.target(Some(t));
            }
            ruby_prism::visit_match_write_node(self, n);
        }
        fn visit_hash_pattern_node(&mut self, n: &ruby_prism::HashPatternNode<'pr>) {
            for e in n.elements().iter() {
                self.target(e.as_assoc_node().and_then(|a| a.value().as_implicit_node()).map(|i| i.value()));
            }
            self.target(n.rest().and_then(|r| r.as_assoc_splat_node()).and_then(|s| s.value()));
            ruby_prism::visit_hash_pattern_node(self, n);
        }
        fn visit_array_pattern_node(&mut self, n: &ruby_prism::ArrayPatternNode<'pr>) {
            self.target(n.rest().and_then(|r| r.as_splat_node()).and_then(|s| s.expression()));
            ruby_prism::visit_array_pattern_node(self, n);
        }
        fn visit_find_pattern_node(&mut self, n: &ruby_prism::FindPatternNode<'pr>) {
            self.target(n.left().expression());
            self.target(n.right().as_splat_node().and_then(|s| s.expression()));
            ruby_prism::visit_find_pattern_node(self, n);
        }
    }
    let mut w = Walk(Vec::new());
    w.visit(program);
    w.0.sort_unstable();
    w.0
}

#[allow(clippy::type_complexity)]
fn numbered_walk(
    program: &ruby_prism::Node,
    sends: bool,
    accept: Option<Vec<usize>>,
) -> (Vec<usize>, Vec<usize>, Vec<(usize, u8)>) {
    use ruby_prism::Visit;
    fn numbered(name: &[u8]) -> bool {
        matches!(name, [b'_', b'1'..=b'9'])
    }
    struct Walk {
        /// The `_N` names declared in each scope (a block's starts with its parent's).
        scopes: Vec<Vec<Vec<u8>>>,
        out: Vec<usize>,
        /// Collect undeclared local reads instead of declared variable calls.
        sends: bool,
        /// Only these targets declare (`assigned_numbered_reads`).
        accept: Option<Vec<usize>>,
        /// The starts of the enclosing blocks and lambdas.
        blocks: Vec<usize>,
        /// Blocks with a declared `_N` read Prism took for a numbered parameter.
        numbered_blocks: Vec<usize>,
        /// Blocks with an undeclared `_N` read (and its N): numbered parameters for the parser gem too.
        mixed_blocks: Vec<(usize, u8)>,
    }
    impl Walk {
        fn scoped(&mut self, inherit: bool, f: impl FnOnce(&mut Self)) {
            let declared = if inherit { self.scopes.last().cloned().unwrap_or_default() } else { Vec::new() };
            self.scopes.push(declared);
            f(self);
            self.scopes.pop();
        }
        fn declare(&mut self, name: &[u8]) {
            if self.accept.is_none() {
                self.declare_name(name);
            }
        }
        fn declare_name(&mut self, name: &[u8]) {
            let top = self.scopes.last_mut().unwrap();
            if numbered(name) && !top.iter().any(|d| d == name) {
                top.push(name.to_vec());
            }
        }
        fn declared(&self, name: &[u8]) -> bool {
            numbered(name) && self.scopes.last().unwrap().iter().any(|d| d == name)
        }
        fn block(&mut self, start: usize, f: impl FnOnce(&mut Self)) {
            self.blocks.push(start);
            self.scoped(true, f);
            self.blocks.pop();
        }
    }
    impl<'pr> Visit<'pr> for Walk {
        fn visit_block_node(&mut self, n: &ruby_prism::BlockNode<'pr>) {
            self.block(n.location().start_offset(), |w| ruby_prism::visit_block_node(w, n));
        }
        fn visit_lambda_node(&mut self, n: &ruby_prism::LambdaNode<'pr>) {
            self.block(n.location().start_offset(), |w| ruby_prism::visit_lambda_node(w, n));
        }
        fn visit_def_node(&mut self, n: &ruby_prism::DefNode<'pr>) {
            if let Some(r) = n.receiver() {
                self.visit(&r);
            }
            self.scoped(false, |w| {
                if let Some(p) = n.parameters() {
                    w.visit(&p.as_node());
                }
                if let Some(b) = n.body() {
                    w.visit(&b);
                }
            });
        }
        fn visit_class_node(&mut self, n: &ruby_prism::ClassNode<'pr>) {
            self.scoped(false, |w| ruby_prism::visit_class_node(w, n));
        }
        fn visit_module_node(&mut self, n: &ruby_prism::ModuleNode<'pr>) {
            self.scoped(false, |w| ruby_prism::visit_module_node(w, n));
        }
        fn visit_singleton_class_node(&mut self, n: &ruby_prism::SingletonClassNode<'pr>) {
            self.visit(&n.expression());
            self.scoped(false, |w| {
                if let Some(b) = n.body() {
                    w.visit(&b);
                }
            });
        }
        fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
            let name = n.name().as_slice();
            if !self.sends && n.is_variable_call() && self.declared(name) {
                self.out.push(n.location().start_offset());
            }
            ruby_prism::visit_call_node(self, n);
        }
        fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
            let name = n.name().as_slice();
            if self.sends && numbered(name) && !self.declared(name) {
                self.out.push(n.location().start_offset());
            } else if !self.sends && n.depth() == 0 && numbered(name) {
                // A block that also reads an undeclared `_N` keeps its numbered parameters.
                if self.declared(name) {
                    self.numbered_blocks.extend(self.blocks.last());
                } else {
                    self.mixed_blocks.extend(self.blocks.last().map(|&b| (b, name[1] - b'0')));
                }
            }
        }
        // The target is declared before the value is parsed (`_1 = _1` reads the local).
        fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode<'pr>) {
            self.declare(n.name().as_slice());
            self.visit(&n.value());
        }
        fn visit_local_variable_operator_write_node(&mut self, n: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
            self.declare(n.name().as_slice());
            self.visit(&n.value());
        }
        fn visit_local_variable_or_write_node(&mut self, n: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
            self.declare(n.name().as_slice());
            self.visit(&n.value());
        }
        fn visit_local_variable_and_write_node(&mut self, n: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
            self.declare(n.name().as_slice());
            self.visit(&n.value());
        }
        fn visit_local_variable_target_node(&mut self, n: &ruby_prism::LocalVariableTargetNode<'pr>) {
            let at = n.location().start_offset();
            if self.accept.as_ref().is_none_or(|a| a.binary_search(&at).is_ok()) {
                self.declare_name(n.name().as_slice());
            }
        }
        fn visit_required_parameter_node(&mut self, n: &ruby_prism::RequiredParameterNode<'pr>) {
            self.declare(n.name().as_slice());
        }
        fn visit_optional_parameter_node(&mut self, n: &ruby_prism::OptionalParameterNode<'pr>) {
            self.declare(n.name().as_slice());
            ruby_prism::visit_optional_parameter_node(self, n);
        }
        fn visit_block_local_variable_node(&mut self, n: &ruby_prism::BlockLocalVariableNode<'pr>) {
            self.declare(n.name().as_slice());
        }
        fn visit_rest_parameter_node(&mut self, n: &ruby_prism::RestParameterNode<'pr>) {
            if let Some(name) = n.name() {
                self.declare(name.as_slice());
            }
        }
        fn visit_keyword_rest_parameter_node(&mut self, n: &ruby_prism::KeywordRestParameterNode<'pr>) {
            if let Some(name) = n.name() {
                self.declare(name.as_slice());
            }
        }
        fn visit_block_parameter_node(&mut self, n: &ruby_prism::BlockParameterNode<'pr>) {
            if let Some(name) = n.name() {
                self.declare(name.as_slice());
            }
        }
        fn visit_required_keyword_parameter_node(&mut self, n: &ruby_prism::RequiredKeywordParameterNode<'pr>) {
            self.declare(n.name().as_slice());
        }
        fn visit_optional_keyword_parameter_node(&mut self, n: &ruby_prism::OptionalKeywordParameterNode<'pr>) {
            self.declare(n.name().as_slice());
            ruby_prism::visit_optional_keyword_parameter_node(self, n);
        }
    }
    let mut w = Walk {
        scopes: vec![Vec::new()],
        out: Vec::new(),
        sends,
        accept,
        blocks: Vec::new(),
        numbered_blocks: Vec::new(),
        mixed_blocks: Vec::new(),
    };
    w.visit(program);
    w.out.sort_unstable();
    let mut mixed = std::mem::take(&mut w.mixed_blocks);
    w.numbered_blocks.retain(|b| !mixed.iter().any(|m| m.0 == *b));
    w.numbered_blocks.sort_unstable();
    w.numbered_blocks.dedup();
    // Each block once, with its maximum.
    mixed.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    mixed.dedup_by_key(|m| m.0);
    (w.out, w.numbered_blocks, mixed)
}

/// Builders::Default#static_regexp: every regexp literal without interpolation is compiled with
/// `Regexp.new`, and Onigmo's error (`helpers::onigmo_compile`) is reported on the literal as
/// `<message>: /source/` (source with line continuations removed). (start, end, message) of each.
// ponytail: `/x` literals are skipped (free-spacing mode is not modelled).
pub fn regexp_compile_errors(program: &ruby_prism::Node, text: &str) -> Vec<(usize, usize, String)> {
    use ruby_prism::Visit;
    struct Walk<'t> {
        text: &'t str,
        out: Vec<(usize, usize, String)>,
    }
    impl Walk<'_> {
        fn check(&mut self, node: &ruby_prism::Node, content: ruby_prism::Location, closing: ruby_prism::Location) {
            let flags = self.text.get(closing.start_offset() + 1..closing.end_offset()).unwrap_or("");
            if flags.contains('x') {
                return;
            }
            let Some(source) = self.text.get(content.start_offset()..content.end_offset()) else { return };
            let source = source.replace("\\\n", "");
            if let Some(msg) = crate::helpers::onigmo_compile::error(&source) {
                let loc = node.location();
                self.out.push((loc.start_offset(), loc.end_offset(), format!("{msg}: /{source}/")));
            }
        }
    }
    impl<'pr> Visit<'pr> for Walk<'_> {
        fn visit_regular_expression_node(&mut self, n: &ruby_prism::RegularExpressionNode<'pr>) {
            self.check(&n.as_node(), n.content_loc(), n.closing_loc());
        }
        fn visit_match_last_line_node(&mut self, n: &ruby_prism::MatchLastLineNode<'pr>) {
            self.check(&n.as_node(), n.content_loc(), n.closing_loc());
        }
    }
    let mut w = Walk { text, out: Vec::new() };
    w.visit(program);
    w.out
}

/// Offsets of the `return`s directly in a singleton class body (`class << self; return; end`),
/// when Prism reports any `return_invalid` (else empty, no walk). Prism reports them; the parser
/// gem's check (targets below 3.3) only covers class and module bodies (#640). A tree deeper than
/// `runner::MAX_AST_DEPTH` gives none (the walk stops there; the runner skips such a file anyway).
pub fn sclass_returns(result: &ruby_prism::ParseResult) -> Vec<usize> {
    use ruby_prism::Visit;
    struct Walk {
        /// Innermost class-like/def scopes: true for a singleton class.
        scopes: Vec<bool>,
        depth: usize,
        found: Vec<usize>,
    }
    struct TooDeep;
    impl Walk {
        fn scoped(&mut self, sclass: bool, f: impl FnOnce(&mut Self)) {
            self.scopes.push(sclass);
            f(self);
            self.scopes.pop();
        }
    }
    impl<'pr> Visit<'pr> for Walk {
        fn visit_branch_node_enter(&mut self, _: ruby_prism::Node<'pr>) {
            self.depth += 1;
            if self.depth > crate::runner::MAX_AST_DEPTH {
                // resume_unwind skips the panic hook: this is control flow, not an error.
                std::panic::resume_unwind(Box::new(TooDeep));
            }
        }
        fn visit_branch_node_leave(&mut self) {
            self.depth -= 1;
        }
        fn visit_def_node(&mut self, n: &ruby_prism::DefNode<'pr>) {
            self.scoped(false, |w| ruby_prism::visit_def_node(w, n));
        }
        fn visit_class_node(&mut self, n: &ruby_prism::ClassNode<'pr>) {
            self.scoped(false, |w| ruby_prism::visit_class_node(w, n));
        }
        fn visit_module_node(&mut self, n: &ruby_prism::ModuleNode<'pr>) {
            self.scoped(false, |w| ruby_prism::visit_module_node(w, n));
        }
        fn visit_singleton_class_node(&mut self, n: &ruby_prism::SingletonClassNode<'pr>) {
            self.scoped(true, |w| ruby_prism::visit_singleton_class_node(w, n));
        }
        fn visit_return_node(&mut self, n: &ruby_prism::ReturnNode<'pr>) {
            if self.scopes.last() == Some(&true) {
                self.found.push(n.location().start_offset());
            }
            ruby_prism::visit_return_node(self, n);
        }
    }
    if !result.errors().any(|e| e.type_name() == "return_invalid") {
        return Vec::new();
    }
    let mut w = Walk { scopes: Vec::new(), depth: 0, found: Vec::new() };
    let program = result.node();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.visit(&program))) {
        Ok(()) => w.found,
        Err(_) => Vec::new(),
    }
}
