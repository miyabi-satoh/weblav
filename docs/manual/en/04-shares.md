# Add shares

A “share” is a folder on the computer that you have decided WebLAV may work with.
In a share, logged-in users can view, add, or move files (→ [Work with files in shares](05-files.md)).
To show something on the viewer's screen (Home), register a folder or file in a share as a “Shortcut” (→ [Register content](06-register-content.md)).
**Adding a share alone does not show anything to people who are not logged in.**

![How folders on the computer relate to the viewer's screen: add "学校" (School) as a share, then register "教材" (Teaching materials) and "英検" (EIKEN) inside it as shortcuts. Only those two appear for viewers. Unregistered "下書き" (Drafts) and "家計簿" (Household accounts) outside the share do not appear.](images/how-it-works.webp)

> **You can add shares only on the computer running WebLAV, when it is open at a `localhost` address.**
>
> - The “Add share...” button does not appear when you open the admin pages from a phone or another computer.
> - It also does not appear on the same computer when you use the QR code address, such as `http://my-pc.local:8370`.
> - The same applies to choosing the location again. You can rename shares, set permissions, and remove shares from the admin pages on any device.

## Open it

1. On the computer running WebLAV, choose “Open in browser” from the WebLAV icon in the task tray (menu bar on macOS, top right of the screen on Ubuntu). This opens `http://localhost:8370`.

   ![WebLAV icon menu in the task tray on Windows](images/quickstart-tray-menu.webp)

2. Log in as an administrator, select your username at the top right, then choose “Admin”.

   ![The username menu at the top right](images/setup-admin-menu.webp)

3. Select the “Shares” tab.

   ![The Shares tab](images/roots-page.webp)

## Add one

1. Select “Add share...”.

   ![The Add share button](images/roots-page.webp)

2. In the folder selection window that opens, choose the folder you want to make a share (select “Select Folder” on Windows, “Open” on macOS, or “Select” on Ubuntu).

It is added immediately, and the edit page of that share opens. Its name is the folder name. You can go on to set its name and permissions.

Selecting a row in the list opens the edit page of that share at any time.

Do not add a broad location such as `C:\` or your entire user folder. All files inside it could be registered, and depending on permissions, other people could move or delete them.

## Set permissions

Administrators can do everything in every share.
What standard users can do in a share is set for each share.

| Permission | What they can do |
|---|---|
| No access | They cannot see the share |
| Read only | View, download, copy to other shares, and list on Home (default) |
| Add only | Read only, plus upload, create new folders, and copy in. They cannot rename, move, delete, or overwrite existing items |
| Full access | Add only, plus rename, move, delete, and overwrite |

1. Select the share row to open its edit page.
2. In the “Permissions” section, choose “Default for standard users”.
3. If some people need a different permission from the default, select “Add exception” under “Per-user exceptions”, choose the person, then choose their permission in the row. Select × in the row to return to the default.
4. Select “Save” in the “Permissions” section.

The list shows the default for standard users (such as “Read only”) and the number of exceptions.

A share set to “No access” does not appear for that person when they choose a folder to register content, either.

When a share is inside another share, the stronger of the outer and inner permissions applies. People who can see the outer share can also reach the inner one through it.

## Add a folder on a NAS or an external disk

You can also add a shared folder on a NAS or a folder on an external disk, as long as the computer running WebLAV can open it.

1. On the computer running WebLAV, connect to the NAS shared folder first. If you are asked for a password, save it (check “Remember my credentials” on Windows, or “Remember this password in my keychain” on macOS).
2. Select “Add share...”, then choose a folder in the NAS shared folder in the window that opens.

If you do not save the password, WebLAV cannot reconnect to the NAS, for example after the computer restarts.

When the NAS or disk is not connected, the list shows “Not connected” for that folder. People who open the folder are told that it cannot be reached. Once it is connected again, they can view it as before.

Only let people you trust write to the NAS shared folder. Files they put there also appear to viewers.

NAS and network drives have no trash. Items deleted or overwritten in such a share cannot be restored (→ [Work with files in shares](05-files.md)).

## Choose the location again after moving the folder

If you moved the folder of a share to another disk or a NAS, do not delete the share; choose its new location instead. The name and permissions stay, and content registered inside it opens the same folders in the new location.

1. On the computer running WebLAV, select the share row to open its edit page.
2. In the “Location” section, select “Choose folder again...”.
3. In the window that opens, choose the folder at its new location.
4. Check the new location in the confirmation, then select “Change”.

Keep the folders inside in the same arrangement as before the move. Content inside opens the folders with the same names in the new location.

## Rename one

The name appears instead of the path when choosing a folder for content, when showing the content location, and in the “Shares” section on Home.

1. Select the share row to open its edit page.
2. Enter a name in the “Name” section, then select “Save” in that section.

## Delete one

> **Deleting it here does not delete folders or files on your computer.**
>
> It removes only the entry from WebLAV’s “Shares” list. The folder stays where it is.

1. Select “⋮” on the share row, then choose “Delete...” (you can also use the “Delete” section at the bottom of the edit page).

   ![The share row menu](images/roots-menu.webp)

2. Select “Delete”.

   ![The removal confirmation](images/roots-delete.webp)

Content registered inside it stops being viewable, but its entries remain. Add the same folder again to make them viewable again. You can add it again only on the computer running WebLAV.

Next, go to [Work with files in shares](05-files.md) or [Register content](06-register-content.md).
