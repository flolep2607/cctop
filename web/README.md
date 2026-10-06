# cctop's web UI

React + TypeScript + Tailwind + shadcn/ui, built by Vite into a single HTML
file with everything inlined: `../src/serve/assets/app/index.html`, which cctop
compiles into its binary. See "The web UI is a React app" in `../CLAUDE.md`.

```bash
npm ci
npm run dev     # proxied to a `cctop serve --no-token --port 7778`
npm run build   # commit the rebuilt ../src/serve/assets/app/index.html
npm run lint
```

Add shadcn components with `npx shadcn@latest add <name>`.
