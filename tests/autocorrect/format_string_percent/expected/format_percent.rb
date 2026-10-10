def m(row, file_stats, a, b)
  row << "%.3f" % (file_stats[:time])
  row << "%d" % (a + b)
  row << "%d" % a.+(b)
  row << "%d" % (-a)
  row << "%d" % (!a)
  row << "%d" % (a[1, 2])
  row << "%s" % a&.[](1)
  row << "%d" % a.==(b)
  row << "%d" % a.foo
end
