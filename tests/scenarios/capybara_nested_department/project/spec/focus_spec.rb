# frozen_string_literal: true

describe Foo do
  fit { expect(page).to have_selector('a') } # rubocop:disable RSpec
  fit { expect(page).to have_selector('a') } # rubocop:disable RSpec/Focus
  fit { expect(page).to have_selector('a') } # rubocop:disable Capybara/RSpec
end
