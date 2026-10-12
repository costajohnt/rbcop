# Corrections that stay byte-identical to RuboCop (rubocop-output/safe.rb == expected/safe.rb).
params.require(:user).permit(:name, :age)
params.require('user').permit(:name)
params.require(key).permit(:name)
Dir.glob(Rails.root.join('app', '**', '*.rb'))
Dir[Rails.root.join('config', '*.yml')]
File.read(Rails.root.join('config', "a'b"))
Rails.root.join('app', "o'reilly")
Rails.root.join('app', 'models')
File.join(Rails.root, "a'b")
"#{Rails.root}/a\"b"
"#{Rails.root.join('x')}.rb"
Rails.env == 'production'
