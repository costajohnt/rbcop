# frozen_string_literal: true

Foo.all.merge Bar.all
Foo.all.merge(Bar.all)
Foo.all.where(Bar.all)
Foo.merge(Bar.all)
Foo.all.where(Bar.all.x)
Foo.all.merge(Bar.all, Baz.all)
Foo.all.merge(Bar.all.all)
Foo.all.merge(Bar.all) { 1 }
Foo.all.where(x: Bar.all)
Foo.all.where(all)
ActiveSupport::TimeZone.all.where(Bar.all)
