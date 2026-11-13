if (case v
    in Integer if foo &&
                  bar
      true
    end)
  1
end

while (case v
       in Integer if foo ||
         bar
         true
       end)
  2
end

case v
in Integer if foo.any? do |e|
                e > 1
              end
  3
in String if bar.map do |e|
  e
end.any?
  4
end
