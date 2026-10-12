RSpec.describe User do
  it 'is valid' do
  end

  it "should run #{'it\'s'}" \
     " now" do
  end

  it 'is continued' do
  end

  it 'has space' do
  end

  it "has #{x} space" do
  end

  it 'works', plain: true, a b: true do
  end

  it 'expects' do
    expect { run }.to change { User.count }
    expect { run }.to change { User.count; system('x') }
    expect(name).to include('abc')
    expect(name).to include('a\b')
    expect(name).to include("it's #{x}")
  end

  it 'stubs' do
    allow(foo).to receive_messages(a: 1, b: 2)
    allow(bar).to receive_messages(c d: 1, e: 2)
    allow(foo).to receive(:a)
    allow(foo).to receive(:"b c")
    allow(foo).to receive("a; b").and_return(1)
  end

  it_behaves_like 'plain'
  it_behaves_like 'it's'

  it 'doubles' do
    instance_double(Foo)
    instance_double(Foo)
    instance_double(Foo; system('x'))
  end

  it 'creates' do
    create_list(:user, 3) { |u| u.touch }
    create_list(:user; system('x'), 3) { |u| u.touch }
  end

  it 'finds' do
    find_by_id('plain').click
    find_by_id('it's').click
    find_by_id('it's').click
    within "#a\\.b" do
    end
    within "#a"b" do
    end
    within %#a\.b% do
    end
    expect(page).to have_current_path /\/plain\/path/
    expect(current_path).to match "/a(b"
    expect(page).to have_current_path /\/#{x}\/y/
  end
end
