# frozen_string_literal: true

Foo.merge Bar.
Foo.merge(Bar.)
Foo.where(Bar.)
Foo.merge(Bar.all)
Foo.where(Bar.all.x)
Foo.merge(Bar., Baz.)
Foo.merge(Bar.all.)
Foo.merge(Bar.) { 1 }
Foo.where(x: Bar.all)
Foo.where(all)
ActiveSupport::TimeZone.allwhere(Bar.)
