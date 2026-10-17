# frozen_string_literal: true

RSpec.describe Foo do
  describe 'cases' do
    cases&.each do |c|
      it { expect(c).to be_ok }
    end
  end



  describe 'csend hook with example' do
    self&.before { it { 1 } }
  end

end
