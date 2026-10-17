# frozen_string_literal: true

def foo(node)
  node.type == :send # rubocop:disable InternalAffairs/NodeTypePredicate
end

# rubocop:disable InternalAffairs/Nope
bar
# rubocop:enable InternalAffairs/Nope
