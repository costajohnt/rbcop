Rails.root.join('app', 'models')
Rails.root.join('app', "a'b")
Rails.root.join('app', "a\nb")
Rails.root.join('app', "a\\b")
File.join(Rails.root, "a'b")
"#{Rails.root}/a\"b"
"#{Rails.root}/config"
x = <<~P
  #{Rails.root}/a'b
P
"#{Rails.root.join('x')}.r'b"
"#{Rails.root.join('x')}.rb"
Rails.env == 'production'
Rails.env == "pro duction"
Rails.env == "a; system('x')"
task :foo do
end
task :"a b" do
end
task :"a'b" do
end
class UsersController < ApplicationController
  def show
    t("users.show.a'b")
    t('users.show.title')
  end
end
