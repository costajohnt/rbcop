x = 1
y = 2
x ||= x #: Integer
y &&= y
x ||= x
z = [1]
z[0] = z[0] #: Integer
x, y = x, y #: [Integer, Integer]
@a ||= @a # comment
#: String
x = x
A ||= A #: Foo
