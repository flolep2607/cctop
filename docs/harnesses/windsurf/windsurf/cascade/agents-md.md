# AGENTS.md

Create AGENTS.md files to provide directory-scoped instructions to Cascade. Instructions automatically apply based on file location in your project.

`AGENTS.md` files provide a simple way to give Cascade context-aware instructions that automatically apply based on where the file is located in your project. This is particularly useful for providing directory-specific coding guidelines, architectural decisions, or project conventions.

## How It Works

When you create an `AGENTS.md` file (or `agents.md`), Windsurf automatically discovers it and feeds it into the same [Rules](/windsurf/cascade/memories#rules) engine that powers `.windsurf/rules/` — just with the activation mode inferred from the file's location instead of frontmatter:

* **Root directory**: Treated as an **always-on** rule — the full content is included in Cascade's system prompt on every message.
* **Subdirectories**: Treated as a **glob** rule with an auto-generated pattern of `<directory>/**` — the content is applied only when Cascade reads or edits files inside that directory.

This location-based scoping makes `AGENTS.md` ideal for providing targeted guidance without cluttering a single global configuration file.

## Creating an AGENTS.md File

Simply create a file named `AGENTS.md` or `agents.md` in the desired directory. The file uses plain markdown with no special frontmatter required.

### Example Structure

```
my-project/
├── AGENTS.md                    # Global instructions for the entire project
├── frontend/
│   ├── AGENTS.md                # Instructions specific to frontend code
│   └── src/
│       └── components/
│           └── AGENTS.md        # Instructions specific to components
├── backend/
│   └── AGENTS.md                # Instructions specific to backend code
└── docs/
    └── AGENTS.md                # Instructions for documentation
```

### Example Content

Here's an example `AGENTS.md` file for a React components directory:

```markdown theme={null}
# Component Guidelines

When working with components in this directory:

- Use functional components with hooks
- Follow the naming convention: ComponentName.tsx for components, useHookName.ts for hooks
- Each component should have a corresponding test file: ComponentName.test.tsx
- Use CSS modules for styling: ComponentName.module.css
- Export components as named exports, not default exports

## File Structure

Each component folder should contain:
- The main component file
- A test file
- A styles file (if needed)
- An index.ts for re-exports
```

## Discovery and Scoping

Windsurf automatically discovers `AGENTS.md` files throughout your workspace:

* **Workspace scanning**: All `AGENTS.md` files within your workspace and its subdirectories are discovered
* **Git repository support**: For git repositories, Windsurf also searches parent directories up to the git root
* **Case insensitive**: Both `AGENTS.md` and `agents.md` are recognized

### Automatic Scoping

The key benefit of `AGENTS.md` is automatic scoping based on file location:

| File Location           | Scope                                                        |
| ----------------------- | ------------------------------------------------------------ |
| Workspace root          | Applies to all files (always on)                             |
| `/frontend/`            | Applies when working with files in `/frontend/**`            |
| `/frontend/components/` | Applies when working with files in `/frontend/components/**` |

This means you can have multiple `AGENTS.md` files at different levels, each providing increasingly specific guidance for their respective directories.

## Best Practices

To get the most out of `AGENTS.md` files:

* **Keep instructions focused**: Each `AGENTS.md` should contain instructions relevant to its directory's purpose
* **Use clear formatting**: Bullet points, headers, and code blocks make instructions easier for Cascade to follow
* **Be specific**: Concrete examples and explicit conventions work better than vague guidelines
* **Avoid redundancy**: Don't repeat global instructions in subdirectory files; they inherit from parent directories

### Content Guidelines

```markdown theme={null}
# Good Example
- Use TypeScript strict mode
- All API responses must include error handling
- Follow REST naming conventions for endpoints

# Less Effective Example
- Write good code
- Be careful with errors
- Use best practices
```

## Comparison with Rules

While both `AGENTS.md` and [Rules](/windsurf/cascade/memories#rules) provide instructions to Cascade, they serve different purposes:

| Feature  | AGENTS.md                        | Rules                                            |
| -------- | -------------------------------- | ------------------------------------------------ |
| Location | In project directories           | `.windsurf/rules/` or global                     |
| Scoping  | Automatic based on file location | Manual (glob, always on, model decision, manual) |
| Format   | Plain markdown                   | Markdown with frontmatter                        |
| Best for | Directory-specific conventions   | Cross-cutting concerns, complex activation logic |

Use `AGENTS.md` when you want simple, location-based instructions. Use Rules when you need more control over when and how instructions are applied.
