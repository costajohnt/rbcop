def foo(&)
  block_method do
    bar(&)
    baz(_1)
  end
end
