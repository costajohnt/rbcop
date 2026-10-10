class Archive
  def up
    last_id = 0

    loop do
      last = DB.query_single(<<~SQL, batch_size: 10, last_id:).first
        SELECT id
        FROM events
        WHERE id > :last_id
      SQL

      break if last.nil?
      last_id = last
    end
  end

  def down
    raise IrreversibleMigration
  end
end
