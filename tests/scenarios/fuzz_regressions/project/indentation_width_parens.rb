# frozen_string_literal: true

class Foo
  (
    class Bar
      def up
        create_table :post do |t|
        end
        execute "x
               y"
        change_table :post do |t|
        end
      end
    end
  )
end
