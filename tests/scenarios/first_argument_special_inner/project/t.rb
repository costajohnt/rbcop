# frozen_string_literal: true

run :foo, defaults.merge(
            bar: 3)

run :foo, defaults.merge(
  bar: 3)

run(:foo, defaults.merge(
            bar: 3))

run(:foo, defaults.merge(
  bar: 3))
