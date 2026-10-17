RSpec.describe Foo do
  it 'a' do
    x.each { expect(_1) }
    x.each { expect(it) }
    x.each { |v| expect(v) }
    x.each { expect(_1) { 1 } }
  end

  it 'b' do
    expect { x.each { raise _1 } }.to raise_error
    expect { x.each { raise it } }.to raise_error
    expect { x.each { |v| raise v } }.to raise_error
    x.each { expect { raise _1 }.to raise_error }
    x.each { expect { raise it }.to raise_error }
  end

  it 'c' do
    expect { _1 }.to raise_error
    expect { it }.to raise_error
    expect { _1 }.not_to raise_error
  end
end
