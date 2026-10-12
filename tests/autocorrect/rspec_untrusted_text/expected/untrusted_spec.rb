RSpec.describe User do
  it 'is valid' do
  end

  it "should run #{'it\'s'}" \
     " now" do
  end

  it 'should\
 be continued' do
  end

  it 'has space' do
  end

  it "  has \#{x} space" do
  end

  it 'works', :"a b", plain: true do
  end

  it 'expects' do
    expect { run }.to change { User.count }
    expect { run }.to change(User, :"count; system('x')")
    expect(name).to include('abc')
    expect(name).to match(/a\\b/)
    expect(name).to match(/it's \#{x}/)
  end

  it 'stubs' do
    allow(foo).to receive_messages(a: 1, b: 2)
    allow(bar).to receive(:"c d").and_return(1)
    allow(bar).to receive(:e).and_return(2)
    allow(foo).to receive(:a)
    allow(foo).to receive(:"b c")
    allow(foo).to receive("a; b").and_return(1)
  end

  it_behaves_like 'plain'
  it_behaves_like :"it's"

  it 'doubles' do
    instance_double(Foo)
    instance_double(Foo)
    instance_double("Foo; system('x')")
  end

  it 'creates' do
    create_list(:user, 3) { |u| u.touch }
    3.times { create(:"user; system('x')") { |u| u.touch } }
  end

  it 'finds' do
    find_by_id('plain').click
    find("#it's").click
    find(:id, "it's").click
    within "#a\\.b" do
    end
    within find_by_id("a\"b") do
    end
    within find_by_id(%q(a.b)) do
    end
    expect(page).to have_current_path /\/plain\/path/
    expect(current_path).to match "/a(b"
    expect(current_path).to match "/#{x}/y"
  end
end
