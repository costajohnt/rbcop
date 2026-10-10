# Oracle output for rbcop's parity gate: emacs format plus each offense's range end
# (`path:line:col:last_line:last_col: S: [Correctable] Cop/Name: message`), which rbcop prints with
# `-f rbcop-oracle`. Messages with invalid UTF-8 (cops quoting string values such as "\xff") are
# scrubbed to U+FFFD instead of crashing the run.
# Used by scripts/oracle.sh: rubocop -r ./scripts/oracle_formatter.rb -f RuboCop::Formatter::RbcopOracleFormatter
module RuboCop
  module Formatter
    class RbcopOracleFormatter < BaseFormatter
      def file_finished(file, offenses)
        offenses.each do |o|
          output.printf(
            "%<path>s:%<line>d:%<column>d:%<last_line>d:%<last_column>d: %<severity>s: %<message>s\n",
            path: file,
            line: o.line,
            column: o.real_column,
            last_line: o.last_line,
            last_column: o.last_column,
            severity: o.severity.code,
            message: message(o)
          )
        end
      end

      private

      def message(offense)
        message = offense.correctable? ? "[Correctable] #{offense.message}" : offense.message
        message.scrub.tr("\n", ' ')
      end
    end
  end
end
