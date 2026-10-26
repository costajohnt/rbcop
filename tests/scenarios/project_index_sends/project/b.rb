# frozen_string_literal: true

# Index sends written by `recv[i] op= v` and in a multiple assignment.
Foo[1, 2] += 1
Foo[1, 2] ||= 1
Foo[1, 2] &&= 1
Foo[1, 2, 3], x = 1, 2
Foo[] += 1
Foo[*a] += 1
Foo[1, 2,
    3] += 1
::Foo[1, 2] += 1
Bar[1] += 1
Bar[1] ||= 1
Bar[1], y = 1, 2
Baz[1] += 1
Baz[1], z = 1, 2
Foo.fetch, w = 1, 2
p x, y, z, w
