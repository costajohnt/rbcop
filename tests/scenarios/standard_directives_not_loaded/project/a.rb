# frozen_string_literal: true

x = 1
if (a = x) # standard:disable Lint/UselessAssignment
  a
end
if b = x # standard:disable Lint/AssignmentInCondition
  b
end
# standard:disable Style/StringLiterals
y = "one"
# standard:enable Style/StringLiterals
z = "two"
# standard:todo Style/StringLiterals
w = "three"
# standard:enable all
# standard:disable Lint
v = "four" if x == 1
# standard:enable Lint
#standard : disable Style/StringLiterals, Layout/SpaceAroundOperators
u ="five"
# standard:enable Style/StringLiterals, Layout/SpaceAroundOperators
t = "six" # rubocop:disable Style/StringLiterals
s = "seven" # rubocop:todo Style/StringLiterals
# rubocop:disable-next Style/StringLiterals
r = "eight"
# rubocop:push -Style/StringLiterals
q = "nine"
# rubocop:pop
# standard:disable-next Style/StringLiterals
p1 = "ten"
# standard:disable
o = "eleven"
# standard:disable Style/StringLiterals # rubocop:disable Layout/LineLength
n = "twelve"
# standard:enable Style/StringLiterals
# rubocop:disable Style/StringLiterals # standard:disable Layout/LineLength
m = "thirteen"
# rubocop:enable Style/StringLiterals
## standard:disable Style/StringLiterals
l = "fourteen"
# standard:disable Style/StringLiterals -- reason here
k = "fifteen"
# standard:enable Style/StringLiterals
# standard:enable Style/StringLiterals
# standard:disable Naming/MethodName
def fooBar; end
# standard:disable Style/NoSuchCop
j = "sixteen"
# Standard:disable Style/StringLiterals
i = "seventeen"
# standardrb:disable Style/StringLiterals
h = "eighteen"
# standard:disable all
g = "nineteen"
# standard:enable all
# rubocop:enable-next Style/StringLiterals
f = "twenty"
# rubocop:next -Style/StringLiterals
e = "twenty-one"
puts y, z, w, v, u, t, s, r, q, p1, o, n, m, l, k, j, i, h, g, f, e
# standard:disable Style/StringLiterals
