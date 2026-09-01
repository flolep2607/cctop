# Language Server Fails with 'No Space Left on Device' on Linux

Resolve Linux language server startup failures caused by exhausted inotify watch/instance limits (ENOSPC). Includes symptoms, diagnosis commands, and sysctl fixes.

On Linux, the Windsurf language server may fail to start with an error containing **"no space left on device"**, even when the system has plenty of free disk space. This is caused by the Linux kernel's **inotify watch** or **inotify instance** limits being exhausted, not by actual disk usage.

The language server uses **inotify** to watch files in your workspace for changes. When the kernel limit is reached, the system returns an `ENOSPC` error—which is commonly surfaced as "no space left on device."

***

## **Symptoms**

You may see the following in Windsurf output logs:

```
Language server failed - no space left on device: no space left on device
```

Often accompanied by stack traces referencing components like:

* `file_watcher`
* `AddTrackedWorkspace`
* `AddDirectoriesRecursive`

Behavior you'll typically observe:

* Windsurf opens normally
* The language server exits immediately after starting
* Language-server-dependent features (e.g., Cascade, autocomplete) do not work

***

## **Diagnosis**

### **1. Check your current inotify limits**

Run the following commands:

```shell theme={null}
# Check the maximum number of inotify watches per user
cat /proc/sys/fs/inotify/max_user_watches

# Check the maximum number of inotify instances per user
cat /proc/sys/fs/inotify/max_user_instances
```

Common defaults are **8192** for watches and **128** for instances. These are frequently too low for IDE usage in large workspaces (especially monorepos) and can be further reduced by other processes that consume inotify resources (containers, sync tools, other editors, background services).

### **2. Check how many inotify instances are currently in use**

```shell theme={null}
find /proc/*/fd -lname anon_inode:inotify 2>/dev/null | wc -l
```

If this count is near (or above) your `max_user_instances`, new inotify users (like the language server) may fail to initialize.

***

## **Solution**

Increase the inotify limits. You can apply changes temporarily (until reboot) or permanently.

### **Temporary fix (until reboot)**

```shell theme={null}
sudo sysctl fs.inotify.max_user_watches=524288
sudo sysctl fs.inotify.max_user_instances=1024
```

### **Permanent fix (survives reboot)**

```shell theme={null}
echo "fs.inotify.max_user_watches=524288" | sudo tee -a /etc/sysctl.conf
echo "fs.inotify.max_user_instances=1024" | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

After applying either fix, **restart Windsurf**. The language server should start successfully.

This is a well-known limitation on Linux that affects other IDEs and developer tools that rely on file watchers. If your organization manages system configurations centrally, ask your IT / infra team to apply these sysctl settings.

***

## **When to use which value**

* **`fs.inotify.max_user_watches=524288`**\
  Recommended for large repositories or monorepos. Each watched file/directory consumes kernel memory (often \~1 KB per watch on 64-bit systems), so **524288 watches** can use roughly **\~512 MB** of kernel memory.
* **`fs.inotify.max_user_instances=1024`**\
  Recommended if you run multiple applications that create inotify instances (multiple IDE windows, containers, file sync tools, etc.). The default of **128** can be exhausted quickly in development environments.
