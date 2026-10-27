# frozen_string_literal: true

# A one-line block ends a multi-line chain: rubocop-ast counts the block as single-line, so the
# whole chain is measured against Layout/LineLength and the `begin` is kept.
def long_chain
  @long_chain ||= begin
    example_groups.
      flat_map(&:declaration_locations).
      group_by(&:first). # by file name
      transform_values { |lines| lines.map(&:last).sort.reverse }
  end
end

# Short enough when joined: redundant.
def short_chain
  @short_chain ||= begin
    items.
      map { |i| i }
  end
end

# A multi-line block is not single-line: redundant.
def multiline_block
  @multiline_block ||= begin
    example_groups.
      flat_map(&:declaration_locations).
      group_by(&:first).
      transform_values do |lines|
        lines.map(&:last).sort.reverse
      end
  end
end
