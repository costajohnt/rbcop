FactoryBot.define do
  factory :post do
    user
    author factory: %i[user]
    a; system('x')
    editor factory: %i[user; system('x')]
    owner it's: 1
  end
end
