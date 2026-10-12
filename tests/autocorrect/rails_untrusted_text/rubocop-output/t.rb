tag.div
system('x')()
tag(:"a b")
tag("a'b")
tag("a\\b")
tag.x#{y}()
tag("a;b")
tag("é")
tag.div
()
  div
X
tag.div()
tag("foo\"bar")
tag.foo_bar(class: 'x')
tag.div
exit!()
t('a'b.c.key')
t('x" + system('y') + ".key')
t('a.k"ey')
t('k\\ey', scope: 'a')
t("a#{b}", scope: :c)
t('x.1.2.5.abc')
I18n.t('
.é')
t('a.key
')
  key
K
link_to 'x', url, target: '_blank', rel: "foo'bar noopener"
link_to 'x', url, target: '_blank', rel: 'a"b noopener'
link_to 'x', url, target: '_blank', rel: "a\b noopener"
link_to 'x', url, target: "_blank", rel: "noopener"
link_to 'x', url, target: :_blank, rel: :noopener
link_to 'x', url, target: %q(_blank), rel: %noopener%
link_to 'x', url, target: '_blank', rel: %nofollow noopener)
link_to 'x', url, target: '_blank', rel: <nofollow
 noopenerR
  nofollow
R
link_to 'x', url, target: '_blank', rel: "x
y noopener"
