def site
  return @site_drop ||= 1
end

def a
  yield(@x ||= 1)
end

def b
  super(@y ||= 1)
end

def c
  [@z ||= 1]
end

def e
  next @v ||= 1
end

def f
  break @w ||= 1
end

def g
  return 1, @u ||= 2
end

def h
  super(@s ||= 1) { }
end

def i
  return
end
