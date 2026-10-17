class CreateX < ActiveRecord::Migration[7.0]
  def change
    create_table(:x) do |t|
      t&.integer :id
      t.integer :id
      t&.column :id, :integer
      t&.string :errors
    end
    add_column :x, :id, :integer
    self&.add_column :x, :id, :integer
    self&.rename_column :x, :a, :id
  end
end
