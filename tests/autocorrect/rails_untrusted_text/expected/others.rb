Rails.root.join("public/a'b; system('x')")
Rails.root.join("public/#{x}")
Rails.public_path.join("a\nb")
enum status: {"a'b" => 0, "c\"d" => 1, "\u00E9" => 2, "#{x}" => 3, :"e f" => 4, "g" => 5}
enum kind: {:"a\nb" => 0, "x\#{y}" => 1}
render text: "a'b", status: 200
render text: <<~T
  body
T
has_many :posts, class_name: "Post"
has_many :tags, class_name: :"x'y".to_s
belongs_to :user, class_name: Foo::Bar.to_s
User.find("1'); system('x')")
User.find("a\nb")
Rails.root.join("a'b", "c\nd")
freeze_time(&blk)
User.order("name; DROP desc", "x")
User.order("na'me")
Rails.logger.debug { "a#{b}'c" }
Rails.logger.debug { "a#{b}\nc" }
ActiveSupport.on_load(:active_record) { include Module.new { system("x") } }
add_column :users, :name, :string
add_index :users, :name, name: "a'b\nc"
redirect_back_or_to("a'b\n", notice: 'x')
User.pluck(:"a'b")
User.pick(:"a b")
x = 1.day.ago.since("a'b")
def self.x
1.day.since
end; Y = 2
