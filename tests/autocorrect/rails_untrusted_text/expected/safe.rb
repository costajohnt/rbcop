# Corrections that stay byte-identical to RuboCop (rubocop-output/safe.rb == expected/safe.rb).
params.expect(user: [:name, :age])
params.expect(user: [:name])
params.expect(key => [:name])
Rails.root.glob("app/**/*.rb")
Rails.root.glob("config/*.yml")
Rails.root.join("config/a'b").read
Rails.root.join("app/o'reilly")
Rails.root.join("app/models")
Rails.root.join("a'b").to_s
"#{Rails.root.join("a\"b")}"
"#{Rails.root.join('x.rb')}"
Rails.env.production?
%{#{Rails.root.join("config/app.rb")}}
