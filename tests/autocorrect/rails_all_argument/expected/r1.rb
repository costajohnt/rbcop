# frozen_string_literal: true

Foo.merge Bar.all
Foo.merge(Bar.all)
Foo.where(Bar.all)
Foo.merge(Bar.all)
Foo.where(Bar.all.x)
Foo.merge(Bar.all, Baz.all)
Foo.merge(Bar.all.all)
Foo.merge(Bar.all) { 1 }
Foo.where(x: Bar.all)
Foo.where(all)
ActiveSupport::TimeZone.all.where(Bar.all)
