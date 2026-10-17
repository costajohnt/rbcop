module RuboCop
  module Cop
    module Custom
      class NoFoo < Base
        def on_send(node)
          add_offense(node, message: "no foo") if node.method_name == :foo
        end
      end
    end
  end
end
