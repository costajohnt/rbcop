def find_zone(z)
  find_zone!(z) rescue nil
end

def a
  (b rescue c)
end

def d
  e
rescue StandardError
  f
end

def g = h rescue i

def j
  k
end
