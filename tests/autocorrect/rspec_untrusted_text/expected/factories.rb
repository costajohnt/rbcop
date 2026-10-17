FactoryBot.define do
  factory :post do
    user
    author factory: %i[user]
    association :"a; system('x')"
    association :editor, factory: :"user; system('x')"
    association :owner, "it's": 1
  end
end
