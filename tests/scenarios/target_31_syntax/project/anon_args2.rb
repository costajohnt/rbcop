def foo(*, **, &)
  with_block(*, **) do
    bar(baz(*, **, &))
  end
end
