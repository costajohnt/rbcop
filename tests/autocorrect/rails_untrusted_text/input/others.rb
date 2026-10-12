Rails.root.join("public/a'b; system('x')")
Rails.root.join("public/#{x}")
Rails.root.join('public', "a\nb")
enum status: ["a'b", "c\"d", "é", "#{x}", :"e f", %q(g)]
enum kind: [:"a\nb", 'x#{y}']
render text: "a'b", status: 200
render text: <<~T
  body
T
has_many :posts, class_name: Post.name
has_many :tags, class_name: :"x'y".to_s
belongs_to :user, class_name: Foo::Bar.to_s
User.where(id: "1'); system('x')").take!
User.find_by!(id: "a\nb")
Rails.root.join("a'b").join("c\nd")
travel_to(Time.now, &blk)
User.order("name; DROP desc", "x")
User.order("na'me")
Rails.logger.debug "a#{b}'c"
Rails.logger.debug("a#{b}\nc")
ActiveRecord::Base.include(Module.new { system("x") })
add_column :users, :name, :string, index: { name: "a'b\nc" }
redirect_back fallback_location: "a'b\n", notice: 'x'
User.select(:"a'b").map(&:"a'b")
User.pluck(:"a b").first
x = 1.day.ago.since("a'b")
X = 1.day.since; Y = 2
