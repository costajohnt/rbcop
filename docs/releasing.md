# Releasing

The repository is private, so releases are built by hand (hosted Actions minutes are not used);
`.github/workflows/release.yml` runs the same scripts once it is public. Nothing below publishes
anything until step 7.

The GitHub Action and `scripts/dist/install.sh` only run an archive whose SHA-256 is listed in
`packaging/checksums/X.Y.Z.sha256` at the tag, so that file is committed before the tag exists,
from the exact archives that get uploaded. Builds are not byte-reproducible across hosts: never
rebuild between committing the checksums and uploading.

1. Set `version` in `Cargo.toml` after the gate (`scripts/ci.sh`) passes.
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
6. Pin the checksums and check the installer against them:

       (cd dist && sha256sum -- *.tar.gz rbcop.rb gems/*.gem | sed 's|  gems/|  |') \
         > packaging/checksums/X.Y.Z.sha256
       RBCOP_DOWNLOAD_BASE=file://$PWD/dist scripts/dist/install.sh X.Y.Z /tmp/b

   Commit the file, then tag that commit `vX.Y.Z`.
7. Publish (after review): `gh release create vX.Y.Z dist/*.tar.gz dist/rbcop.rb dist/gems/*.gem`,
   `gem push` each gem, and copy `dist/rbcop.rb` into the tap.

With the workflow (once public): run it with stage `build`; it attaches everything and a
`SHA256SUMS` to a draft release. Commit that `SHA256SUMS` as `packaging/checksums/X.Y.Z.sha256`,
publish the draft on that commit (which creates the tag), then run the workflow on the tag with
stage `publish-gems`. Gems go out through RubyGems trusted publishing: register `release.yml`
with environment `release` as a trusted publisher for the `rbcop` gem on rubygems.org (a pending
publisher before the first push), and give the `release` environment a required reviewer.
