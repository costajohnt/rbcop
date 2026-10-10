# Spec corpus

The source files here are code examples from the specs of
[RuboCop](https://github.com/rubocop/rubocop) 1.91.0 (`spec/rubocop/cop/**/*_spec.rb` and
`spec/support/*.rb` at the v1.91.0 tag), extracted by `scripts/extract_cold_corpus.rb`. They are
RuboCop's work, used under its MIT license (Copyright (c) 2012-26 Bozhidar Batsov), which is
reproduced in `config/LICENSE`. The expected output was recorded by running RuboCop 1.91.0 on them.

`<Dept_Cop>/<variant>.input` holds one config variant's `.rubocop.yml` and sources;
`<variant>.expected` holds RuboCop's output for them. `scripts/record-spec-corpus.sh` regenerates
both (format in `scripts/record_spec_corpus.rb`); `tests/spec_corpus.rs` checks rbcop against them.
`skip.txt` lists the examples left out, with reasons.
