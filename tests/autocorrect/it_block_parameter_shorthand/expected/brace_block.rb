foo.each {  { it: } }
foo.each {  bar(it:, x: 1) }
foo.each {  bar(post: it) }
foo.each { |post| bar(:post, posts) }
