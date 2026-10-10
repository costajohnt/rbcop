# RuboCop's EmacsStyleFormatter with one change: messages containing invalid UTF-8 (cops that
# quote string values such as "\xff") are scrubbed to U+FFFD instead of crashing the whole run.
# Used by scripts/oracle.sh: rubocop -r ./scripts/oracle_formatter.rb -f RuboCop::Formatter::RbcopOracleFormatter
module RuboCop
  module Formatter
    class RbcopOracleFormatter < EmacsStyleFormatter
      private

      def message(offense)
        message =
          if offense.disabled?
            "[Suppressed] #{offense.message}"
          elsif offense.corrected_with_todo?
            "[Todo] #{offense.message}"
          elsif offense.corrected?
            "[Corrected] #{offense.message}"
          elsif offense.correctable?
            "[Correctable] #{offense.message}"
          else
            offense.message
          end
        message.scrub.tr("\n", ' ')
      end
    end
  end
end
