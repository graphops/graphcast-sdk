
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

We try to keep the hostory of the `main` branch linear, and avoid merge
commits. Once your pull request is approved, merge it following these
steps:
```
git checkout main
git pull main
git rebase main my/branch
git push -f
git checkout main
git merge my/branch
git push
```

Allegedly, clicking on the `Rebase and merge` button in the Github UI has
the same effect.
