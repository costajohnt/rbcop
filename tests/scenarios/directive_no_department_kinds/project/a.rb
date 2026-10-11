# frozen_string_literal: true

# rubocop:push -LineLength +AbcSize -Metrics
puts 1
# rubocop:pop
# rubocop:disable-next MethodLength
puts 2
# rubocop:enable-next AbcSize, Lint/Syntax
puts 3
puts 4 # rubocop:disable Metrics, LineLength , Layout/LineLength, all
# rubocop:disable MethodLength, AbcSize
foo
# rubocop:enable MethodLength, AbcSize
bar
# rubocop:disable MethodLength, AbcSize
baz
# rubocop:enable AbcSize, MethodLength, NotACop, RedundantCopDisableDirective
# rubocop:next -ClassLength
