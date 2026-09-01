# Role Based Access & Management

Configure RBAC permissions, create custom roles, and manage user access for Windsurf Enterprise plans.

Windsurf's Role-Based Access Control system provides granular, role-based access to enterprise resources, enabling administrators to assign permissions and roles dynamically for secure and efficient access management.

<Note>Role-based access features are available for Enterprise plans only.</Note>

## Role Based Access Controls

Windsurf's role-based access system allows enterprise organizations to implement fine-grained access controls across all team resources. The system enables:

* **Granular Permission Management**: Control access to specific features and data based on user roles
* **Dynamic Role Assignment**: Administrators can assign and modify roles for individual users or user groups
* **Secure Resource Access**: Ensure users only have access to the resources they need for their responsibilities
* **Audit and Compliance**: Track user permissions and access patterns for security and compliance requirements

The role-based access system integrates seamlessly with Windsurf's existing authentication mechanisms, including SSO and SCIM, to provide a comprehensive security framework for enterprise deployments.

## Role Management

<Note>We are continually working to improve role management features and functionality.</Note>

Roles can be created and managed in the Windsurf admin console via the Settings tab. For Windsurf's SaaS offering, access the Settings tab at:

<Card title="Team Settings" icon="gear" href="https://windsurf.com/team/settings">
  Manage roles, permissions, and team settings from the admin console.
</Card>

### Creating a New Role

<Steps>
  <Step title="Navigate to Role Management">
    Go to [windsurf.com/team/settings](https://windsurf.com/team/settings) and locate the Role Management section.
  </Step>

  <Step title="Create Role">
    Click the **"Create Role"** button to start creating a new role.
  </Step>

  <Step title="Configure Role">
    Enter a descriptive name for the role and select the appropriate permissions from the checkbox list.
  </Step>

  <Step title="Save Role">
    Review your selections and save the new role. It will now be available for assignment to users.
  </Step>
</Steps>

## Role Permissions

Windsurf provides two default roles out of the box:

* **Admin Role**: Includes all available permissions for complete system access
* **User Role**: Includes no permissions by default, providing a minimal access baseline

### Modifying Role Permissions

To modify permissions for custom roles, click the permissions dropdown next to the role name in the Role Management section. This allows you to add or remove specific permissions as needed.

### Available Permissions

Windsurf offers a comprehensive set of permissions organized into the following categories:

#### Attribution

* **Attribution Read**: Read access to the attribution page

#### Analytics

* **Analytics Read**: Read access to the analytics page

#### Teams

* **Teams Read-Only**: Read-only access to the teams page
* **Teams Update**: Allows updating user roles in the teams page
* **Teams Delete**: Allows deleting users from the teams page
* **Teams Invite**: Allows inviting users to the teams page

#### Indexing

* **Indexing Read**: Read access to the indexing page
* **Indexing Create**: Create access to the indexing page
* **Indexing Update**: Allows updating indexed repos
* **Indexing Delete**: Allows deleting indexes
* **Indexing Management**: Allows index database management and pruning

#### SSO

* **SSO Read**: Read access to the SSO page
* **SSO Write**: Write access to the SSO page

#### Service Key

* **Service Key Read**: Read access to the service keys page
* **Service Key Create**: Allows creating service keys
* **Service Key Update**: Allows updating service keys
* **Service Key Delete**: Allows deleting service keys

#### Billing

* **Billing Read**: Read access to the billing page
* **Billing Write**: Write access to the billing page

#### Role Management

* **Role Read**: Read access to the roles tab in settings
* **Role Create**: Able to create new roles
* **Role Update**: Allows updating roles
* **Role Delete**: Allows deleting roles

#### Team Settings

* **Team Settings Read**: Allows read access to team settings
* **Team Settings Update**: Allows updating team settings

### Disable Windsurf Access Feature

For administrators who need access to team analytics and audit/attribution logging but do not wish to consume a license, Windsurf provides a "disable Windsurf access" feature.

To access this feature:

<Steps>
  <Step title="Navigate to Manage Team">
    Go to the **"Manage Team"** tab in your team settings.
  </Step>

  <Step title="Edit User">
    Find the user you want to modify and click **"Edit"** next to their name.
  </Step>

  <Step title="Disable Access">
    In the user edit dialog, you can disable their Windsurf access while maintaining their administrative permissions for analytics and logging.
  </Step>
</Steps>

## User Groups

<Note>User Groups are available for Enterprise organizations with SCIM integration enabled.</Note>

For enterprise organizations, Windsurf offers the ability to split users into multiple user groups via SCIM (System for Cross-domain Identity Management) integration. This feature enables:

* **Organizational Structure**: Mirror your company's organizational structure within Windsurf
* **Group-Based Analytics**: View analytics and usage data filtered by specific user groups
* **Delegated Administration**: Assign group administrators who can manage specific user groups
* **Scalable Management**: Efficiently manage large numbers of users through group-based operations

User groups are automatically synchronized with your identity provider through SCIM, ensuring that organizational changes are reflected in Windsurf's access controls.

## User Management

Windsurf's role-based access functionality allows administrators to assign roles to individual users or user groups, providing flexible access control management.

### Assigning Roles to Users

User role management is performed in the Windsurf admin console at [windsurf.com/team/settings](https://windsurf.com/team/settings).

<Steps>
  <Step title="Navigate to User Management">
    Go to the team settings page and locate the user management section.
  </Step>

  <Step title="Find User">
    Scroll through the user list or use the search functionality to find the user you want to modify. Users can be sorted alphabetically by name, email, sign-up time, or last login.
  </Step>

  <Step title="Edit User Role">
    Click **"Edit"** next to the user's name to open the user management dialog.
  </Step>

  <Step title="Select Role">
    In the pop-out window, select the appropriate role from the dropdown menu.
  </Step>

  <Step title="Save Changes">
    Confirm your selection and save the changes. The new role will be applied immediately.
  </Step>
</Steps>

### Administrative Hierarchy

Windsurf's role-based access system recognizes different levels of administrative access:

* **Super Admin**: Users with the admin role in the "all users" group have complete system access and can modify any role or permission
* **Group Admins**: Administrators of specific user groups can only make role and permission changes within their assigned groups

This hierarchical structure ensures that administrative responsibilities can be delegated appropriately while maintaining security boundaries.

### User Sorting and Management

The user management interface provides several sorting options to help administrators efficiently manage large teams:

* **Alphabetical by Name**: Sort users by their display names
* **Email Address**: Sort users by their email addresses
* **Sign-up Time**: View users in order of when they joined the team
* **Last Login**: Sort by most recent activity to identify active users

These sorting options make it easier to find specific users and understand team engagement patterns.
