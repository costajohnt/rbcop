if a && /x/i =~ v
    foo
  end

if a && !/x/ =~ v
    foo
  end

if a && /(?<cap>x)/ =~ v
    foo
  end

if a && (/x#{y}/ =~ v)
    foo
  end

if a && (v =~ /x/)
    foo
  end

if a && b && /x/ =~ v
    foo
  end

if a && /x/ =~ v
  foo
end

if /x/ =~ v && a
  foo
end

if !/x/ =~ v && !a
  foo
end

if /x/ =~ v && /y/ =~ w
    foo
  end
