# How to Contribute to GGRS

First and foremost: Thank you for showing interest in contributing to GGRS!

Please send a [GitHub Pull Request](https://github.com/gschup/ggrs/pull/new/main) with a clear list of what you've done (read more about [pull requests](http://help.github.com/pull-requests/)). When you send a pull request, it would be great if you wrote unit- or integration tests for your changes. Please format your code via `cargo fmt` and make sure all of your commits are atomic (one feature per commit).

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
