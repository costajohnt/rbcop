# List every RuboCop cop with its default status, source file, size and mixins, as TSV.
# usage: ruby scripts/cop_inventory.rb > docs/cop-inventory.tsv
require "yaml"

gem_dir = Gem::Specification.find_by_name("rubocop").gem_dir
config = YAML.unsafe_load_file(File.join(gem_dir, "config/default.yml"))
cop_dir = File.join(gem_dir, "lib/rubocop/cop")

def snake(name)
  name.gsub(/([A-Z]+)([A-Z][a-z])/, '\1_\2').gsub(/([a-z\d])([A-Z])/, '\1_\2').downcase
end

puts %w[cop enabled lines mixins file].join("\t")
config.each do |name, params|
  next unless name.include?("/") && params.is_a?(Hash)

  dept, cop = name.split("/", 2)
  file = File.join(cop_dir, snake(dept), "#{snake(cop)}.rb")
  file = Dir.glob(File.join(cop_dir, "**", "#{snake(cop)}.rb")).first unless File.exist?(file)
  source = file ? File.read(file) : ""
  mixins = source.scan(/^\s*(?:include|extend)\s+([A-Z]\w*)/).flatten.uniq - %w[AutoCorrector]
  mixins << "AutoCorrector" if source.match?(/extend AutoCorrector/)
  rel = file ? file.delete_prefix("#{gem_dir}/") : "?"
  puts [name, params["Enabled"], source.lines.count, mixins.join(","), rel].join("\t")
end
