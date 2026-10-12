def Foo.m0
[1].each { |x| x }
end
def Foo.m0
[1].each { |x| x }
end

def Foo.m1
x { |a, (b, *c), d; e| a }
x { |*| }
x { |**| }
x { |&b| }
x { |a, b = 1, *c, d:, e: 2, **f, &g| }
x { |a,| a }
end
def Foo.m1
x { |a, (b, *c), d; e| a }
x { |*| }
x { |**| }
x { |&b| }
x { |a, b = 1, *c, d:, e: 2, **f, &g| }
x { |a,| a }
end

def Foo.m2
x { _1 + _2 }
-> { _1 }
proc { _3 }
end
def Foo.m2
x { _1 + _2 }
-> { _1 }
proc { _3 }
end

def Foo.m3
x { it + 1 }
foo.map { it }
-> { it }
end
def Foo.m3
x { it + 1 }
foo.map { it }
-> { it }
end

def Foo.m4
-> (a, b = 1) { }
->(a; b) {}
lambda { |x| }
-> do a rescue b end
foo.bar ->(x) { }
end
def Foo.m4
-> (a, b = 1) { }
->(a; b) {}
lambda { |x| }
-> do a rescue b end
foo.bar ->(x) { }
end

def Foo.m5
a.b(1).c { |x| x }.d
a&.b { }
a.b do |x| end.c
foo a, b do end
foo(&nil)
f(&:x)
foo(*a, **b, &c)
end
def Foo.m5
a.b(1).c { |x| x }.d
a&.b { }
a.b do |x| end.c
foo a, b do end
foo(&nil)
f(&:x)
foo(*a, **b, &c)
end

def Foo.m6
if a; b; elsif c; d; else; e; end
a ? b : (c)
b unless c
unless a; b; else; c; end
if a then b end
a ? b : c ? d : e
end
def Foo.m6
if a; b; elsif c; d; else; e; end
a ? b : (c)
b unless c
unless a; b; else; c; end
if a then b end
a ? b : c ? d : e
end

def Foo.m7
if /x/; end
unless /x#{a}/; end
while /x/; end
if a..b; end
if a...b; end
if !(a..b); end
if a and /x/; end
end
def Foo.m7
if /x/; end
unless /x#{a}/; end
while /x/; end
if a..b; end
if a...b; end
if !(a..b); end
if a and /x/; end
end

def Foo.m8
case a; when 1, 2 then 3; when *b then 4; else; end
case; when a; end
case a; when 1; else b; end
end
def Foo.m8
case a; when 1, 2 then 3; when *b then 4; else; end
case; when a; end
case a; when 1; else b; end
end

def Foo.m9
case a
in [1, *r] if r then 1
in {a:, **nil} unless a
in {b: Integer => c, **rest}
in [*, 1, *post]
in Foo(x, y,)
in Foo::Bar(z:)
in Foo[]
in 1 | 2 | nil
in ..3
in ^c
in ^(c + 1)
in ^@a
in {}
in []
in [x, *]
in String => s
in {"q": 1}
else
end
case a; in 1; end
case a; in 1; else 2; end
end
def Foo.m9
case a
in [1, *r] if r then 1
in {a:, **nil} unless a
in {b: Integer => c, **rest}
in [*, 1, *post]
in Foo(x, y,)
in Foo::Bar(z:)
in Foo[]
in 1 | 2 | nil
in ..3
in ^c
in ^(c + 1)
in ^@a
in {}
in []
in [x, *]
in String => s
in {"q": 1}
else
end
case a; in 1; end
case a; in 1; else 2; end
end

def Foo.m10
a => [x, y]
a in Integer
b = 1
a in ^b
end
def Foo.m10
a => [x, y]
a in Integer
b = 1
a in ^b
end

def Foo.m11
while a; b; end
until a; end
begin; a; end while b
begin; a; end until b
for x, y in z; end
for a in b do c; end
for @a in b; end
c while d
x = while a; break 1; end
end
def Foo.m11
while a; b; end
until a; end
begin; a; end while b
begin; a; end until b
for x, y in z; end
for a in b do c; end
for @a in b; end
c while d
x = while a; break 1; end
end

def Foo.m12
begin; a; rescue E => e; b; else; c; ensure; d; end
a rescue b
begin end
begin a; b end
begin; a; ensure; b; end
begin; rescue A, B => @e; rescue *X; rescue => c.d; rescue => c[0]; end
end
def Foo.m12
begin; a; rescue E => e; b; else; c; ensure; d; end
a rescue b
begin end
begin a; b end
begin; a; ensure; b; end
begin; rescue A, B => @e; rescue *X; rescue => c.d; rescue => c[0]; end
end

def Foo.m13
a
rescue E
b
else
c
ensure
d
end
def Foo.m13
a
rescue E
b
else
c
ensure
d
end

def Foo.m14
x do; a; rescue; b; end
f(1) rescue nil
x = a rescue b
end
def Foo.m14
x do; a; rescue; b; end
f(1) rescue nil
x = a rescue b
end

def Foo.m15
x { next 1, a }
x { break [a: 1] }
x { next }
x { break 1, 2 }
return 1, *a
return
x { redo }
yield
yield 1, a: 2
begin; rescue; retry; end
end
def Foo.m15
x { next 1, a }
x { break [a: 1] }
x { next }
x { break 1, 2 }
return 1, *a
return
x { redo }
yield
yield 1, a: 2
begin; rescue; retry; end
end

def Foo.m16
"a#{b}c"
"a\nb"
"a
b"
:"a#{b}"
`ls #{a}`
`ls`
'a' 'b'
?a
"#@a #$b #@@c"
'a\nb'
"\u00e9\u{20 41}"
%q(a)
%Q(a#{b})
'a
b'
end
def Foo.m16
"a#{b}c"
"a\nb"
"a
b"
:"a#{b}"
`ls #{a}`
`ls`
'a' 'b'
?a
"#@a #$b #@@c"
'a\nb'
"\u00e9\u{20 41}"
%q(a)
%Q(a#{b})
'a
b'
end

def Foo.m17
"a\n#{b}\nc
d"
"a" "b" "c"
"a" "#{b}"
"x\ty
#{z}\
w"
"é\é
#{1}"
%Q(a(b)
\)c)
end
def Foo.m17
"a\n#{b}\nc
d"
"a" "b" "c"
"a" "#{b}"
"x\ty
#{z}\
w"
"é\é
#{1}"
%Q(a(b)
\)c)
end

def Foo.m18
x = <<~E
  a
  #{b}
  c
E
y = <<-E
one
E
z = <<-E.strip
  a
  b\t
E
w = <<E
E
end
def Foo.m18
x = <<~E
  a
  #{b}
  c
E
y = <<-E
one
E
z = <<-E.strip
  a
  b\t
E
w = <<E
E
end

def Foo.m19
x = <<~E
  a #{b}
    c
E
y = <<~'E'
  q\n
  r\\
E
z = <<'E'
q\n
r\\
E
puts(<<~A, <<~B)
  a
A
  b
B
end
def Foo.m19
x = <<~E
  a #{b}
    c
E
y = <<~'E'
  q\n
  r\\
E
z = <<'E'
q\n
r\\
E
puts(<<~A, <<~B)
  a
A
  b
B
end

def Foo.m20
:a
:"a b"
:""
:"#{a}"
%s(x)
:a?
:[]=
:+
:@a
:$a
:A
:"a
b"
end
def Foo.m20
:a
:"a b"
:""
:"#{a}"
%s(x)
:a?
:[]=
:+
:@a
:$a
:A
:"a
b"
end

def Foo.m21
/a#{b}/im
//
/x\d/
%r{a/b}x
/a
b/
/\//
/#{a}/o
%r(a#{b}c)mix
/a/u
end
def Foo.m21
/a#{b}/im
//
/x\d/
%r{a/b}x
/a
b/
/\//
/#{a}/o
%r(a#{b}c)mix
/a/u
end

def Foo.m22
[1, *a, 2]
[*a]
[]
[[1]]
[1, a: 2]
%w()
%W(a #{b}c d)
%I(a#{b} c)
%i()
%w(a\ b c)
%w[a b]
%i[a b]
end
def Foo.m22
[1, *a, 2]
[*a]
[]
[[1]]
[1, a: 2]
%w()
%W(a #{b}c d)
%I(a#{b} c)
%i()
%w(a\ b c)
%w[a b]
%i[a b]
end

def Foo.m23
x = {a: 1, "b" => 2, c => 3, **d, "e": 4, 1 => 5}
{}
{ "a#{b}": 1 }
{a: 1}.merge(b: 2)
{ a: }
foo(a:)
{"": 1}
end
def Foo.m23
x = {a: 1, "b" => 2, c => 3, **d, "e": 4, 1 => 5}
{}
{ "a#{b}": 1 }
{a: 1}.merge(b: 2)
{ a: }
foo(a:)
{"": 1}
end

def Foo.m24
1..2
1...
..2
3r
1.5r
2i
1.5i
2ri
-1
- 1
-1.5
- 2.5
-3r
-2i
1e3
1.0e20
1.5e-7
0x10
0b11
0o7
1_000
100.0
3.14r
1/2r
0.1
end
def Foo.m24
1..2
1...
..2
3r
1.5r
2i
1.5i
2ri
-1
- 1
-1.5
- 2.5
-3r
-2i
1e3
1.0e20
1.5e-7
0x10
0b11
0o7
1_000
100.0
3.14r
1/2r
0.1
end

def Foo.m25
$~
$1
$9
$`
$'
$+
$0
$stdout
$-w
@x
@@y
__method__
self
nil
true
false
__LINE__
__ENCODING__
end
def Foo.m25
$~
$1
$9
$`
$'
$+
$0
$stdout
$-w
@x
@@y
__method__
self
nil
true
false
__LINE__
__ENCODING__
end

def Foo.m26
a[1]
a[1] = 2
a[] = 1
foo[1, 2] = 3
a[k: 1]
a[*b]
a[1, &c]
end
def Foo.m26
a[1]
a[1] = 2
a[] = 1
foo[1, 2] = 3
a[k: 1]
a[*b]
a[1, &c]
end

def Foo.m27
foo(1, k: 2, **h)
foo(*a, &b)
foo.()
foo::bar
Foo::Bar()
foo.bar = 1
foo&.bar = 1
-a
+a
~a
a.!
!a
not a
!!a
a != b
a !~ b
foo.bar baz, qux: 1 do end
end
def Foo.m27
foo(1, k: 2, **h)
foo(*a, &b)
foo.()
foo::bar
Foo::Bar()
foo.bar = 1
foo&.bar = 1
-a
+a
~a
a.!
!a
not a
!!a
a != b
a !~ b
foo.bar baz, qux: 1 do end
end

def Foo.m28
super(a: 1)
super
super { }
super(&b)
super(1) { |x| }
super() do end
end
def Foo.m28
super(a: 1)
super
super { }
super(&b)
super(1) { |x| }
super() do end
end

def Foo.m29
a =~ b
/(?<n>x)/ =~ s
/x/ =~ s
/#{a}(?<b>x)/ =~ c
c !~ /x/
a =~ /x/
end
def Foo.m29
a =~ b
/(?<n>x)/ =~ s
/x/ =~ s
/#{a}(?<b>x)/ =~ c
c !~ /x/
a =~ /x/
end

def Foo.m30
a, (b, *c), d.e, f[0], *, @g = 1, 2
a, = b
*a = 1
a, b = *c
a, b = c, *d
(a, b), c = 1
a.b, c[1, 2] = 1, 2
@a, $b, @@c = 1
a = *b
a = 1, 2
*a, b = c
end
def Foo.m30
a, (b, *c), d.e, f[0], *, @g = 1, 2
a, = b
*a = 1
a, b = *c
a, b = c, *d
(a, b), c = 1
a.b, c[1, 2] = 1, 2
@a, $b, @@c = 1
a = *b
a = 1, 2
*a, b = c
end

def Foo.m31
a &&= 1
a ||= b rescue c
@a -= 1
$a ||= 1
@@a &&= 1
a.b &&= 1
a[1] &&= 2
a&.b += 1
a[*b] += 1
a[1] ||= a[2] &&= 3
a.b ||= 1
@a ||= 1
$a += 1
@@a ||= 2
$a &&= 1
@i &&= 1
a += 1
@@c = 1
$g = 1
end
def Foo.m31
a &&= 1
a ||= b rescue c
@a -= 1
$a ||= 1
@@a &&= 1
a.b &&= 1
a[1] &&= 2
a&.b += 1
a[*b] += 1
a[1] ||= a[2] &&= 3
a.b ||= 1
@a ||= 1
$a += 1
@@a ||= 2
$a &&= 1
@i &&= 1
a += 1
@@c = 1
$g = 1
end

def Foo.m32
def m = 1
def self.m() = 1
def a.b; end
def (a).b; end
def `(x); end
def ==(o); end
def []=(k, v); end
def m!; end
end
def Foo.m32
def m = 1
def self.m() = 1
def a.b; end
def (a).b; end
def `(x); end
def ==(o); end
def []=(k, v); end
def m!; end
end

def Foo.m33
def m(...); bar(...); end
def m(*, **, &) = foo(*, **, &)
def m(**nil); end
def m(a, (b, c), *d); end
def m(a, b = 1, *c, d, e:, f: 2, **g, &h); end
end
def Foo.m33
def m(...); bar(...); end
def m(*, **, &) = foo(*, **, &)
def m(**nil); end
def m(a, (b, c), *d); end
def m(a, b = 1, *c, d, e:, f: 2, **g, &h); end
end

def Foo.m34
class << self; a; end
class << a; end
alias a b
alias $a $b
alias :"a#{b}" c
alias a= b=
undef a, :b
undef :"x#{y}", z
end
def Foo.m34
class << self; a; end
class << a; end
alias a b
alias $a $b
alias :"a#{b}" c
alias a= b=
undef a, :b
undef :"x#{y}", z
end

def Foo.m35
defined?(a)
defined? @a
defined? a.b
defined?(yield)
defined?(super)
a = 1 if defined? a
defined? (a)
end
def Foo.m35
defined?(a)
defined? @a
defined? a.b
defined?(yield)
defined?(super)
a = 1 if defined? a
defined? (a)
end

def Foo.m36
a && b || c and d or e
not(a)
(a; b)
()
(a)
((a))
end
def Foo.m36
a && b || c and d or e
not(a)
(a; b)
()
(a)
((a))
end

def Foo.m37
Foo::Bar
::Baz
foo::Bar
Foo::Bar::Baz
end
def Foo.m37
Foo::Bar
::Baz
foo::Bar
Foo::Bar::Baz
end

def Foo.m38
END { b }
END { }
end
def Foo.m38
END { b }
END { }
end

def Foo.m39
%r{a\}b}
%r(a\)b)
%r!a\!b!
%r{a\/b}
/a\/b/
%r[a\]]
/a\
b/
%r|a\|b|
/a#{b}\/c
d/
end
def Foo.m39
%r{a\}b}
%r(a\)b)
%r!a\!b!
%r{a\/b}
/a\/b/
%r[a\]]
/a\
b/
%r|a\|b|
/a#{b}\/c
d/
end

def Foo.m40
"a\
b"
'a\
b'
"a\\
b"
:"a\
b"
"\C-a\nb
c"
"\M-\C-a
b"
"\ca
b"
"\x41\101\e
b"
end
def Foo.m40
"a\
b"
'a\
b'
"a\\
b"
:"a\
b"
"\C-a\nb
c"
"\M-\C-a
b"
"\ca
b"
"\x41\101\e
b"
end

def Foo.m41
while a do b end
until a do end
loop do break end
x = a if b
return a if b
end
def Foo.m41
while a do b end
until a do end
loop do break end
x = a if b
return a if b
end

def Foo.m42
x = <<~E
  a \
    b #{c} \
  d
  e
E
y = <<-E
  a \
  b
E
end
def Foo.m42
x = <<~E
  a \
    b #{c} \
  d
  e
E
y = <<-E
  a \
  b
E
end
