case v
in Integer if foo &&
              bar then 1
in String if baz ||
  qux then 2
end

case v
when Integer then foo &&
                  bar
when String
  baz ||
    qux
end

w = case v
    when Integer
      foo +
      bar
    end
