# FedRAMP Security Admin Guide

Windsurf FedRAMP Security Admin Guide for securely setting up, configuring, operating, and decommissioning top-level administrative accounts. Includes role definitions, account lifecycle procedures, and a reference table of all admin-controlled security settings.

# FedRAMP Security Admin Guide

This guide describes how to securely set up, configure, operate, and decommission top-level administrative accounts in Windsurf. It covers administrative role definitions, account lifecycle procedures, and all admin-controlled security settings with their associated functions, security impacts, and recommended values.

<Note>This guide is written for the Windsurf FedRAMP deployment which runs on AWS GovCloud. The FedRAMP deployment uses a dedicated enterprise portal and SSO-based authentication (OIDC or SAML 2.0). Some features described in other Windsurf documentation for the SaaS offering are not available in the FedRAMP environment.</Note>

***

## Administrative role definitions

Windsurf uses a Role-Based Access Control (RBAC) system to govern administrative privileges. Roles are managed through the Admin Portal under the Role Management settings section and can be assigned to individual users.

### Built-in roles

Windsurf provides two built-in roles that cannot be deleted.

| Role      | Description                                                                                                                                                                   | Default permissions           |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| **Admin** | Full administrative access to organization settings, user management, analytics, and security controls. This is the highest level of privilege a user can hold within a team. | All permissions enabled       |
| **User**  | Standard end-user access with no administrative permissions. Users can access Windsurf's coding features but cannot view or modify organization settings.                     | No administrative permissions |

### Custom roles

Administrators can create custom roles to implement the principle of least privilege. Custom roles are composed of granular permissions selected from the categories below. To create a custom role, navigate to the Admin Portal and open the Role Management section under Settings.

### Permission reference

The table below lists every permission available for role assignment in the FedRAMP deployment. Each permission controls access to a specific administrative function.

| Category            | Permission               | Description                                              |
| ------------------- | ------------------------ | -------------------------------------------------------- |
| **Teams**           | Teams Read-Only          | Read-only access to the teams management page            |
| **Teams**           | Teams Update             | Ability to update user roles on the teams page           |
| **Teams**           | Teams Delete             | Ability to remove users from the teams page              |
| **Analytics**       | Analytics Read           | Read access to the analytics page and dashboards         |
| **Attribution**     | Attribution Read         | Read access to the attribution page                      |
| **License**         | License Read             | Read access to the license page                          |
| **SSO**             | SSO Read                 | Read access to the SSO configuration page                |
| **SSO**             | SSO Write                | Ability to configure and modify SSO provider settings    |
| **Service Key**     | Service Key Read         | Read access to the service keys page                     |
| **Service Key**     | Service Key Create       | Ability to create new service keys for API access        |
| **Service Key**     | Service Key Update       | Ability to modify existing service keys                  |
| **Service Key**     | Service Key Delete       | Ability to revoke and delete service keys                |
| **Role Management** | Role Read                | Read access to the roles tab in settings                 |
| **Role Management** | Role Create              | Ability to create new roles                              |
| **Role Management** | Role Update              | Ability to modify existing role definitions              |
| **Role Management** | Role Delete              | Ability to delete roles                                  |
| **External Chat**   | External Chat Management | Ability to modify external chat model configurations     |
| **Indexing**        | Indexing Read            | Read access to the indexing configuration page           |
| **Indexing**        | Indexing Create          | Ability to create new indexes                            |
| **Indexing**        | Indexing Update          | Ability to update existing indexed repositories          |
| **Indexing**        | Indexing Delete          | Ability to delete indexes                                |
| **Indexing**        | Indexing Management      | Ability to perform index database management and pruning |
| **Fine-Tuning**     | Fine-Tuning Read         | Read access to the fine-tuning page                      |
| **Fine-Tuning**     | Fine-Tuning Create       | Ability to create fine-tuning jobs                       |
| **Fine-Tuning**     | Fine-Tuning Update       | Ability to update fine-tuning jobs                       |
| **Fine-Tuning**     | Fine-Tuning Delete       | Ability to delete fine-tuning jobs                       |

<Note>A number of these permissions (such as Attribution, License, SSO, Indexing, Fine-Tuning) exist in the RBAC system but their corresponding portal pages are not available in the FedRAMP multitenant deployment. These permissions are included in the role management UI for completeness but do not grant access to any active features in this environment.</Note>

***

## Admin account lifecycle procedures

This section describes the end-to-end lifecycle of a top-level administrative account, from initial creation through decommissioning.

### Account setup

**SSO-based onboarding** is the primary provisioning method in the FedRAMP deployment. The platform supports both OIDC and SAML 2.0 for Single Sign-On integration. Users authenticate through the configured identity provider, and after the user's first login creates their account, an administrator assigns the appropriate role through the Admin Portal. Note that SSO integration in the FedRAMP environment requires coordination with the Windsurf FedRAMP team and cannot be configured in a self-serve capacity.

Every new admin account should be configured according to the principle of least privilege. Prefer custom roles with only the permissions needed for the administrator's responsibilities rather than assigning the full Admin role unless the user requires complete system access.

### Authentication and MFA requirements

The FedRAMP deployment uses Single Sign-On exclusively, supporting both OIDC and SAML 2.0 protocols. Email and password authentication is not available. All users must authenticate through the configured identity provider.

Multi-Factor Authentication (MFA) is enforced through the organization's identity provider. Windsurf inherits the MFA policies configured in the connected IdP, meaning that all authentication strength requirements (such as requiring a second factor, phishing-resistant authenticators, or conditional access policies) are governed at the IdP level. Organizations should configure their IdP to require MFA for all users accessing the Windsurf application, particularly for accounts holding administrative roles.

<Warning>Windsurf strongly recommends requiring MFA for all administrative accounts. Configure your identity provider to enforce MFA as a condition for accessing the Windsurf application.</Warning>

### Account configuration

After an administrative account is created, the following configuration steps should be completed.

**Role assignment** determines the scope of the account's administrative access. Assign roles through the Admin Portal by navigating to the Manage Team tab, locating the user, clicking Edit, and selecting the appropriate role from the dropdown. Changes take effect immediately.

**Service key management** is required when the administrator needs API access for automation or analytics. Service keys are created under Settings with scoped permissions matching the key's intended use. Each service key should be named descriptively (for example, "Analytics Dashboard") and assigned a role with the minimum permissions required.

### Account operation

Ongoing operational practices for administrative accounts include the following.

**Regular access reviews** should be conducted to verify that administrative accounts still require their current level of access. Review the list of users with the Admin role periodically through the Manage Team tab and adjust roles as responsibilities change.

**Activity monitoring** is available through the built-in analytics dashboards. Administrators with Analytics Read permission can track user activity, engagement metrics, and feature usage. The Analytics API provides programmatic access to this data for integration with external monitoring systems.

**Service key rotation** should be performed on a regular schedule. To rotate a key, create a new service key with the same permissions, update the consuming system to use the new key, and then delete the old key.

### Account decommissioning

When an administrator no longer requires access, the account should be decommissioned promptly using the following procedure.

<Steps>
  <Step title="Revoke administrative role">
    Navigate to the Admin Portal, open the Manage Team tab, locate the user, click Edit, and change their role from Admin to User (or a custom role with no administrative permissions).
  </Step>

  <Step title="Revoke service keys">
    Delete any service keys that were created by or exclusively used by the departing administrator. Navigate to Settings, then Service Key, and delete the relevant keys.
  </Step>

  <Step title="Remove or deactivate the account">
    Remove the user through the Manage Team tab by clicking Delete next to their name. This will deactivate the user's Windsurf account and release their license seat.
  </Step>

  <Step title="Review residual access">
    Verify that the decommissioned account no longer appears in any administrative role by checking the Manage Team user list filtered by the Admin role. Confirm that all service keys associated with the account have been deleted.
  </Step>
</Steps>

<Warning>Decommission administrative accounts immediately when an administrator changes roles or leaves the organization. Delayed decommissioning creates unnecessary security exposure.</Warning>

***

## Security settings reference

The table below documents all admin-controlled security settings available in the FedRAMP deployment's Admin Portal. Each entry describes the setting's function, its security impact, and the recommended configuration for a security-conscious deployment.

| Setting                              | Function                                                                                                                                                                                                                                                                   | Security impact                                                                                                                                                                                           | Recommended value                                                                                                                                                                    |
| ------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Role-Based Access Control (RBAC)** | Controls which administrative actions each user can perform based on their assigned role and permissions. Managed under the Role Management section in Settings.                                                                                                           | Limits the blast radius of compromised accounts by restricting permissions to only what each user needs. Overly broad role assignments increase the potential impact of a single account compromise.      | **Configure with least privilege.** Create custom roles with only the permissions each administrator requires. Reserve the built-in Admin role for a small number of administrators. |
| **Service key permissions**          | Scopes API access tokens to specific permission sets, controlling which operations automated systems can perform. Managed under the Service Key section in Settings.                                                                                                       | Service keys with excessive permissions can be exploited if leaked, granting unauthorized access to user management, analytics, or other functions.                                                       | **Scope to minimum required permissions.** Create dedicated service keys for each integration with only the permissions that integration needs. Rotate keys regularly.               |
| **SSO provider configuration**       | Configures the identity provider used for all user authentication, supporting both OIDC and SAML 2.0 protocols. Email/password authentication is not available. SSO setup requires coordination with the Windsurf FedRAMP team. Managed under the SSO section in Settings. | Centralizes authentication through the organization's IdP, enabling enforcement of MFA, conditional access, and session policies. Misconfiguration could lock out all users or allow unauthorized access. | **Configure with your organization's approved identity provider (OIDC or SAML 2.0).** Verify the configuration by testing login with a non-admin account before rolling out broadly. |

*Last updated: January 28, 2026*
