def Foo.m0
[1].each { |x| x }
end
def Foo.m0
[1].each { |x| x }
end

def Foo.m1
x { _1 + _2 }
-> { _1 }
proc { _3 }
end
def Foo.m1
x { _1 + _2 }
-> { _1 }
proc { _3 }
end

def Foo.m2
x { it + 1 }
foo.map { it }
-> { it }
end
def Foo.m2
x { it + 1 }
foo.map { it }
-> { it }
end

def Foo.m3
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
def Foo.m3
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

def Foo.m4
{"": 1}
foo("": 1)
_1
end
def Foo.m4
{"": 1}
foo("": 1)
_1
end
