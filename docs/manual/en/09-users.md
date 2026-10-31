# Manage users

## Add a user

1. On the “Users” tab in the admin pages, select “Add user...”.

   ![The Users tab and user list](images/users-page.webp)

2. Choose and enter a username and password, then select “Add”.

   ![The Add user screen](images/user-add.webp)

To make the user an administrator, choose “Admin” under “Role” in the list.

## Roles

- **Editor**: Can register contents. They can change or delete only contents they registered. They can only view contents registered by other people in the admin pages.
- **Admin**: Can change or delete all contents. They can also manage users, shares, and site settings.

What editors can do in a share (view, add, move, delete) is set for each share. To give a person different permissions, in “Shares” in the admin pages, select the share row to open its edit page, then choose that person's permission under “Per-user exceptions” in the “Permissions” section (→ [Add shares](04-shares.md)). Administrators can do everything in every share.

## Reset a password

When someone forgets their password, an administrator chooses a new password and tells them.

1. In the “Users” list, select “⋮” on the row, then choose “Reset password...”.

   ![The user row menu](images/user-menu.webp)

2. Enter a new password, then select “Save”.

   ![The Reset password screen](images/user-password.webp)

Resetting the password also makes that user's recovery code stop working. Ask them to create a new one.

Users with a recovery code can also reset their own password (→ [Sign in and display](03-account.md)). If there is only one administrator, that administrator’s password can be reset only with their recovery code.

## Change a username

1. In the “Users” list, select “⋮” on the row, then choose “Change username...”.

   ![The user row menu](images/user-menu.webp)

2. Enter a new username, then select “Change”.

Let them know to log in with the new username from now on, including when they use their saved recovery code. Users can also change their own username (→ [Sign in and display](03-account.md)).

## Delete a user

1. In the “Users” list, select “⋮” on the row, then choose “Delete...”.

   ![The user row menu](images/user-menu.webp)

2. Select “Delete”.

   ![The Delete user confirmation](images/user-delete.webp)

Contents registered by the deleted user remain, and only administrators can change them.
The record of what they did in shares is kept.
Contents with “Creator only” visibility become “Hidden”.
If someone will take over, choose that person under “Creator” on the content edit page, then select “Save”.

You cannot delete the only administrator.
