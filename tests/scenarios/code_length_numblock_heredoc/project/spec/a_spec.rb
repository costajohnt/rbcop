RSpec.describe Foo do
  it 'a' do
    x.each do
      raise <<-EOM
        |#{it}
        |b
      EOM
    end
  end

  it 'b' do
    x.each do
      raise <<-EOM
        |#{_1}
        |b
      EOM
    end
  end

  it 'c' do
    x.each do
      raise <<-EOM
        |a
      EOM
      it
    end
  end

  it 'd' do
    x.each do |v|
      raise <<-EOM
        |#{v}
      EOM
    end
  end
end

class C
  x.each do
    raise <<-EOM
      |#{_1}
    EOM
  end
end

module M
  x.each do
    raise <<-EOM
      |#{it}
    EOM
  end
end

def k
  -> do
    raise <<-EOM
      |#{_1}
    EOM
  end
end

def l
  x.map { _1 }.each do
    raise <<~EOM
      #{it}
    EOM
  end
end
