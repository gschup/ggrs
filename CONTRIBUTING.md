# Contribution Guidelines

First and foremost: Thank you for showing interest in contributing to GGRS! Make sure to read the [Code of Conduct](./CODE_OF_CONDUCT.md).
If you have a cool example or showcase of GGRS in use, let me know so your project can be highlighted!

## Create an issue

- [Bug report](https://github.com/gschup/ggrs/issues/new?assignees=&labels=bug&template=bug_report.md&title=)
- [Feature request](https://github.com/gschup/ggrs/issues/new?assignees=&labels=enhancement&template=feature_request.md&title=)

## Contribute to GGRS

Please send a [GitHub Pull Request](https://github.com/gschup/ggrs/pull/new/main) with a clear list of what you've done
(read more about [pull requests](http://help.github.com/pull-requests/)). When you send a pull request,
it would be great if you wrote unit- or integration tests for your changes. Please format your code via `cargo fmt` and
make sure all of your commits are atomic (one feature per commit).

Always write a clear log message for your commits. One-line messages are fine for small changes, but bigger changes should look like this:

    >$ git commit -m "prefix: brief summary of the commit
    > 
    > A paragraph describing what changed and its impact."

With the following prefixes commonly used:

- `feat`: for new features
- `fix`: for fixing a bug
- `doc`: for adding/changing documentation
- `test`: for adding/changing tests
- `chore`: for any minor code cleanups

More about the [GitHub flow](https://guides.github.com/introduction/flow/).
More about the [Conventional Commits Specification](https://www.conventionalcommits.org/en/v1.0.0/)
