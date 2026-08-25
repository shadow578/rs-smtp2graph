# Release Checklist

- bump version number in `app/smtp2graph/Cargo.toml`
- create tag in git with `git tag "v<version>"`
- push tag to origin with `git push origin --tags`
- workflow automatically builds and releases binaries for Linux and Windows
