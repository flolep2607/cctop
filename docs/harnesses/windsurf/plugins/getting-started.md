# Welcome to Windsurf Plugins

Install and set up Windsurf Plugins for JetBrains, VS Code, Visual Studio, Vim, NeoVim, Jupyter, Chrome, and other IDEs with AI-powered coding assistance.

**Windsurf Plugins** bring AI-powered coding assistance to your preferred IDE or editor.

<Card title="Teams and Enterprise" icon="users" href="/plugins/accounts/teams-getting-started">
  Get started with your team!
</Card>

<CardGroup>
  <Card
    title="Cascade"
    icon={
  <svg
    width="25"
    height="25"
    viewBox="0 0 1292 1292"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
  >
    <path
      d="M1195 599C1195 848.08 993.08 1050 744 1050C494.92 1050 293 848.08 293 599C293 349.92 494.92 148 744 148C993.08 148 1195 349.92 1195 599ZM411.5 599C411.5 782.635 560.365 931.5 744 931.5C927.635 931.5 1076.5 782.635 1076.5 599C1076.5 415.365 927.635 266.5 744 266.5C560.365 266.5 411.5 415.365 411.5 599Z"
      fill="#34E8BB"
    />
    <path
      d="M1096.19 1053.62C1116.8 1078.03 1113.86 1114.77 1087.65 1133.04C1002.41 1192.46 903.441 1229.92 799.584 1241.61C676.505 1255.46 552.082 1232.51 442.049 1175.65C332.016 1118.79 241.314 1030.58 181.415 922.172C130.87 830.693 104.172 728.301 103.33 624.396C103.071 592.449 131.338 568.79 163.173 571.479C195.007 574.168 218.29 602.208 219.218 634.143C221.573 715.175 243.206 794.78 282.679 866.22C331.512 954.6 405.457 1026.51 495.161 1072.87C584.866 1119.22 686.302 1137.94 786.643 1126.64C867.75 1117.51 945.198 1089.11 1012.66 1044.15C1039.24 1026.44 1075.58 1029.21 1096.19 1053.62Z"
      fill="#34E8BB"
    />
    <path
      d="M177.334 450.08C146.261 442.514 126.947 411.072 137.349 380.829C160.687 312.983 195.56 249.512 240.566 193.267C285.571 137.023 339.851 89.0802 400.928 51.4326C428.153 34.6511 463.065 46.5999 477.261 75.2582C491.457 103.917 479.508 138.389 452.641 155.738C406.542 185.506 365.436 222.584 330.994 265.627C296.552 308.67 269.39 356.906 250.456 408.411C239.421 438.428 208.408 457.646 177.334 450.08Z"
      fill="#34E8BB"
    />
  </svg>
}
    href="/plugins/cascade/cascade-overview"
  >
    Windsurf's coding agent.
  </Card>

  <Card title="Usage" icon="bars-progress" href="/plugins/accounts/usage">
    Credits and usage.
  </Card>

  <Card title="Models" icon="robot" href="/plugins/cascade/models">
    Models available for use.
  </Card>
</CardGroup>

## JetBrains (Local)

<Note>
  These steps do not apply for enterprises on a self-hosted plan.
  If you are an enterprise user, please refer to the instructions in your enterprise portal.
</Note>

<Note>
  For remote development environments, use the "Windsurf (Remote Development)" plugin instead. See the [Remote Development section](#remote-development) below.
</Note>

<Steps>
  <Step title="Install Local Plugin">
    Open the `Plugins` menu in your JetBrains IDE. The shortcut for this is `⌘+,` on Mac and `Ctrl+,` on Linux/Windows. It is also accessible from the settings menu.
    Search for the Windsurf plugin, and install it. The plugin loader will prompt you to restart the IDE.

    <Frame>
      <img />
    </Frame>
  </Step>

  <Step title="Wait for Language Server">
    Upon successful installation, Windsurf will begin downloading a language server.
    This is the program that communicates with our APIs to let you use Windsurf's AI features.
    The download usually takes ten to twenty seconds, but the download speed may depend on your internet connection.
    In the meantime, you are free to use your IDE as usual.

    You should see a notification on the bottom right to indicate the progress of the download.

    <Frame>
      <img />
    </Frame>
  </Step>

  <Step title="Authorize">
    Open a project. Windsurf will prompt you to sign in with a notification popup at the bottom right linking you to a login page.
    Equivalently, click the widget at the right of the bottom status bar and select the login option there.

    <Frame>
      <img />
    </Frame>

    If you're not already signed in, you'll be prompted to sign in or create an account.

    <Frame>
      <img />
    </Frame>

    Once you've signed in, the webpage will indicate that you can return to your IDE.

    <Frame>
      <img />
    </Frame>
  </Step>

  <Step title="All Done!">
    You're all set. Windsurf's AI features — Autocomplete, Chat, Command, and more — are now available.

    At any point, you can check your status by clicking the status bar widget at the bottom right.
    If signed in, you will have access to your Windsurf settings and other controls.

    If you'd like early access to new features, click on "Switch to Pre-Release"
    to try out the [latest pre-release version](https://plugins.jetbrains.com/plugin/20540-windsurf-plugin-for-python-js-java-go--/versions/pre-release)
    of the plugin.

    <Frame>
      <img />
    </Frame>
  </Step>
</Steps>

### Remote Development

For JetBrains IDEs used in remote development environments, you need to use the separate "Windsurf (Remote Development)" plugin.

For advanced agentic AI capabilities and cutting-edge features, we strongly recommend using the native Windsurf Editor or the JetBrains local plugin. This plugin continues to receive the latest models, compatibility updates, and bug fixes, but does not include newer features exclusive to the Windsurf Editor.

#### Requirements

* JetBrains IDE version 2025.1.3 or greater

#### Installation Steps

<Steps>
  <Step title="Install on Host">
    Open the `Plugins (Host)` menu in your JetBrains IDE. The shortcut for this is `⌘+,` on Mac and `Ctrl+,` on Linux/Windows. It is also accessible from the settings menu.
    Search for **"Windsurf (Remote Development)"** and install it.
    Restart your IDE when prompted.

    <Frame>
      <img />
    </Frame>
  </Step>

  <Step title="Install on Client">
    Open the `Plugins (Client)` menu and search for **"Windsurf (Remote Development)"**.
    Install the plugin and restart the IDE again.

    <Frame>
      <img />
    </Frame>
  </Step>

  <Step title="Wait for Language Server">
    After installing the plugin on the host, Windsurf will begin downloading a language server.
    This is the program that communicates with our APIs to let you use Windsurf's AI features.
    The download usually takes ten to twenty seconds, but the download speed may depend on your internet connection.
    In the meantime, you are free to use your IDE as usual.

    You should see a notification on the bottom right to indicate the progress of the download.

    <Frame>
      <img />
    </Frame>
  </Step>

  <Step title="Authorize">
    After the language server download is completed, Windsurf will prompt you to sign in with a notification popup at the bottom right linking you to a login page.
    Equivalently, click the widget at the right of the bottom status bar and select the login option there.

    <Frame>
      <img />
    </Frame>

    If you're not already signed in, you'll be prompted to sign in or create an account.

    <Frame>
      <img />
    </Frame>

    Once you've signed in, the webpage will indicate that you can return to your IDE.

    <Frame>
      <img />
    </Frame>
  </Step>

  <Step title="All Done!">
    You're all set. Windsurf's AI features are now available in your remote environment.
  </Step>
</Steps>

## Older Plugins

We strongly recommend using the native Windsurf Editor or the JetBrains local plugin for their advanced agentic AI capabilities and cutting-edge features.
The plugins below are in maintenance mode.

<Tabs>
  <Tab title="Visual Studio Code">
    <Steps>
      <Step title="Install Plugin">
        Find the Windsurf Plugin (formerly Codeium) in the VS Code Marketplace and install it.

        <Frame>
          <img />
        </Frame>
      </Step>

      <Step title="Authorize">
        After installation, VS Code will prompt you with a notification in the bottom right corner to sign in to Windsurf.
        Equivalently, you can sign in to Windsurf via the profile icon at the bottom of the left sidebar.

        <Frame>
          <img />
        </Frame>

        <Note>If you get an error message indicating that the browser cannot open a link from Visual Studio Code, you may need to update your browser and restart the authorization flow.</Note>
        If you're not already signed in, you'll be prompted to sign in or create an account.

        <Frame>
          <img />
        </Frame>

        Once you sign in, you'll be redirected back to Visual Studio Code.
        <Note>If you are using a browser-based VS Code IDE like GitPod or Codespaces, you will be routed to instructions on how to complete authentication by providing an access token.</Note>
      </Step>

      <Step title="Wait for Language Server">
        Once you're signed in, Windsurf will start downloading a language server.
        This is the program that communicates with our APIs to let you use Windsurf's AI features.
        The download usually takes ten to twenty seconds, but the download speed may depend on your internet connection.
        In the meantime, you are free to use VS Code as usual.
      </Step>

      <Step title="All Done!">
        You're all set. Windsurf's AI features — Autocomplete, Chat, and Command — are now available.
      </Step>
    </Steps>
  </Tab>

  <Tab title="Vim / Neovim">
    ### Extension Installation

    <Steps>
      <Step title="Install Plugin">
        Follow the **Get Started** instructions in the public [`codeium.vim` repo](https://github.com/Exafunction/codeium.vim). That’s it!
      </Step>
    </Steps>

    ### Using Windsurf Plugin

    <Steps>
      <Step title="Setup">
        While Windsurf supports many languages, we’ll demonstrate with Python. Create a new file `test.py`.
      </Step>

      <Step title="From Code">
        Windsurf can suggest multiple lines of code from a partial function header:

        <Frame>
          <img alt="Snippet one" />
        </Frame>
      </Step>

      <Step title="Accept Suggestion">
        Press **Tab** to accept.
      </Step>

      <Step title="From Comments">
        Windsurf also understands comments:

        <Frame>
          <img alt="Snippet two" />
        </Frame>
      </Step>
    </Steps>
  </Tab>

  <Tab title="Visual Studio">
    ### Extension Installation

    <Steps>
      <Step title="Open Extension Marketplace">
        In the Visual Studio menu bar, click **Extensions → Manage Extensions**.

        <Frame>
          <img alt="Manage Extensions" />
        </Frame>
      </Step>

      <Step title="Install Windsurf Plugin">
        In **Manage Extensions**, click **Visual Studio Marketplace**, search for **Windsurf**, then click **Download**.

        <Frame>
          <img alt="Install plugin" />
        </Frame>
      </Step>

      <Step title="Relaunch Visual Studio">
        Close the window and relaunch Visual Studio.
      </Step>

      <Step title="Sign in to Windsurf Plugin">
        Open or create a project. A browser window will open and prompt you to sign in.
      </Step>

      <Step title="Create Account">
        If you don’t have an account yet, you’ll be redirected to create one.
      </Step>

      <Step title="All Done!">
        You're all set. Windsurf's AI features are now available in Visual Studio.
      </Step>
    </Steps>

    ### Using Windsurf Plugin

    <Steps>
      <Step title="Setup">
        While Windsurf supports many languages, we’ll demonstrate with C#. Create or open a C# file.
      </Step>

      <Step title="From Code">
        Windsurf can suggest multiple lines of code from a partial function signature:

        <Frame>
          <img alt="Suggestion example" />
        </Frame>
      </Step>

      <Step title="Accept Suggestion">
        Press **Tab** to accept.

        <Frame>
          <img alt="After accept" />
        </Frame>
      </Step>
    </Steps>
  </Tab>

  <Tab title="Jupyter Notebook">
    ### Install Windsurf Plugin

    <Steps>
      <Step title="Install Jupyter Extension">
        Open a new Jupyter Lab session. In a cell, paste and run `Shift+Enter` the following:

        ```python theme={null}
        import sys
        !{sys.executable} -m pip install -U pip --user
        !{sys.executable} -m pip install -U codeium-jupyter --user
        ```

        If you’re inside a virtual environment, run:

        ```python theme={null}
        import sys
        !{sys.executable} -m pip install -U pip
        !{sys.executable} -m pip install -U codeium-jupyter
        ```

        When the commands finish, close the notebook and stop the Jupyter server.
      </Step>

      <Step title="Launch Jupyter">
        Relaunch Jupyter and open a notebook. Open the settings (<kbd>Ctrl</kbd> + <kbd>,</kbd>) and navigate to the **Windsurf** section. You’ll see fields for an enterprise URL and a token.

        <Frame>
          <img alt="Settings UI" />
        </Frame>

        Click **Get Windsurf Authentication Token** and follow the link. Paste the token back into the settings dialog.

        <Frame>
          <img alt="Settings menu" />
        </Frame>

        <Note>If you can’t find the Windsurf settings, you likely didn’t restart Jupyter. Stop the server (Ctrl+C) and start it again with <code>jupyter lab</code>.</Note>
      </Step>

      <Step title="Create Account">
        If you don’t have a Windsurf account, you’ll be prompted to create one.
      </Step>

      <Step title="Authenticate">
        After signing in, copy the token and paste it into the settings dialog.

        <Frame>
          <img alt="Auth token field" />
        </Frame>
      </Step>

      <Step title="All Done!">
        You're all set. Windsurf's AI features are now available in Jupyter.
      </Step>
    </Steps>

    ### Using Windsurf Plugin

    <Steps>
      <Step title="From Code">
        Windsurf can suggest multiple lines of code from a partial function header:

        <Frame>
          <img alt="Snippet one" />
        </Frame>
      </Step>

      <Step title="Accept Suggestion">Press **Tab** to accept.</Step>

      <Step title="From Comments">
        Windsurf also understands comments:

        <Frame>
          <img alt="Snippet two" />
        </Frame>
      </Step>
    </Steps>
  </Tab>

  <Tab title="Chrome">
    ### Install Windsurf

    <Steps>
      <Step title="Install Chrome Extension">
        Visit the [Chrome Web Store page](https://chrome.google.com/webstore/detail/codeium/hobjkcpmjhlegmobgonaagepfckjkceh) and click **Add to Chrome**.

        <Frame>
          <img alt="Chrome Web Store" />
        </Frame>
      </Step>

      <Step title="Pin Extension">
        Open the extensions dropdown and click the **Pin** icon so the Windsurf icon stays visible.

        <Frame>
          <img alt="Pin extension" />
        </Frame>
      </Step>

      <Step title="Sign In">
        The extension opens a login page automatically. If not, click the extension icon and follow the link.

        <Frame>
          <img alt="Sign in" />
        </Frame>
      </Step>

      <Step title="All Done!">
        You're all set. Try [creating a new Colab notebook](https://colab.research.google.com/#create=true).

        <Frame>
          <img alt="Signed in" />
        </Frame>
      </Step>
    </Steps>

    ### Using Windsurf

    <Steps>
      <Step title="From Code">
        Windsurf can suggest multiple lines of code from a partial function header:

        <Frame>
          <img alt="Snippet one" />
        </Frame>
      </Step>

      <Step title="Accept Suggestion">Press **Tab** to accept.</Step>

      <Step title="From Comments">
        Windsurf also understands comments:

        <Frame>
          <img alt="Snippet two" />
        </Frame>
      </Step>
    </Steps>
  </Tab>

  <Tab title="Eclipse">
    ### Extension Installation

    <Steps>
      <Step title="Drag the Install button">
        Visit the [Windsurf Plugin page on Eclipse Marketplace](https://marketplace.eclipse.org/content/codeium) and drag the **Install** button to the Eclipse toolbar.

        <Frame>
          <img alt="Drag Install button" />
        </Frame>
      </Step>

      <Step title="Confirm Selected Features">
        In the **Confirm Selected Features** prompt, click **Confirm**.

        <Frame>
          <img alt="Confirm features" />
        </Frame>
      </Step>

      <Step title="Trust Unsigned Content">
        In the **Trust Artifacts** prompt, select **Unsigned** and click **Trust**.

        <Frame>
          <img alt="Trust unsigned" />
        </Frame>
      </Step>

      <Step title="Restart Eclipse">
        When prompted, restart Eclipse to complete the installation.

        <Frame>
          <img alt="Restart Eclipse" />
        </Frame>
      </Step>

      <Step title="Create / Sign In">
        When the browser opens, sign in or create an account, then return to Eclipse.
      </Step>

      <Step title="All Done!">You're all set. Windsurf's AI features are now available in Eclipse.</Step>
    </Steps>

    ### Using Windsurf

    <Steps>
      <Step title="Setup">
        While Windsurf supports many languages, we’ll demonstrate with Java. Create a new file `Fib.java`.
      </Step>

      <Step title="From Code">
        Windsurf can suggest multiple lines of code from a partial function header:

        ```java theme={null}
        package test;

        public class Fib {

            public int fib(int n) {
            }

        }
        ```
      </Step>

      <Step title="Accept Suggestion">Press **Tab** to accept.</Step>
    </Steps>
  </Tab>
</Tabs>
