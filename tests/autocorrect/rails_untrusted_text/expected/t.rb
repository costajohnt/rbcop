tag("div\nsystem('x')")
tag(:"a b")
tag("a'b")
tag("a\\b")
tag("x#{y}")
tag("a;b")
tag("é")
tag(<<~X)
  div
X
tag.div()
tag("foo\"bar")
tag.foo_bar(class: 'x')
tag(:"div\nexit!")
t('key', scope: [:"a'b", :c])
t(:key, scope: :"x\" + system('y') + \"")
t('k"ey', scope: :a)
t('k\\ey', scope: 'a')
t("a#{b}", scope: :c)
t('x.1.2.5.abc')
I18n.t('é', scope: [:"\n"])
t(<<~K, scope: :a)
  key
K
link_to 'x', url, target: '_blank', rel: "foo'bar"
link_to 'x', url, target: '_blank', rel: 'a"b'
link_to 'x', url, target: '_blank', rel: "a\\b"
link_to 'x', url, target: "_blank", rel: "noopener"
link_to 'x', url, target: :_blank, rel: :noopener
link_to 'x', url, target: %q(_blank)
link_to 'x', url, target: '_blank', rel: %q(nofollow)
link_to 'x', url, target: '_blank', rel: <<~R
  nofollow
R
link_to 'x', url, target: '_blank', rel: "x\ny"
