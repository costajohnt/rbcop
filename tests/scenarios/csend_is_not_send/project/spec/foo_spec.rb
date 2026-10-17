# frozen_string_literal: true

RSpec.describe Foo do
  describe 'cases' do
    cases&.each do |c|
      it { expect(c).to be_ok }
    end
  end

  describe 'hooks' do
    before { 1 }
  end

  describe 'csend hook' do
    self&.before { 1 }
  end

  describe 'csend hook with example' do
    self&.before { it { 1 } }
  end

  describe 'numbered' do
    cases&.each { it { expect(_1).to be_ok } }
  end
end
