# frozen_string_literal: true

RSpec.describe Foo do
  it 'works' do
    users = 3.times.map { create(:user) }
    expect(current_path).to eq('/x')
    expect(response).to have_http_status(:ok)
    expect(users).to be_present
  end
end
