class C
  def self.permission_types
    @_permission_types ||= Enum.new(full: 1, create_post: 2, readonly: 3)
  end
end
