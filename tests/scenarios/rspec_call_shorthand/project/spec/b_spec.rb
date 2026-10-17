# frozen_string_literal: true

RSpec.describe Foo do
  subject { y.() }

  let(:x) { y.(1) }

  before { y.() }

  it do
    y.()
    expect(y.()).to eq 1
    allow(x).to receive(:call) do
      y.()
    end
    instance.summable.(query, true)
    x&.()
    y.() { 1 }
    y.() do
      1
    end
    should eq 1
  end

  it { expect(y.()).to be_ok }

  describe 'z' do
    it { y.().().(&b) }
  end
  y.()
end
