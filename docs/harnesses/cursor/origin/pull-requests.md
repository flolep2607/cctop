# Pull requests

Origin is currently released in early beta. You can create repos, push and pull with git, mirror from GitHub, browse and search code, open and merge pull requests, and share with your Cursor team.

Please submit any and all feedback to [hi@cursor.com](mailto:hi@cursor.com) to help us make the product better.

Open a repository at [cursor.com/codebase](https://cursor.com/codebase) and select the **Pull Requests** tab to see open pull requests and open one to review it.

## Pull request list

The **Pull Requests** tab lists pull requests for the repository. Open a row to review activity, commits, checks, and changed files.

## Open a pull request

Push a branch to Origin, then open a pull request against the base branch:

```bash
git checkout -b my-change
git push -u origin my-change
```

From the repository **Code** tab, use the create-pull-request flow after your branch is on Origin. Cursor cloud agents can also open pull requests on Origin as part of a task.

## Pull request page

Open a pull request from the list to review it. Each pull request has four tabs:

- **Activity** — opens, comments, reviews, status changes, and other activity in order
- **Commits** — commits in the pull request; open one to inspect its diff
- **Checks** — check runs and status for the branch; results appear as checks report for the head commit
- **Files Changed** — the file diff; comment on lines and leave reviews from the diff

Along with the tabs, you can request reviewers, leave reviews, comment on the pull request or on individual lines, and merge once reviews and CI are satisfied. Origin surfaces merge conflicts so you can resolve them before merging.

## Mirrored GitHub repositories

On a repository [mirrored from GitHub](https://cursor.com/docs/origin/mirror-github.md), you can see and interact with GitHub pull requests as if they were Origin pull requests, and your changes sync back to GitHub.

Pull requests opened on a repository created directly on Origin stay on Origin. They are not mirrored anywhere.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
