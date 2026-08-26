# Endpoint Security Configuration

Cursor loads JavaScript modules and performs file I/O during startup. Endpoint security software that intercepts file operations or injects into processes can slow startup past internal timeouts, causing features like Agent to fail. This page covers how to configure exclusions so Cursor works alongside your security stack.

## What to exclude

Add the following processes and paths to your security product's exclusion list.

### Windows

**Processes:** each process has a user install path and a system install path. Add the path that matches your install type.

| Process            | Install type | Path                                                                                             |
| ------------------ | ------------ | ------------------------------------------------------------------------------------------------ |
| `Cursor.exe`       | User         | `%LOCALAPPDATA%\Programs\cursor\Cursor.exe`                                                      |
| `Cursor.exe`       | System       | `%ProgramFiles%\cursor\Cursor.exe`                                                               |
| `rg.exe`           | User         | `%LOCALAPPDATA%\Programs\cursor\resources\app\node_modules\@vscode\ripgrep\bin\rg.exe`           |
| `rg.exe`           | System       | `%ProgramFiles%\cursor\resources\app\node_modules\@vscode\ripgrep\bin\rg.exe`                    |
| `inno_updater.exe` | User         | `%LOCALAPPDATA%\Programs\cursor\resources\app\node_modules\cursor-inno-updater\inno_updater.exe` |
| `inno_updater.exe` | System       | `%ProgramFiles%\cursor\resources\app\node_modules\cursor-inno-updater\inno_updater.exe`          |

**Paths:**

| Path                              | Description                                               |
| --------------------------------- | --------------------------------------------------------- |
| `%LOCALAPPDATA%\Programs\cursor\` | Application binaries and bundled modules (user install)   |
| `%ProgramFiles%\cursor\`          | Application binaries and bundled modules (system install) |
| `%APPDATA%\Cursor\`               | User data, settings, and workspace storage                |

### macOS

**Processes:** `Cursor.app`

**Paths:**

| Path                        | Description        |
| --------------------------- | ------------------ |
| `/Applications/Cursor.app/` | Application bundle |

## Why exclusions may be needed

Cursor's extension host reads JavaScript files from its own install directory at startup. When security software adds per-file scanning latency, the cumulative delay can exceed Cursor's startup timeout.

This primarily affects startup. Once modules are loaded into memory, ongoing file operations are infrequent and unlikely to cause issues.

Cursor's own files are code-signed binaries and bundled JavaScript, not user-generated content. Excluding them from real-time scanning is low-risk and does not reduce protection for user files or network traffic.

Both **process exclusions** and **path exclusions** may be needed. Some products use kernel-level minifilter drivers that scan all file I/O regardless of which process is reading. A process-only exclusion may not be sufficient — add path exclusions for the Cursor install directory as well.

## Identifying active security software

These commands can help identify which products are running so you know where you may need to configure exclusions. On Windows, run in an **Administrator PowerShell** window:

```powershell
# Registered AV products
Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct |
  Select-Object displayName, pathToSignedProductExe

# Kernel-level filesystem filter drivers
fltmc

# Check for EDR process injection via environment variables
[System.Environment]::GetEnvironmentVariables() |
  Where-Object { $_.Keys -match "BPP|COR_PROFILER|COMPLUS|__COMPAT" }

# Windows Defender status
Get-MpComputerStatus |
  Select-Object IsTamperProtected, RealTimeProtectionEnabled, AMRunningMode
```

**How to read `fltmc` output:** Standard Windows drivers you can ignore include `WdFilter`, `storqosflt`, `wcifs`, `CldFlt`, `bfs`, `FileCrypt`, `luafv`, `Wof`, `FileInfo`, `npsvctrig`, `bindflt`, and `UnionFS`. Other drivers are likely from third-party security software.

**How to read the environment variable output:** If it returns any results, an EDR product is injecting code into every new process on the machine, and an exclusion may be necessary.

## Verifying exclusions are working

After applying exclusions, restart Cursor and verify that Agent features work without timing out. If you previously saw empty Extension Host logs (Cmd/Ctrl+Shift+P → "Output" → "Extension Host"), they should now show normal startup output.

## Troubleshooting checklist

1. Run the [identification commands above](https://cursor.com/docs/enterprise/endpoint-security.md#identifying-active-security-software) to determine which security products are running
2. Add both process and path exclusions for the identified products in their management consoles
3. Restart Cursor and test Agent — this is the definitive test of whether exclusions are working
4. If exclusions don't resolve the issue, [export logs](https://cursor.com/help/troubleshooting/agent-issues.md#what-if-i-see-agent-execution-timed-out) and contact Cursor support with the diagnostic output\`


---

## Sitemap

[Overview of all docs pages](/llms.txt)
