# Manage users

## Add a user

1. On the “Users” tab in the admin pages, select “Add user...”.

   ![The Users tab and user list](images/users-page.webp)

2. Choose and enter a username and password, then select “Add”.

   ![The Add user screen](images/user-add.webp)

After you add the user, their edit page opens. To make them an administrator, choose “Admin” under “Role”, then select “Save” (→ “Change a username or role” below).

## Roles

- **Standard user**: Can register content. They can change or delete only content they registered. They cannot change content registered by other people. They cannot open the admin pages.
- **Admin**: Can change or delete all content. They can also manage users, shares, and site settings.

Change your own username and password in “Account”, like everyone else (→ [Log in and display](03-account.md)). In the “Users” list, your own row is marked “(you)” and does not open. To make yourself a standard user or to delete yourself, ask another administrator.

What standard users can do in a share (view, add, move, delete) is set for each share. To give a person different permissions, in “Shares” in the admin pages, select the share row to open its edit page, then add that person under “Per-user exceptions” and choose their permission in the “Permissions” section (→ [Add shares](04-shares.md)). Administrators can do everything in every share.

## Reset a password

When someone forgets their password, an administrator chooses a new password and tells them.

1. In the “Users” list, select the person's row to open their edit page.
2. Select “⋮” next to the username, then choose “Reset password...”.
3. Enter a new password, then select “Save”.

   ![The Reset password screen](images/user-password.webp)

Resetting the password also makes that user's recovery code stop working. Ask them to create a new one.

Users with a recovery code can also reset their own password (→ [Log in and display](03-account.md)). If there is only one administrator, that administrator’s password can be reset only with their recovery code.

## Change a username or role

1. In the “Users” list, select the person's row. Their edit page opens.
2. Change “Username” or “Role”, then select “Save” in that section.

   ![The user edit page](images/user-edit.webp)

After changing a username, let them know to log in with the new username from now on, including when they use their saved recovery code. Users can also change their own username (→ [Log in and display](03-account.md)).

## Delete a user

1. In the “Users” list, select “⋮” on the row, then choose “Delete...”. (You can also use the “Delete” section at the bottom of the edit page.)

   ![The user row menu](images/user-menu.webp)

2. Select “Delete”.

   ![The Delete user confirmation](images/user-delete.webp)

Content registered by the deleted user remains, and only administrators can change it.
Content items with “Creator only” visibility become “Hidden”. To show them again, an administrator chooses a new “Visibility” on the content edit page.
If someone will take over, choose that person under “Creator” on the content edit page, then select “Save”.
The record of what they did is kept.
