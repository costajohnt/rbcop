if !(val = arg) and (argv.empty? or /\A-./ =~ (val = argv[0]))
  v
end
