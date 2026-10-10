class_eval <<-EOT, __FILE__, __LINE__ + 1
  # EXAMPLE: def capitalize(*params, &block)
  #   to_str.capitalize(*params, &block)
  # end

  def #{unsafe_method}[]#{unsafe_method}(*params, &block);  end
EOT
