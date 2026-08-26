# Windsurf Ignore

Configure which files and directories Windsurf should ignore during indexing using .codeiumignore files with gitignore-style syntax.

## WindsurfIgnore

By default, Windsurf Indexing will ignore:

* Paths specified in `gitignore`
* Files in `node_modules`
* Hidden pathnames (starting with ".")

When a file is ignored, it will not be indexed, and also does not count against the Indexing Max Workspace Size file counts.
Files included in .gtiignore cannot be edited by Cascade.

If you want to further configure files that Windsurf Indexing ignores, you can add a `.codeiumignore` file to your repo root, with the same syntax as `.gitignore`

<Frame>
  <img />
</Frame>

### Global .codeiumignore

For enterprise customers managing multiple repositories, you can enforce ignore rules across all repositories by placing a global `.codeiumignore` file in the `~/.codeium/` folder. This global configuration will apply to all Windsurf workspaces on your system.

The global `.codeiumignore` file uses the same syntax as `.gitignore` and works in addition to any repository-specific `.codeiumignore` files.

## System Requirements

When first enabled, Windsurf will consume a fraction of CPU while it indexes the workspace. Depending on your workspace size, this should take 5-10 minutes, and only needs to happen once per workspace. CPU usage will return to normal automatically. Windsurf Indexing also requires RAM (\~300MB for a 5000-file workspace).

The "Max Workspace Size (File Count)" setting determines the largest workspace for which Windsurf Indexing will try to index a particular workspace / module. If your workspace does not appear to be indexed, please try adjusting this number higher. For users with \~10GB of RAM, we recommend setting this no higher than 10,000 files.
