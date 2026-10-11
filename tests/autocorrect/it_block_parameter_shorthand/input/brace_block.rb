foo.each { |post| { post: } }
foo.each { |post| bar(post:, x: 1) }
foo.each { |post| bar(post: post) }
foo.each { |post| bar(:post, posts) }
