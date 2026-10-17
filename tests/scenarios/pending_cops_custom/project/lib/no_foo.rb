# frozen_string_literal: true

module RuboCop
  module Cop
    module Custom
      class NoFoo < Base
        extend AutoCorrector
        MSG = "Avoid `foo`."
        def on_send(node)
          return unless node.method?(:foo) && node.receiver.nil?
          add_offense(node.loc.selector) { |c| c.replace(node.loc.selector, 'bar') }
        end
      end
    end
  end
end
