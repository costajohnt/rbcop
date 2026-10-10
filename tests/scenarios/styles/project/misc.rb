require 'debug'
my_debug
byebug
# a comment line that is long enough to exceed the sixty column limit for sure
x = 'SKIP this line even though it is longer than sixty characters in total'
y = 'ftp://example.com/a/very/long/path/that/goes/on/and/on/and/on/forever/x'
z = 'http://example.com/a/very/long/path/that/goes/on/and/on/and/on/forever/x'
puts x, y, z

