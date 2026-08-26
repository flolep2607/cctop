# C#, .NET, and CPP

Setup guide for C#, .NET Core, .NET Framework (Mono), and C++ development in Windsurf using open-source tooling like OmniSharp, clangd, and LLDB.

# Windsurf Development Environment Setup Guide

## Overview

Windsurf workspaces rely **exclusively on open‑source tooling** for compiling, linting, and debugging. Microsoft's proprietary Visual Studio components cannot be redistributed, so we integrate community‑maintained language servers, debuggers, and compilers instead.

This guide covers two stacks:

1. **.NET / C#** – targeting both .NET Core and .NET Framework (via Mono)
2. **C / C++** – using clang‑based tooling

You can install either or both in the same workspace.

> ⚠️ **Important**: The examples below are templates that you must customize for your specific project. You'll need to edit file paths, project names, and build commands to match your codebase.

***

## 1. .NET / C# development

> **Choose the flavour that matches your codebase.**

### .NET Core / .NET 6+

**Extensions:**

* **[C#](https://marketplace.windsurf.com/vscode/item?itemName=muhammad-sammy.csharp)** (`muhammad-sammy.csharp`) – bundles **OmniSharp LS** and **NetCoreDbg**, so you can hit <kbd>F5</kbd> immediately

* **[.NET Install Tool](https://marketplace.windsurf.com/vscode/item?itemName=ms-dotnettools.vscode-dotnet-runtime)** (`ms-dotnettools.vscode-dotnet-runtime`) – auto‑installs missing runtimes/SDKs

* **[Solution Explorer](https://marketplace.windsurf.com/vscode/item?itemName=fernandoescolar.vscode-solution-explorer)** (`fernandoescolar.vscode-solution-explorer`) – navigate and manage .NET solutions and projects

**Debugger:** Nothing else is required—the extension already contains the language server and an open‑source debugger suitable for .NET Core.

**Build:** `dotnet build`

### .NET Framework via Mono

**Extensions:**

* **[Mono Debug](https://marketplace.windsurf.com/vscode/item?itemName=chrisatwindsurf.mono-debug)** (`chrisatwindsurf.mono-debug`) – debug adapter for Mono ([Open VSX](https://open-vsx.org/extension/chrisatwindsurf/mono-debug))
* **[C#](https://marketplace.windsurf.com/vscode/item?itemName=muhammad-sammy.csharp)** (`muhammad-sammy.csharp`) for language features

**Debugger:** **You must also install the Mono tool‑chain inside the workspace.** Follow the install guide in the [Mono repo](https://gitlab.winehq.org/mono/mono#compilation-and-installation). The debugger extension connects to that runtime at debug time.

> **⚠️ .NET Framework Configuration**: After installing Mono, to use the C# extension with .NET Framework projects, you need to toggle a specific setting in the IDE Settings. Go to **Settings** (in the C# Extension section) and toggle off  **"Omnisharp: Use Modern Net"**. This setting uses the OmniSharp build for .NET 6, which provides significant performance improvements for SDK-style Framework, .NET Core, and .NET 5+ projects. Note that this version *does not* support non-SDK-style .NET Framework projects, including Unity.

**Build:** `mcs Program.cs`

### Configure `tasks.json` for Your Project

**You must create/edit `.vscode/tasks.json` in your workspace root** and customize these templates:

```jsonc theme={null}
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "build-dotnet",
      "type": "shell",
      "command": "dotnet",
      "args": ["build", "YourProject.csproj"], // ← Edit this
      "group": "build",
      "problemMatcher": "$msCompile"
    },
    {
      "label": "build-mono",
      "type": "shell",
      "command": "mcs",
      "args": ["YourProgram.cs"], // ← Edit this
      "group": "build"
    }
  ]
}
```

### Configure `launch.json` for Debugging

**You must create/edit `.vscode/launch.json` in your workspace root** and update the paths:

```jsonc theme={null}
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": ".NET Core Launch",
      "type": "coreclr",
      "request": "launch",
      "preLaunchTask": "build-dotnet",
      "program": "${workspaceFolder}/bin/Debug/net6.0/YourApp.dll", // ← Edit this path
      "cwd": "${workspaceFolder}",
      "args": [] // Add command line arguments if needed
    },
    {
      "name": "Mono Launch",
      "type": "mono",
      "request": "launch",
      "preLaunchTask": "build-mono",
      "program": "${workspaceFolder}/YourProgram.exe", // ← Edit this path
      "cwd": "${workspaceFolder}"
    }
  ]
}
```

### CLI equivalents

```bash theme={null}
# .NET Core
$ dotnet build
$ dotnet run

# Mono / .NET Framework
$ mcs Program.cs
$ mono Program.exe
```

### .NET Framework Limitations

⚠️ **Important**: .NET Framework codebases with mixed assemblies (C++/CLI) or complex Visual Studio dependencies have significant limitations in Windsurf. These codebases typically require Visual Studio's proprietary build system and cannot be fully compiled or debugged in Windsurf due to dependencies on Microsoft-specific tooling and assembly reference resolution.

**Recommended approaches for .NET Framework projects:**

* Use Windsurf alongside Visual Studio for code generation and editing
* Migrate compatible portions to .NET Core where possible

***

## 2. C / C++ development

**Required Extensions:**

| Extension                                                                                                        | Purpose                                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **[Windsurf C++ Tools](https://open-vsx.org/extension/Codeium/windsurf-cpptools)** (`Codeium.windsurf-cpptools`) | This is a bundle of the three extensions we recommend using to get started. Package that contains C/C++ LSP support, debugging support, and CMake support. |

> **Note:** Installing the Windsurf C++ Tools bundle will automatically install the individual extensions listed below, so you only need to install the bundle.

| Extension                                                                                                                                           | Purpose                                                                                                                        |
| --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| **[clangd](https://marketplace.windsurf.com/vscode/item?itemName=llvm-vs-code-extensions.vscode-clangd)** (`llvm-vs-code-extensions.vscode-clangd`) | **clangd** language‑server integration. If `clangd` is missing it will offer to download the correct binary for your platform. |
| **[CodeLLDB](https://marketplace.windsurf.com/extension/vadimcn/vscode-lldb)** (`vadimcn.vscode-lldb`)                                              | Native debugger based on LLDB for C/C++ and Rust code.                                                                         |
| **[CMake Tools](https://marketplace.windsurf.com/vscode/item?itemName=ms-vscode.cmake-tools)** (`ms-vscode.cmake-tools`)                            | Project configuration, build, test, and debug integration for **CMake**‑based projects.                                        |

For non‑CMake workflows you can still invoke `make`, `ninja`, etc. via custom `tasks.json` targets.

### Configure C/C++ Build Tasks

**Create/edit `.vscode/tasks.json`** for your C/C++ project:

```jsonc theme={null}
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "build-cpp",
      "type": "shell",
      "command": "clang++",
      "args": ["-g", "main.cpp", "-o", "main"], // ← Edit for your files
      "group": "build",
      "problemMatcher": "$gcc"
    }
  ]
}
```

***

## 3. Notes & Gotchas

* **Open‑source only** – decline any prompt to install proprietary Microsoft tooling; Windsurf containers cannot ship it.
* **Container vs Host** – SDKs/compilers must be present **inside** the Windsurf workspace container.
* **Keyboard shortcuts**
  * <kbd>Ctrl/⌘ + Shift + B</kbd> → compile using the active build task
  * <kbd>F5</kbd> → debug using the selected `launch.json` config

***

## 4. Setup Checklist

* Install the required extensions for your language stack
* **Create and customize** `.vscode/tasks.json` with your project's build commands
* **Create and customize** `.vscode/launch.json` with correct paths to your executables
* For Mono: install the runtime and verify `mono --version`
* Update file paths, project names, and build arguments to match your codebase
* Test your setup: Press <kbd>Ctrl/⌘ + Shift + B</kbd> to build, then <kbd>F5</kbd> to debug

> 💡 **Tip**: The configuration files are project-specific. You'll need to adapt the examples above for each workspace.
