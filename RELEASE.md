# Releasing the kubert crates

A release is a version bump, a `<crate>/v<version>` git tag, and a GitHub
release. Releases are not published to crates.io.

1. Set `version` in `<crate>/Cargo.toml`, refresh the lockfile with
   `cargo metadata --format-version=1 > /dev/null`, and commit both changes in a
   pull request titled `build: <crate>/v<version>`. A breaking change bumps the
   minor version; other changes bump the patch version.

2. Merge the pull request, then tag the merge commit and push the tag:

   ```bash
   git tag kubert/v0.27.0
   git push origin kubert/v0.27.0
   ```

   Use each crate's own tag prefix, for example
   `kubert-prometheus-process/v0.2.4`.

3. Check that the crate's release workflow succeeds and that a GitHub release
   named `<crate> v<version>` appears. If the tag does not match the version in
   `Cargo.toml`, the workflow fails before creating the release; delete the tag
   locally and on the remote, fix the version, and re-tag.
