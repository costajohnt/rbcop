# Releasing

The repository is private, so releases are built by hand (hosted Actions minutes are not used);
`.github/workflows/release.yml` runs the same scripts once it is public. Nothing below publishes
anything until the last step.

1. Set `version` in `Cargo.toml` and tag `vX.Y.Z` after the gate (`scripts/ci.sh`) passes.
2. Linux archives, on a Linux x86_64 host with `zig` 0.13 and `cargo-zigbuild` on `PATH` and the
   four Rust targets installed (`rustup target add {x86_64,aarch64}-unknown-linux-{gnu,musl}`):

       scripts/dist/build.sh

3. macOS archives, on a Mac (`rustup target add aarch64-apple-darwin x86_64-apple-darwin`):

       scripts/dist/build.sh

4. Copy all six `dist/rbcop-*.tar.gz` and `.sha256` files into one `dist/`, then build the gems
   and the Homebrew formula:

       scripts/dist/gem.sh                     # dist/gems/*.gem: six platform gems and the `ruby` one
       scripts/dist/homebrew.sh > dist/rbcop.rb

5. Check before publishing, on each host: install the gem for that host and compare it with a
   cargo build on a corpus,

       gem install --local --install-dir /tmp/g dist/gems/rbcop-X.Y.Z-<platform>.gem
       GEM_HOME=/tmp/g scripts/dist/compare.sh target/release/rbcop /tmp/g/bin/rbcop \
         "$PWD/config/all-cops.yml" <corpus>/jekyll <corpus>/mastodon

   and the same for `dist/gems/rbcop-X.Y.Z.gem` (the `ruby` platform gem).
   `RBCOP_DOWNLOAD_BASE=file://$PWD/dist scripts/dist/install.sh X.Y.Z /tmp/b` checks the
   installer and the checksums.
6. Publish (manual, after review): `gh release create vX.Y.Z dist/*.tar.gz dist/*.sha256
   dist/gems/*.gem dist/rbcop.rb`, `gem push` each gem, and copy `dist/rbcop.rb` into the tap.
   The GitHub Action downloads from the release, so it works once the release exists.
