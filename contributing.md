
# Contributing to the Graphcast SDK

Welcome to the Graphcast SDK! Thanks a ton for your interest in contributing.

If you run into any problems feel free to create an issue. PRs are much appreciated for simple things. If it's something more complex we'd appreciate having a quick chat in GitHub Issues or the Graph Discord server.

Join the conversation on [the Graph Discord](https://thegraph.com/discord).

Please follow the [Code of Conduct](https://github.com/graphops/graphcast-sdk/blob/main/CODE_OF_CONDUCT.md) for all the communications and at events. Thank you!

## Commit messages and pull requests

We follow [conventional commits](https://www.conventionalcommits.org/en/v1.0.0/). 

In brief, each commit message consists of a header, with optional body and footer:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

`<type>` must be one of the following:
- feat: A new feature
- fix: A bug fix
- docs: Documentation only changes
- style: Changes that do not affect the meaning of the code (white-space, formatting, missing semi-colons, etc)
- refactor: A code change that neither fixes a bug nor adds a feature
- perf: A code change that improves performance
- test: Adding missing tests
- chore: Changes to the build process or auxiliary tools and libraries such as documentation generation
- revert: If the commit reverts a previous commit, contains the header of the reverted commit. 

Make sure to include an exclamation mark after the commit type and scope if there is a breaking change.

`<scope>` optional and could be anything that specifies the place of the commit change, e.g. solver, [filename], tests, lib, ... we are not very restrictive on the scope. The scope should just be lowercase and if possible contain of a single word.

`<description>` contains succinct description of the change with imperative, present tense. don't capitalize first letter, and no dot (.) at the end.

`<body>` include the motivation for the change, use the imperative, present tense

`<footer>` contain any information about Breaking Changes and reference GitHub issues that this commit closes

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
git push --force-with-lease
git checkout dev
git merge my/branch
git push
```

Allegedly, clicking on the `Rebase and merge` button in the Github UI has
the same effect.

## Release process

TL;DR

```
1. Make sure both `dev` and `main` are up-to their respective head 
2. Checkout a release branch from `dev` branch 
	1. update version in Cargo.toml
	2. run `scripts/release.sh`
	3. After script finishes, push the branch. Squash the commit if there were multiple attempts and commits.
3. Open release PR, set merge target to `dev`. Do not delete the release branch. Rebase and merge. Make sure all tests pass.
4. Repeat step 3 for `main`, delete release branch afterwards.
5. Create Github release with target set to `main`. Generate release notes.
```

We would like to keep `main` branch for official releases while using `dev` branch as the default upstream branch for features. Therefore ongoing development, feature work, and bug fixes will takes place on the dev branch, and only merge release tag commits to the `main` branch.

To start working on a new feature or bug fix, contributors should create a new branch off of the dev branch. Once the feature or bug fix is complete, a pull request should be created to merge the changes into the dev branch. All changes to the dev branch should be reviewed by at least one other person before merging.

When it's time to create a new release, we will merge the changes from the dev branch into the `main` branch using a pull request with new version tag (ex. `v0.1.0`). This pull request should be reviewed by at least one project owner before merging. Once the changes are merged into `main`, we will create a new tag for the release and use this tag to generate release notes and create a release in GitHub. To release a new version in crates.io, we update the version in Cargo.toml, utilize the tool [orhun/git-cliff](https://github.com/orhun/git-cliff) to maintain the changelog markdown by simply running the script under `scripts/release.sh` with the new version tag.

```
git tag -a vX.X.X -m "vX.X.X"
git push --follow-tags
```

It's important to note that all changes to the `main` branch should go through pull requests and be reviewed by at least one project admin. This helps ensure that the `main` branch only contains clean releases and that any issues or bugs are caught before they are released to our users. 

By following this release process, we can keep our repository organized, ensure that our releases are clean and stable, and make it easier for contributors to work on new features and bug fixes.
