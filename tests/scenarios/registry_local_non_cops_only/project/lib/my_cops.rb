# frozen_string_literal: true

module RuboCop
  module Cop
    module Custom
      # A cop.
      class NoPuts < Base
        def on_send(node)
          add_offense(node) if node.method?(:puts_x)
        end
      end

      # An abstract base, not a cop.
      class Abstract < Base
        exclude_from_registry
      end

      # A helper module, not a cop.
      module Helper
        def helper; end
      end
    end

    module Custom2
      # A cop of another department.
      class Thing < Base
      end
    end
  end
end
