# frozen_string_literal: true

class AddX < ActiveRecord::Migration[8.0]
  def change
    create_table :queries do |t|
      t.boolean :a, null: false, default: nil
      t.boolean :b, default: nil
      t.boolean :c, null: false
      t.boolean :d, default: nil, null: false
      t.boolean :f, default: true, null: false
    end
    add_column :queries, :e, :boolean, null: false, default: nil
  end
end
