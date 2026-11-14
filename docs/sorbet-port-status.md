# rubocop-sorbet port status

Tracking issue #760 (part of #736). rubocop-sorbet 0.17.2 is ported natively in four batches on
the integration branch `feat/sorbet-plugin`, which merges to master once all four are in. Until
then the plugin is wired as a native plugin (vendored `config/plugins/rubocop-sorbet.yml`,
`NATIVE_PLUGINS`, registry.tsv, the oracle set `oracle-sorbet`) and the cops not ported yet go to
the RuboCop fallback like any unimplemented cop (`src/fallback.rs`). `scripts/check-plugins.sh`
checks only the cops `src/cops` implements, so each batch extends the gate by landing its cops.

## Per batch

1. Port the batch's cops into `src/cops/sorbet/` (Ruby source:
   `~/rubocop-1.92.2/gems/gems/rubocop-sorbet-0.17.2/lib/rubocop/cop/sorbet/` on CT 440). Use the
   helpers listed below; a helper belongs to exactly one batch, which writes it, and later batches
   only reuse or extend it.
2. Spec corpus: record the gem's tests at v0.17.2 into a scratch tree
   (`git clone --depth 1 --branch v0.17.2 https://github.com/Shopify/rubocop-sorbet`, then
   `PLUGIN=rubocop-sorbet scripts/record-spec-corpus.sh <clone>`; the gem is tested with Minitest,
   so scripts/extract_minitest_corpus.rb runs its tests) and commit only the
   `tests/spec_corpus/Sorbet_<Cop>/` directories of the batch's cops. Every committed variant is
   checked, so directories of unported cops stay out.
3. `SPEC_CORPUS_ONLY=Sorbet_ cargo test --release --test spec_corpus`, then the FRESH remote gate:
   its check-plugins sorbet iteration must show the batch's cops and `diff 0` on every corpus.
4. Update the table below and the docs/plugins.md row.

## Notes for every batch

- The oracles were recorded with `BUNDLE_GEMFILE=config/sorbet-oracle/Gemfile`, a lockfile pinning
  sorbet-static 0.6.13680, and check-plugins sets it for the sorbet iteration: the TargetSorbetVersion
  cops (S4) read the sorbet-static version from `Bundler.locked_gems`, which is `nil` in a corpus
  with a Gemfile and no lockfile, and the resulting NoMethodError stops the whole RuboCop run.
  Ports of those cops must follow Bundler (BUNDLE_GEMFILE, else the Gemfile or gems.rb found
  upward from the working directory; Gemfile.lock or gems.locked next to it).
- The spec corpus has nothing for the four TargetSorbetVersion cops: their tests stub
  `Bundler.locked_gems` with mocha, and a recorded variant runs without a lockfile. S4 adds
  tests/scenarios cases with a lockfile instead.
- Sorbet/ValidGemVersionAnnotations has no default.yml entry; it is registered from registry.tsv
  (`cop_registry::register_plugin`).
- Loading the gem patches two RuboCop cops: Style/MutableConstant ignores `T.let(value, ...)`'s
  wrapper and type declarations (`type_member`, `type_template`, `T.type_alias`), and Lint/Void skips
  an expression followed by a `#: absurd` comment. S4 ports both (gated on the plugin being loaded)
  and adds them to the sorbet check-plugins list. Layout/LeadingCommentSpace and
  Lint/SelfAssignment only get `AllowRBSInlineAnnotation: true` from the plugin's default.yml and
  are checked from S1 on.
- README.md and docs/migration.md still use Sorbet as the example of a plugin rbcop hands to
  RuboCop; the last batch updates them.

## Cops

Oracle offenses: the 15 corpora (nine corpus, six corpus2) under config/all-cops-sorbet.yml, from
`tmp-1.92.2/oracle-sorbet` on CT 440. Crashes are (cop, file) pairs in the `.crashes` files, which
the gate skips.

| Batch | Cops | Oracle offenses | Helpers it owns | Status |
|---|---:|---:|---|---|
| S1 | 9 | 257,155 | `sorbet_sigil` | ported |
| S2 | 22 | 8,605 | `sorbet_signature`, `sorbet_t_enum`, `sorbet_gem_version` | to do |
| S3 | 24 | 12,598 | `sorbet_rbs_assertion` | ported |
| S4 | 8 | 146,088 | `sorbet_target_version`, `sorbet_t_let`, `sorbet_rbi_type` | to do |

The S4 row leaves out the Style/MutableConstant and Lint/Void patches.

| Cop | Batch | Oracle offenses | Status |
|---|---|---:|---|
| Sorbet/EnforceSigilOrder | S1 | 36 | ported |
| Sorbet/EnforceSingleSigil | S1 | 0 | ported |
| Sorbet/FalseSigil | S1 | 42,372 | ported |
| Sorbet/HasSigil | S1 | 42,368 | ported |
| Sorbet/IgnoreSigil | S1 | 42,368 | ported |
| Sorbet/StrictSigil | S1 | 43,072 | ported |
| Sorbet/StrongSigil | S1 | 44,341 | ported |
| Sorbet/TrueSigil | S1 | 42,598 | ported |
| Sorbet/ValidSigil | S1 | 0 | ported |
| Sorbet/AllowIncompatibleOverride | S2 | 0 | fallback |
| Sorbet/CapitalizedTypeParameters | S2 | 0 | fallback |
| Sorbet/CheckedTrueInSignature | S2 | 0 | fallback |
| Sorbet/EmptyLineAfterSig | S2 | 2 | fallback |
| Sorbet/ForbidComparableTEnum | S2 | 0 | fallback |
| Sorbet/ForbidExtendTSigHelpersInShims | S2 | 0 | fallback |
| Sorbet/ForbidRBIOutsideOfAllowedPaths | S2 | 0 | fallback |
| Sorbet/ForbidSig | S2 | 8,408 | fallback |
| Sorbet/ForbidSigWithRuntime | S2 | 0 | fallback |
| Sorbet/ForbidSigWithoutRuntime | S2 | 21 | fallback |
| Sorbet/ForbidTEnum | S2 | 3 | fallback |
| Sorbet/ForbidTHelpers | S2 | 114 | fallback |
| Sorbet/ForbidTSig | S2 | 6 | fallback |
| Sorbet/KeywordArgumentOrdering | S2 | 0 | fallback |
| Sorbet/MultipleTEnumValues | S2 | 1 | fallback |
| Sorbet/RedundantExtendTSig | S2 | 4 | fallback |
| Sorbet/RuntimeOnFailureDependsOnChecked | S2 | 0 | fallback |
| Sorbet/SetterReturnType | S2 | 0 | fallback |
| Sorbet/SignatureBuildOrder | S2 | 0 | fallback |
| Sorbet/SingleLineRbiClassModuleDefinitions | S2 | 0 | fallback |
| Sorbet/ValidGemVersionAnnotations | S2 | 46 | fallback |
| Sorbet/VoidCheckedTests | S2 | 0 | fallback |
| Sorbet/BindingConstantWithoutTypeAlias | S3 | 0 | ported |
| Sorbet/BlockMethodDefinition | S3 | 8,611 | ported |
| Sorbet/CallbackConditionalsBinding | S3 | 89 | ported |
| Sorbet/ConstantsFromStrings | S3 | 417 | ported |
| Sorbet/ForbidIncludeConstLiteral | S3 | 95 | ported |
| Sorbet/ForbidMixesInClassMethods | S3 | 1 | ported |
| Sorbet/ForbidSuperclassConstLiteral | S3 | 1,079 | ported |
| Sorbet/ForbidTAbsurd | S3 | 14 | ported |
| Sorbet/ForbidTAnyWithNil | S3 | 0 | ported |
| Sorbet/ForbidTAssertType | S3 | 0 | ported |
| Sorbet/ForbidTBind | S3 | 978 | ported |
| Sorbet/ForbidTBindInAssignment | S3 | 0 | ported |
| Sorbet/ForbidTCast | S3 | 355 | ported |
| Sorbet/ForbidTCollectionInstantiation | S3 | 0 | ported |
| Sorbet/ForbidTMust | S3 | 0 | ported |
| Sorbet/ForbidTUnsafe | S3 | 0 | ported |
| Sorbet/ForbidTUntyped | S3 | 822 | ported |
| Sorbet/ForbidTypeAliasedShapes | S3 | 12 | ported |
| Sorbet/ForbidUntypedStructProps | S3 | 0 | ported |
| Sorbet/ImplicitConversionMethod | S3 | 109 (25 crashes) | ported |
| Sorbet/Refinement | S3 | 6 (4 crashes) | ported |
| Sorbet/SelectByIsA | S3 | 10 | ported |
| Sorbet/StructPropName | S3 | 0 | ported |
| Sorbet/TypeAliasName | S3 | 0 | ported |
| Sorbet/BuggyObsoleteStrictMemoization | S4 | 0 | fallback |
| Sorbet/EnforceSignatures | S4 | 143,768 (16 crashes) | fallback |
| Sorbet/ForbidTLet | S4 | 2,199 | fallback |
| Sorbet/ForbidTStruct | S4 | 42 | fallback |
| Sorbet/ForbidTTypeAlias | S4 | 79 | fallback |
| Sorbet/ObsoleteStrictMemoization | S4 | 0 | fallback |
| Sorbet/RedundantTLet | S4 | 0 | fallback |
| Sorbet/RedundantTLetForLiteral | S4 | 0 | fallback |
