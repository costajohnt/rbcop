# frozen_string_literal: true

module RuboCop
  module Cop
    module Custom
      # Flags puts.
      class NoPuts < Base
        MSG = 'no puts'
        RESTRICT_ON_SEND = %i[puts].freeze

        def on_send(node)
          add_offense(node)
        end
      end
    end
  end
end
