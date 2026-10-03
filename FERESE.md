# Ferese fork

`ferese` is the default branch and contains Ferese's changes. `master` tracks
[pop-os/iced](https://github.com/pop-os/iced). Keep local changes on `ferese`
or a feature branch.

The **Sync upstream** workflow updates the upstream tracking branch every Monday.
It can also be run from the Actions page. It reads upstream's default branch,
so it follows a future rename from `master` to `main`. It refuses to overwrite
local commits on the tracking branch and does not merge into `ferese`.

To merge an upstream update in a checkout with an `upstream` remote:

```sh
git fetch upstream
git switch ferese
git merge upstream/master
```

Use `upstream/main` if upstream renames its branch. Resolve conflicts, run the
relevant tests, then push `ferese`. Ferese applications pin tested commits;
updating this fork does not automatically change an installed desktop.
