# Remote Indexing

Index remote repositories from GitHub, GitLab, and BitBucket for enterprise teams without storing code locally.

<Note> This feature is only available in the Windsurf Plugins for Enterprise plans. </Note>

While Local Indexing works great, the user may want to index codebases that they do not have stored locally for our models to take in as context.

For this use case, organizations on Teams and Enterprise plans can use Windsurf's Indexing Service to globally import all the relevant repositories. The indexing and embedding is then performed by Windsurf's servers (on an isolated tenant), and once the index is created, it is available to be queried by any member of the Team.

## Adding a repository

From [https://windsurf.com/indexing](https://windsurf.com/indexing) you can add a repository to index. Currently we support Git repositories from GitHub, GitLab, and BitBucket.

<Frame>
  <img />
</Frame>

You can choose to index a particular branch and to automatically re-index the repository after some number of days.

## Security Guarantees

We clone the repository in order to create the index, but once we finish creating embeddings for the codebase we delete all the code and code snippets **assuming that the Store Snippets setting is unchecked.** We don't persist anything other than the embeddings themselves, from which you cannot derive the original code.

Furthermore, all indexing and embedding is performed on a single-tenant instance—nothing about the indexing process is shared between multiple Windsurf Teams customers.
