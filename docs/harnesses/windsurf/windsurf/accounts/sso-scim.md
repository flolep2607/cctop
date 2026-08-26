# Setting up SSO & SCIM

Configure Single Sign-On (SSO) and SCIM provisioning for your organization using Google Workspace, Microsoft Azure AD, Okta, or other SAML identity providers.

<Note>This feature is only available to Enterprise users.</Note>

<Note>This feature is not applicable to Cognition Platform plans. For Cognition Platform, SSO should be configured and managed in your Cognition Platform settings instead.</Note>

<Tabs>
  <Tab title="Google SSO">
    Windsurf now supports sign in with Single Sign-On (SSO) via SAML. If your organization uses Microsoft Entra, Okta, Google Workspaces, or some other identity provider that supports SAML, you will be able to use SSO with Windsurf.

    <Note>Windsurf only supports SP-initiated SSO; IDP-initiated SSO is NOT currently supported.</Note>

    ### Configure IDP Application

    On the google admin console (admin.google.com) click **Apps -> Web and mobile apps** on the left.

    <Frame>
      <img />
    </Frame>

    Click on **Add app**, and then **Add custom SAML app**.

    <Frame>
      <img />
    </Frame>

    Fill out **App name** with `Windsurf`, and click **Next**.

    The next screen (Google Identity Provider details) on Google’s console page has data you’ll need to copy to Windsurf’s SSO settings on [https://windsurf.com/team/settings](https://windsurf.com/team/settings).

    * Copy **SSO URL** from Google’s console page to Windsurf’s settings under **SSO URL**
    * Copy **Entity ID** from Google’s console page to Windsurf’s settings under **Idp Entity ID**
    * Copy **Certificate** from Google’s console page to Windsurf’s settings under **X509 Certificate**
    * Click **Continue** on Google’s console page

    The next screen on Google’s console page requires you to copy data from Codeium’s settings page

    * Copy **Callback URL** from Codeium’s settings page to Google’s console page under **ACS URL**
    * Copy **SP Entity ID** from Codeium’s settings page to Google’s console page under **SP Entity ID**
    * Change **Name ID** format to **EMAIL**
    * Click **Continue** on Google’s console page

    The next screen on Google’s console page requires some configuration

    * Click on **Add Mapping**, select **First name** and set the **App attributes** to **firstName**
    * Click on **Add Mapping**, select **Last name** and set the **App attributes** to **lastName**
    * Click **Finish**

    <Frame>
      <img />
    </Frame>

    On Codeium’s settings page, click **Enable Login with SAML**, and then click **Save**. Make sure to click on **Test Login** to make sure login works as expected. All users now will have SSO login enforced.
  </Tab>

  <Tab title="Microsoft Entra ID">
    Windsurf Enterprise now supports sign in with Single Sign-On (SSO) via SAML. If your organization uses Microsoft Entra ID (formerly Azure AD), you will be able to use SSO with Windsurf.

    <Note>Windsurf only supports SP-initiated SSO; IDP-initiated SSO is NOT currently supported.</Note>

    ## Part 1: Create Enterprise Application in Microsoft Entra ID

    <Note>All steps in this section are performed in the **Microsoft Entra ID admin center**.</Note>

    1. In Microsoft Entra ID, click on **Add**, and then **Enterprise Application**.

    <Frame>
      <img />
    </Frame>

    2. Click on **Create your own application**.

    <Frame>
      <img />
    </Frame>

    3. Name your application **Windsurf**, select *Integrate any other application you don't find in the gallery*, and then click **Create**.

    <Frame>
      <img />
    </Frame>

    ## Part 2: Configure SAML and User Attributes in Microsoft Entra ID

    <Note>All steps in this section are performed in the **Microsoft Entra ID admin center**.</Note>

    4. In your new Windsurf application, click on **Set up single sign on**, then click **SAML**.

    5. Click on **Edit** under **Basic SAML Configuration**.

    6. **Keep this Entra ID tab open** and open a new tab to navigate to the **Windsurf Teams SSO settings** at [https://windsurf.com/team/settings](https://windsurf.com/team/settings).

    7. In the **Microsoft Entra ID** SAML configuration form:
       * **Identifier (Entity ID)**: Copy the **SP Entity ID** value from the **Windsurf SSO settings page**
       * **Reply URL (Assertion Consumer Service URL)**: Copy the **Callback URL** value from the **Windsurf SSO settings page**
       * Click **Save** at the top

    8. Configure user attributes for proper name display. In **Microsoft Entra ID**, under **Attributes & Claims**, click **Edit**.

    9. Create 2 new claims by clicking **Add new claim** for each:
       * **First claim**: Name = `firstName`, Source attribute = `user.givenname`
       * **Second claim**: Name = `lastName`, Source attribute = `user.surname`

    ## Part 3: Configure SSO Settings in Windsurf Portal

    <Note>Complete the configuration in the **Windsurf portal** ([https://windsurf.com/team/settings](https://windsurf.com/team/settings)).</Note>

    10. In the **Windsurf SSO settings page**:
        * **Pick your SSO ID**: Choose a unique identifier for your team's login portal (this cannot be changed later)
        * **IdP Entity ID**: Copy the value from **Microsoft Entra ID** under **Set up Windsurf** → **Microsoft Entra Identifier**
          <Note>The IdP Entity ID URL must end with a trailing `/` (e.g., `https://sts.windows.net/{tenant-id}/`). If the URL does not include the trailing slash, add it manually.</Note>
        * **SSO URL**: Copy the **Login URL** value from **Microsoft Entra ID**
        * **X509 Certificate**: Download the **SAML certificate (Base64)** from **Microsoft Entra ID**, open the file, and paste the text content here

    11. In the **Windsurf portal**, click **Enable Login with SAML**, then click **Save**.

    12. **Test the configuration**: Click **Test Login** to verify the SSO configuration works as expected.

    <Note>**Important**: Do not log out or close the Windsurf settings page until you've successfully tested the login. If the test fails, you may need to troubleshoot your configuration before proceeding.</Note>
  </Tab>

  <Tab title="Okta SSO">
    Windsurf Enterprise now supports sign in with Single Sign-On (SSO) via SAML. If your organization uses Microsoft Entra, Okta, Google Workspaces, or some other identity provider that supports SAML, you will be able to use SSO with Windsurf.

    <Note>Windsurf only supports SP-initiated SSO; IDP-initiated SSO is NOT currently supported.</Note>

    ### Configure IDP Application

    Click on Applications on the left sidebar, and then Create App Integration

    <Frame>
      <img />
    </Frame>

    Select SAML 2.0 as the sign-in method

    <Frame>
      <img />
    </Frame>

    Set the app name as Windsurf (or to any other name), and click Next

    Configure the SAML settings as

    * Single sign-on URL to [https://auth.windsurf.com/\_\_/auth/handler](https://auth.windsurf.com/__/auth/handler)
    * Audience URI (SP Entity ID) to [www.codeium.com](http://www.codeium.com)
    * NameID format to EmailAddress
    * Application username to Email

    Configure the attribute statements as following, and then click **Next**.

    <Frame>
      <img />
    </Frame>

    In the feedback section, select “This is an internal app that we have created”, and click **Finish**.

    ### Register Okta as a SAML provider

    You should be redirected to the Sign on tab under your custom SAML application. Now you’ll want to take the info in this page and fill it out in Windsurf’s SSO settings.

    * Open [https://windsurf.com/team/settings](https://windsurf.com/team/settings), and click on Configure SAML
    * Copy the text after ‘Issuer’ in Okta’s application page and paste it under Idp Entity ID
    * Copy the text after ‘Sign on URL’ in Okta’s application page and paste it under SSO URL
    * Download the Signing Certificate and paste it under X509 certificate
    * Check Enable Login with SAML and then click Save
    * Test the login with the Test Login button. You should see a success message:

    <Frame>
      <img />
    </Frame>

    At this point everything should have been configured, and can now add users to the new Windsurf Okta application.

    You should share your organization's custom Login Portal URL with your users and ask them to sign in via that link.

    <Frame>
      <img />
    </Frame>

    Users who login to Windsurf via SSO will be auto-approved into the team.

    ### Caveats

    Note that Windsurf does not currently support IDP-initiated login flows.

    We also do not yet support OIDC.

    # Troubleshooting

    ### Login with SAML config failed: Firebase: Error (auth/operation-not-allowed)

    <Frame>
      <img />
    </Frame>

    This points to your an invalid SSO ID, or your SSO URL being incorrect, make sure it is alphanumeric and has no extra spaces or invalid characters. Please go over the steps in the guide again and make sure you use the correct values.

    ### Login with SAML config failed: Firebase: SAML Response \<Issuer> mismatch. (auth/invalid-credential)

    <Frame>
      <img />
    </Frame>

    This points to your IdP entity ID being invalid, please make sure you copy it correctly from the Okta portal, without any extra characters or spaces before or after the string.

    ### Failed to verify the signature in samlresponse

    This points to an incorrect value of your X509 certificate, please make sure you copy the correct key, and that it is formatted as:

    ```
    -----BEGIN CERTIFICATE-----
    value
    ------END CERTIFICATE------
    ```
  </Tab>

  <Tab title="Azure SCIM">
    Windsurf supports SCIM synchronization for users and groups with Microsoft Entra ID / Azure AD. It isn't necessary to setup SSO to use SCIM synchronization, but it is highly recommended.

    You'll need:

    * Admin access to Microsoft Entra ID / Azure AD
    * Admin access to Windsurf
    * An existing Windsurf Application on Entra ID (normally from your existing SSO application)

    <Note>
      **Service Key Permissions Required**

      The service key used for SCIM provisioning must have the following permissions:

      * **Team User Read** - Required to read user and group information
      * **Team User Update** - Required to create and update users and groups
      * **Team User Delete** - Required to deactivate/delete users and groups

      You can create a custom role with these permissions or use an existing admin role that includes them.
    </Note>

    ## Step 1: Create a Role with SCIM Permissions

    Before setting up SCIM provisioning, you need to create a role with the required permissions.

    1. Go to [Windsurf Team Settings](https://windsurf.com/team/settings)
    2. Under "Other Settings", click **Configure** next to **Role Management**
    3. Click **Add Role** and name it "SCIM Provisioning"
    4. Add the following permissions:
       * Team User Read
       * Team User Update
       * Team User Delete
    5. Click **Save**

    ## Step 2: Navigate to the existing Windsurf Application

    Go to Microsoft Entra ID on Azure, click on Enterprise applications on the left sidebar, and then click on the existing Windsurf application in the list.

    <Frame>
      <img />
    </Frame>

    ## Step 3: Setup SCIM provisioning

    Click on Get started under Provision User Accounts in the middle (step 3), and then click on Get started again.

    <Frame>
      <img />
    </Frame>

    Under the Provisioning setup page, select the following options.

    Provisioning Mode:  Automatic

    Admin Credentials > Tenant URL: [https://server.codeium.com/scim/v2](https://server.codeium.com/scim/v2)

    <Frame>
      <img />
    </Frame>

    Leave the Azure provisioning page open, now go to the Windsurf web portal, and click on the profile icon  in the NavBar on the top of the page.Under Team Settings, select Service Key and click on Add Service Key. Enter any key name (such as 'Azure SCIM Provisioning'), **select the "SCIM Provisioning" role you created earlier**, and click Create Service Key. Copy the output key, go back to the Azure page, paste it to Secret Token.

    <Frame>
      <img />
    </Frame>

    (What you should see after creating the key on Windsurf)

    On the Provisioning page, click on Test Connection and that should have verified the SCIM connection.

    Now above the Provisioning form click on Save.

    ## Step 4: Configure SCIM Provisioning

    After clicking on Save, a new option Mappings should have appeared in the Provisioning page. Expand Mappings, and click on Provision Microsoft Entra ID Users

    <Frame>
      <img />
    </Frame>

    Under attribute Mappings, delete all fields under displayName, leaving only the fields userName, active, and displayName.

    <Frame>
      <img />
    </Frame>

    For active, now click on Edit. Under Expression, modify the field to

    ```
    NOT([IsSoftDeleted])
    ```

    Then click Ok.

    Your user attributes should look like

    <Frame>
      <img />
    </Frame>

    In the Attribute Mapping page, click on Save on top, and navigate back to the Provisioning page.

    Now click on the same page, under Mappings click on Provision Microsoft Entra ID Groups. Now only click delete for externalId, and click Save on top. Navigate back to the Provisioning page.

    <Frame>
      <img />
    </Frame>

    On the Provisioning page at the bottom, there should also be a Provisioning Status toggle. Set that to On to enable SCIM syncing. Now every 40 minutes your users and groups for the Entra ID application will be synced to Windsurf.

    <Frame>
      <img />
    </Frame>

    Click on Save to finish, you have now enabled user and group syncing for SCIM. Only users and groups assigned to the application will be synced to Windsurf. Note that removing users only disables them access to Windsurf (and stops them from taking up a seat) rather than deleting users due to Azure's SCIM design.
  </Tab>

  <Tab title="Okta SCIM">
    Windsurf supports SCIM synchronization for users and groups with Okta. It isn't necessary to setup SSO to use SCIM synchronization, but it is highly recommended.

    You'll need:

    * Admin access to Okta
    * Admin access to Windsurf
    * An existing Windsurf Application on Okta (normally from your existing SSO application)

    ## Step 1: Navigate to the existing Windsurf Application

    Go to Okta, click on Applications, Applications on the left sidebar, and then click on the existing Windsurf application in the application list.

    ## Step 2: Enable SCIM Provisioning

    Under the general tab, App Settings click on Edit on the top right. Then tick the 'Enable SCIM Provisioning' checkbox, then click Save. A new provisioning tab should have showed up on the top.

    Now go to provisioning, click Edit and input in the following fields:

    SCIM connector base URL: [https://server.codeium.com/scim/v2](https://server.codeium.com/scim/v2)

    Unique identifier field for users: email

    Supported provisioning actions: Push New Users, Push Profile Updates, Push Groups

    Authentication Mode: HTTP Header

    For HTTP Header - Authorization, you can generate the token from

    * [https://windsurf.com/team/settings](https://windsurf.com/team/settings) and go to the Service Key Configuration
    * Click on Configure, then Add Service Key, and give your API key a name
    * Copy the API key, go back to Okta and paste it to HTTP Header - Authorization

    Click on Save after filling out Provisioning Integration.

    ## Step 3: Setup Provisioning

    Under the provisioning tab, on the left there should be two new tabs. Click on To App, and Edit Provisioning to App. Tick the checkbox for Create Users, Update User Attributes, and Deactivate Users, and click Save.

    After this step, all users assigned to the group will now be synced to Windsurf.

    ## Step 4: Setup Group Provisioning (Optional)

    In order to sync groups to Windsurf, you will have to specify which groups to push. Under the application, click on the Push Groups tab on top. Now click on + Push Groups -> Find Groups by name. Filter for the group you would like to add, make sure Push group memberships immediately is checked, and then click Save. The group will be created and group members will be synced to Windsurf. Groups can then be used to filter for group analytics in the analytics page.
  </Tab>

  <Tab title="SCIM API">
    This guide shows how to create and maintain groups in Windsurf with the SCIM API.

    There are reasons one may want to provision groups manually rather than with their Identity Provider (Azure/Okta). Companies may want Groups provisioned from a different internal source (HR website, Sourcecode Management Tool etc.) that Windsurf doesn't have access to, or companies may finer control to Groups than what their Idendity Provider provides. Groups can thus be created with an API via HTTP request instead. The following provides examples on the HTTP request via CURL.

    There are 5 main APIs here, Create Group, Add group members, Replace group members, Delete Group, and List Users in a Group.

    ### Create Group

    ```
    curl -k -X POST https://server.codeium.com/scim/v2/Groups -d '{
    "displayName": "<group name>",
    "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Group"]
    }' -H "Authorization: Bearer <api secret key>" -H "Content-Type: application/scim+json"
    ```

    ### Add Group Members

    ```
    curl -X PATCH https://server.codeium.com/scim/v2/Groups/<group name> -d '{"schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
    "Operations":[
    {
    "op": "add",
    "path":"members",
    "value": [{"value": "<email 1>"}, {"value": "<email 2>"}]
    }]}' -H "Authorization: Bearer <api secret key>" -H "Content-Type: application/scim+json"
    ```

    ### Replace Group Members

    ```
    curl -X PATCH https://server.codeium.com/scim/v2/Groups/<group name> -d '{"schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
    "Operations":[
    {
    "op": "replace",
    "path":"members",
    "value": [{"value": "<email 1>"}, {"value": "<email 2>"}]
    }]}' -H "Authorization: Bearer <api secret key>" -H "Content-Type: application/scim+json"
    ```

    ### Delete Group

    ```
    curl -X DELETE https://server.codeium.com/scim/v2/Groups/<group name> -H "Authorization: Bearer <api secret key>" -H "Content-Type: application/scim+json"
    ```

    ### List Group

    ```
    curl -X GET -H "Authorization: Bearer <api secret key>" "https://server.codeium.com/scim/v2/Groups"
    ```

    ### List Users in a Group

    ```
    curl -X GET -H "Authorization: Bearer <api secret key>" "https://server.codeium.com/scim/v2/Groups/<group_id>"
    ```

    You'll have to at least create the group first, and then replace group to create a group with members in them. You'll also need to URL encode the group names if your group name has a special character like space, so a Group name such as 'Engineering Group' will have to be 'Engineering%20Group' in the URL.

    Note that users need to be created in Windsurf (through SCIM or manually creating the account) before they can be added to a group.

    ## User APIs

    There are also APIs for users as well. The following are some of the common SCIM APIs that Windsurf supports.

    Disable a user (Enable by replacing false to true):

    ```
    curl -X PATCH \
      https://server.codeium.com/scim/v2/Users/<user api key> \
      -H 'Content-Type: application/scim+json' \
      -H 'Authorization: Bearer <api secret key>' \
      -d '{
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [
          {
            "op": "replace",
            "path": "active",
            "value": false
          }
        ]
      }'
    ```

    Disable CLI access for a user (set `cliActive` to `true` to re-enable):

    ```
    curl -X PATCH \
      https://server.codeium.com/scim/v2/Users/<user api key> \
      -H 'Content-Type: application/scim+json' \
      -H 'Authorization: Bearer <api secret key>' \
      -d '{
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [
          {
            "op": "replace",
            "path": "cliActive",
            "value": false
          }
        ]
      }'
    ```

    The `cliActive` attribute controls whether a user can access Devin CLI. It is independent of the `active` attribute — disabling CLI access does not affect the user's seat or their access to other Windsurf products. If `cliActive` is not set for a user, they follow the team's default CLI access policy.

    Create a user:

    ```
    curl -X POST \
      https://server.codeium.com/scim/v2/Users \
      -H 'Content-Type: application/scim+json' \
      -H 'Authorization: Bearer <api secret key>' \
      -d '{
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
        "userName": "<email>",
        "displayName": "<full name>",
        "active": true
    }' 
    ```

    Update name:

    ```
    curl -X PATCH \
      'https://<enterprise portal url>/_route/api_server/scim/v2/Users/<user api key>' \
        -H 'Authorization: Bearer <service key>' \
        -H 'Content-Type: application/scim+json' \
        -d '{
          "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
          "Operations": [
            {
              "op": "Replace",
              "path": "displayName",
              "value": "<new name>"
            }
          ]
       }'
    ```

    ## Creating Api Secret Key

    Go to [https://windsurf.com/team/settings](https://windsurf.com/team/settings). Under Service Key Configuration, click on Add Service Key. Enter any key name (such as 'Azure Provisioning Key') and click Create Service Key. Copy the output key and save it, you can now use the key to authorize the above APIs.
  </Tab>

  <Tab title="Duo">
    ## Prerequisites

    This guide assumes that you have Duo configured and acts as your organizational IDP, or has external IDP configured.

    You will need administrator access to both Duo and Windsurf accounts.

    ## Configure Duo for Windsurf

    1. Navigate to Applications, and add a Generic SAML service provider

    <Frame>
      <img />
    </Frame>

    2. Navigate to SSO in Team Settings

    <Frame>
      <img />
    </Frame>

    3. When enabling SAML for the first time, you will be required to set up your SSO ID. **You will not be able to change it later.**

       It is advised to set this to your organization or team name with alphanumeric characters only.

    4. Copy the `Entity ID` value from the Duo portal and paste it into the `IdP Entity ID` field in the Windsurf portal.

    5. Copy the `Single Sign-On URL` value from the Duo portal and paste it into the `SSO URL` field in the Windsurf portal.

    6. Copy the certificate value from the Duo portal and paste it in the `X509 Certificate` field in the Windsurf portal

    <Frame>
      <img />
    </Frame>

    7. Copy the `SP Identity ID` value from the Windsurf portal and paste it into the `Entity ID` field in the Duo portal.

    8. Copy the `Callback URL (Assertion Consumer Service URL)` from the Windsurf portal and paste it into the `Assertion Consumer Service (ACS) URL` field in the Duo portal.

    9. In the Duo portal, configure the attribute statements as following:

    <Frame>
      <img />
    </Frame>

    10. Enable the SAML login in the Windsurf portal so you can test it.

    **NOTE: DO NOT LOGOUT OR CLOSE THE WINDOW AT THIS POINT.**

    If you get an error or it times out, troubleshoot your settings, otherwise you have to disable your SAML Settings in the Windsurf portal.

    **If you logout or close the window without confirming a successful test - you may get locked out.**

    11. Once your test was successfully completed, you may logout. You can now use SSO sign in when browsing to your team/organization page with the SSO ID you have configured in step 3.

    [https://www.codeium.com/yourssoid/login](https://www.codeium.com/yourssoid/login)
  </Tab>

  <Tab title="PingID">
    ## Prerequisites

    This guide assumes that you have PingID configured and acts as your organizational IDP, or has external IDP configured.

    You will need administrator access to both PingID and Windsurf accounts.

    ## Configure PingID for Windsurf

    1. Navigate to Applications and add Windsurf as a SAML Application

    <Frame>
      <img />
    </Frame>

    2. Navigate to SSO in Team Settings

    <Frame>
      <img />
    </Frame>

    3. When enabling SAML for the first time, you will be required to set up your SSO ID. **You will not be able to change it later.**

    It is advised to set this to your organization or team name with alphanumeric characters only.

    4. In PingID - choose to manually enter the configuration and fill out the fields with the following values:

    * ACS URLs - this is the `Callback URL (Assertion Consumer Service URL)` from the Windsurf portal.
    * Entity ID - this is the `SP Entity ID` from the Windsurf portal.

    <Frame>
      <img />
    </Frame>

    5. Copy the `Issuer ID` from PingID to the `IdP Entity ID` value in the Windsurf portal.

    6. Copy the `Single Signon Service` value from PingID to the `SSO URL` value in the Windsurf portal.

    7. Download the Signing Certificate from PingID as X509 PEM (.crt), open the file and copy its contents to the `X509 Certificate` value in the Windsurf portal.

    **Note**: make sure you have the fill begin and end lines with 5 dashes (-) and no other characters are copied!

    8. In attribute mappings, make sure to map:

    * `saml_subject` - Email Address
    * `firstName` - Given Name
    * `lastName` - Family Name

    <Frame>
      <img />
    </Frame>

    9. Add/edit any other policies and access as required by your setup/organization

    10. Enable the SAML login in the Windsurf portal so you can test it.

    **NOTE: DO NOT LOGOUT OR CLOSE THE WINDOW AT THIS POINT.**

    If you get an error or it times out, troubleshoot your settings, otherwise you have to disable your SAML Settings in the Windsurf portal.

    **If you logout or close the window without confirming a successful test - you may get locked out.**

    11. Once your test was successfully completed, you may logout. You can now use SSO sign in when browsing to your team/organization page with the SSO ID you have configured in step 3.

    [https://www.codeium.com/yourssoid/login](https://www.codeium.com/yourssoid/login)
  </Tab>
</Tabs>
