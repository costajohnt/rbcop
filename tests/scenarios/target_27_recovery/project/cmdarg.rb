it "x" do
  put "/u/#{user.name}.json", params: { a: { value:, locale: } }

  [1, 2].each do |t|
    get t
  end
end

it "y" do
  put "/u/#{user.name}.json", params: { names: }

  expect(response.status).to eq(200)

  expect do
    put "/u", params: { names: }
  end
end
