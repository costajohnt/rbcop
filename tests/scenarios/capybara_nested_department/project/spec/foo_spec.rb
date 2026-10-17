# frozen_string_literal: true

describe Foo do
  it { expect(page).to have_selector('a') } # rubocop:disable Capybara
  it { expect(page).to have_selector('b') } # rubocop:disable Capybara/RSpec
  it { expect(page).to have_selector('c') } # rubocop:disable Capybara/RSpec/HaveSelector, Capybara/RSpec
  # rubocop:disable Capybara
  it { expect(page).to have_selector('d') }
  # rubocop:enable Capybara/RSpec
  it { expect(page).to have_selector('e') }
  # rubocop:enable Capybara
  it { expect(page).to have_selector('f') } # rubocop:disable RSpec
  it { expect(page).to have_selector('g') } # rubocop:todo Capybara/RSpec
end
