RSpec.describe Foo do
  x.each do
    describe it::Bar do
      it { expect(described_class).to eq Foo }
      it { expect(Foo).to be_ok }
    end
  end
  x.each { create it }
  x.each { create(it) }
  x.each { let(it) { 1 } }
  x.each { subject(it) { 1 } }
  x.each { it { is_expected.to eq it } }
  x.each { include_examples it }
  x.each { it_behaves_like it }
  x.each { expect(it).to eq 1 }
  x.each { expect(it).to be_truthy }
  x.each { allow(it).to receive(:foo) }
end
