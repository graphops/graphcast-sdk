
# Contributing to the Graphcast SDK

Welcome to the Graphcast SDK! Thanks a ton for your interest in contributing.

If you run into any problems feel free to create an issue. PRs are much appreciated for simple things. If it's something more complex we'd appreciate having a quick chat in GitHub Issues or the Graph Discord server.

Join the conversation on [the Graph Discord](https://thegraph.com/discord).

Please follow the [Code of Conduct](https://github.com/graphops/graphcast-sdk/blob/main/CODE_OF_CONDUCT.md) for all the communications and at events. Thank you!

## Commit messages and pull requests

We use the following format for commit messages:
`{Brief description of changes}`, for example: `My great new change`.

The body of the message can be terse, with just enough information to
explain what the commit does overall. In a lot of cases, more extensive
explanations of _how_ the commit achieves its goal are better as comments
in the code.

Commits in a pull request should be structured in such a way that each
commit consists of a small logical step towards the overall goal of the
pull request. Your pull request should make it as easy as possible for the
reviewer to follow each change you are making. For example, it is a good
idea to separate simple mechanical changes like renaming a method that
touches many files from logic changes. Your pull request should not be
structured into commits according to how you implemented your feature,
often indicated by commit messages like 'Fix problem' or 'Cleanup'. Flex a
bit, and make the world think that you implemented your feature perfectly,
in small logical steps, in one sitting without ever having to touch up
something you did earlier in the pull request. (In reality, that means
you'll use `git rebase -i` a lot).

Please do not merge main into your branch as you develop your pull
request; instead, rebase your branch on top of the latest main if your
pull request branch is long-lived.

We try to keep the hostory of the `main` and `dev` branch linear, and avoid merge
commits. Once your pull request is approved, merge it following these
steps:
```
git checkout dev
git pull dev
git rebase dev my/branch
git push -f
git checkout dev
git merge my/branch
git push
```

Allegedly, clicking on the `Rebase and merge` button in the Github UI has
the same effect.

## Release process

We would like to keep `main` branch for official releases while using `dev` branch as the default upstream branch for features. Therefore ongoing development, feature work, and bug fixes will takes place on the dev branch, and only merge release tag commits to the `main` branch.

To start working on a new feature or bug fix, contributors should create a new branch off of the dev branch. Once the feature or bug fix is complete, a pull request should be created to merge the changes into the dev branch. All changes to the dev branch should be reviewed by at least one other person before merging.

When it's time to create a new release, we will merge the changes from the dev branch into the `main` branch using a pull request with new version tag (ex. `v0.1.0`). This pull request should be reviewed by at least one project owner before merging. Once the changes are merged into `main`, we will create a new tag for the release and use this tag to generate release notes and create a release in GitHub. To release a new version in crates.io, we update the version in Cargo.toml, utilize the package [keepachangelog](https://keepachangelog.com/en/1.1.0/) to maintain the changelog markdown by simply running the script under `scripts/release.sh` with the new version tag.

```
git tag -a vX.X.X -m "vX.X.X"
git push --follow-tags
```

It's important to note that all changes to the `main` branch should go through pull requests and be reviewed by at least one project admin. This helps ensure that the `main` branch only contains clean releases and that any issues or bugs are caught before they are released to our users.

By following this release process, we can keep our repository organized, ensure that our releases are clean and stable, and make it easier for contributors to work on new features and bug fixes.
