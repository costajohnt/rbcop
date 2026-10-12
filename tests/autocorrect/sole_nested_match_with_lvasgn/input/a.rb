if a
  if /x/i =~ v
    foo
  end
end

if a
  unless /x/ =~ v
    foo
  end
end

if a
  if /(?<cap>x)/ =~ v
    foo
  end
end

if a
  if /x#{y}/ =~ v
    foo
  end
end

if a
  if v =~ /x/
    foo
  end
end

if a
  if b && /x/ =~ v
    foo
  end
end

if a
  foo if /x/ =~ v
end

if /x/ =~ v
  foo if a
end

unless /x/ =~ v
  foo unless a
end

if /x/ =~ v
  if /y/ =~ w
    foo
  end
end
