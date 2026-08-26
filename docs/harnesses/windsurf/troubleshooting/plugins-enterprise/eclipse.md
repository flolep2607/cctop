# Eclipse Troubleshooting

Troubleshoot Eclipse plugin issues including startup problems, empty chat screen, WebView2, and certificate errors with Java keystore solutions.

<Note>
  We strongly recommend using the native Windsurf Editor or the JetBrains local plugin for their advanced agentic AI capabilities and cutting-edge features.
  The Eclipse plugin is under maintenance mode.
</Note>

# Supported Versions

Version 4.25+ (2022-09+)

# Gathering extension logs

In Eclipse, logs are written to the following paths:

* **Mac/Linux**: \~/.codeium/codeium.log
* **Windows**: C:\Users\<username>.codeium\codeium.log

# Known IDE issues and solutions

## Codeium isn't starting

If Codeium isn't starting up, use the logs to debug what the cause could be (See above). If you are not able to resolve the issue, file a help request by submitting a ticket at help.codeium.com. Make sure to include the logs referenced above to help our team debug the issue as quickly as possible.

## Codeium Chat shows an empty screen

If you are using Windows 10, it's possible you need to install **WebView2** to switch the Eclipse web renderer from Internet Explorer to Edge.

You can see if this is the case by right-clicking --> `Properties` and seeing if there is an Internet Explorer icon.

## Certificate issue

This issue may be indicated by the following errors in the logs:

```
Failed to fetch extension version at <YourDomainURL>
javax.net.ssl.SSLHandshakeException: PKIX path building failed: sun.security.provider.certpath.SunCertPathBuilderException: unable to find valid certification path to requested target
```

Unlike other IDEs, Eclipse does not use the OS certificate store. You will have to load the certificates to the Java keystore.

* SaaS users will have to load the Codeium Github URL
* Self-hosted (On-prem) users will have to load their Codeium Enterprise domain URL as well as the Codeium Github URL

**Note**: This is an example for SaaS users, but the process is the same. *For enterprise users - Your certificate is issued and managed by your local IT or Admin team. Please reach out to them for assistance with installing the necessary certificates on your system.*

1. Export the certificate for [https://exafunction.github.io/](https://exafunction.github.io/) from the browser as `githubio.cer` file

In Chrome: navigate to the website, click the padlock, click `Connection is secure`, click `Certificate is valid`, go to the `Details` tab, press the `Copy to File...` button

2. Import in JDK/JRE keystore: (Need to run from cmd prompt opened with "Administrator" privilege)

```
keytool -import -noprompt -trustcacerts -alias codeiumgithub -file githubio.cer -keystore "%JAVA_HOME%/jre/lib/security/ cacerts" -storepass changeit
```

3. Verify that the certificate is added to the Keystore by executing:

```
keytool -list -keystore "%JAVA_HOME%/jre/lib/security/ cacerts" | findstr codeium
```

Enter the Keystore password.

4. Restart Eclipse and browse the marketplace extension from an internal browser. You should be directed to trust the unsigned content.

5. In some cases you might also need to pass the certificates path in VM arguments by editing your eclipse.ini file and adding the path:

```
-Djavax.net.ssl.trustStore="path-to-your-certificates"
```
