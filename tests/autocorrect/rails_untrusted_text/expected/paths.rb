Rails.root.join("app/models")
Rails.root.join("app/a'b")
Rails.root.join('app', "a\nb")
Rails.root.join('app', "a\\b")
Rails.root.join("a'b").to_s
"#{Rails.root.join("a\"b")}"
"#{Rails.root.join("config")}"
x = <<~P
  #{Rails.root}/a'b
P
"#{Rails.root.join('x')}.r'b"
"#{Rails.root.join('x.rb')}"
Rails.env.production?
Rails.env == "pro duction"
Rails.env == "a; system('x')"
task foo: :environment do
end
task :"a b" do
end
task ab: :environment do
end
class UsersController < ApplicationController
  def show
    t("users.show.a'b")
    t('.title')
  end
end
%{#{Rails.root}/a"b}
