FactoryBot.define do
  factory :post do
    association :user
    association :author, factory: :user
    association :"a; system('x')"
    association :editor, factory: :"user; system('x')"
    association :owner, "it's": 1
  end
end
