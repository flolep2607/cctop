# Gathering Plugin Logs

How to collect diagnostic logs from JetBrains, VS Code, Eclipse, Visual Studio, and NeoVim for troubleshooting Windsurf plugin issues.

If you're having issues, the first step in the troubleshooting process is to retrieve the logs from your IDE. Here's how you can get Windsurf logs for each of the major IDEs:

## JetBrains IDEs

<Tabs>
  <Tab title="Local">
    Cascade has now the option to generate a diagnostics file directly from the IDE, there are 2 ways to do so:

    * In the Cascade window, click on the 3 dots in the upper right side, and select Download Diagnostics
    * In the IDE menu, go to Tools > Windsurf > Download Windsurf Diagnostics

    The first option is preferred since it also includes Cascade embedded browser logs.
    This button will automatically collect relevant logs and parameters into a text file.

    In extreme situations, you can always get the IDE full log (idea.log) from Help > Show Log in Explorer/Finder.
  </Tab>

  <Tab title="Remote">
    To gather the Windsurf diagnostics, you can use the following options:

    * In the Cascade window, click on the 3 dots in the upper right side, and select Download Diagnostics
    * In the IDE menu, go to Tools > Windsurf > Download Windsurf Diagnostics

    The first option is preferred since it also includes Cascade embedded browser logs.

    In addition, to collect the full IDE logs:

    * In the IDE menu, go to Tools > Windsurf > Collect Host and Client Logs
  </Tab>
</Tabs>

## VS Code

1. Go to the Command Palette (`Ctrl/Cmd + Shift + P` or go to View > Command Palette)

2. Type in "Show logs" and select the option that reads "Developer: Show Logs"

3. Change the dropdown in the top right that reads "Extension Host" and select "Windsurf"

4. You should see something similar to the image below:

<Frame>
  <img />
</Frame>

5. Export or copy the logs

## Eclipse

In Eclipse, logs are written to the following paths:

* **Mac/Linux**: \~/.codeium/codeium.log
* **Windows**: C:\Users\<username>.codeium\codeium.log

## Visual Studio

Go to **view > output**, select "Windsurf" in the dropdown, and copy the logs.

## NeoVim

Set `g:codeium_log_file` to a path to a file in their vimrc and then relaunch vim.

Then the logs should be written to that file.
