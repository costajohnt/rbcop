# A stale heredoc hash differs from its eql? twin. Array#uniq (DuplicateHashKey,
# IdenticalConditionalBranches) on 8 or fewer elements matches keys by one byte of the
# per-process random hash and then eql?, so RuboCop flags such a pair once in 256 runs; 9+ keys
# or branches compare the full hash, which is what these cases test (#785).
h = { <<~K => 1, "k\n" => 2, a: 0, b: 0, c: 0, d: 0, e: 0, f: 0, g: 0 }
  k
K
s = Set[<<~S, "s\n"]
  s
S
u = { <<~U => 1, "u\n" => 2 }
u
U
p h, s, u
