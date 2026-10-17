def m
  x.each do
    raise <<-EOM
      |#{it}
      |b
    EOM
  end
end
def m5
  x.each do
    raise <<-EOM
      |#{_1}
      |b
    EOM
  end
end
def m4
  x.each do
    raise <<-EOM
      |a
      |b
    EOM
  end
end
def m6
  x.each do
    raise <<-EOM
      |a
      |b
    EOM
    it
  end
end
def m7
  x.each do
    raise <<~EOM
      #{_1}
    EOM
  end
end
