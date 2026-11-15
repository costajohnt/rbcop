# Releasing

Releases are built by `.github/workflows/release.yml` (see the end of this file). The manual steps
below run the same scripts by hand; nothing below publishes anything until step 7.

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

   Commit the file, then tag that commit and push the tag: `git tag vX.Y.Z <sha> && git push origin
   vX.Y.Z`.
7. Publish (after review): `gh release create vX.Y.Z --verify-tag dist/*.tar.gz dist/rbcop.rb
   dist/gems/*.gem` (`--verify-tag` refuses to run without the pushed tag; otherwise gh would create
   it from the default branch's tip, not the checksums commit), `gem push` each gem, and copy
   `dist/rbcop.rb` into the tap.

With the workflow: run it with stage `build`; it attaches everything and a
`SHA256SUMS` to a draft release. Commit that `SHA256SUMS` as `packaging/checksums/X.Y.Z.sha256`,
point the draft at that commit (`gh release edit vX.Y.Z --target <sha>`: the workflow creates it
against the default branch, whose tip can be another commit by then), publish it (which creates the
tag there), then run the workflow on the tag with stage `publish-gems`. Gems go out through RubyGems trusted publishing: register `release.yml`
with environment `release` as a trusted publisher for the `rbcop` gem on rubygems.org (a pending
publisher before the first push), and give the `release` environment a required reviewer.

The `attest` job runs after `draft-release` and attests every file in `SHA256SUMS` with
`actions/attest-build-provenance`; wait for it to finish before publishing the draft. Archives
built and uploaded by hand (steps 2 to 7) get no attestation. Check one with

    gh attestation verify rbcop-X.Y.Z-<target>.tar.gz -R costajohnt/rbcop \
      --signer-workflow costajohnt/rbcop/.github/workflows/release.yml

(`gh` 2.49 or newer, signed in; the attestations are public). Releases up to 0.1.4 have none.

## glibc floor

The `*-linux-gnu` archives target glibc 2.17 (`cargo zigbuild --target <target>.2.17` in
`scripts/dist/build.sh`), and `build.sh` fails if the binary needs a newer `GLIBC_` symbol version.
They link only `libc.so.6`, `libpthread.so.0` and `libdl.so.2`. Measured on the published 0.1.4
archives with `readelf --dyn-syms -W rbcop | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1`:
`GLIBC_2.17` for both x86_64 and aarch64. Raising the floor is a minor release and goes in the
release notes and README.

## Versions

rbcop is 0.x. Every published release is meant to be pinned: a tag is never moved and a release's
files are never replaced (their checksums are committed before the tag). From the release after
0.1.4 on:

- A patch release (0.Y.Z) fixes bugs only. A parity fix can change output toward what RuboCop
  1.92.2 prints for the same input; nothing else about options, configuration or platforms changes.
- A minor release (0.Y.0) may change the RuboCop target version, options, configuration handling,
  supported platforms or the glibc floor; its notes say which.

Earlier patch releases did not follow this (0.1.4 moved the target from RuboCop 1.92.1 to 1.92.2).
