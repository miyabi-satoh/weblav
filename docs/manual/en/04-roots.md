# Add allowed folders

An “allowed folder” is the area from which you can choose contents.
**Adding one alone does not show anything to viewers.**
Folders inside it appear only after you register them as “Folder” or “Archive” in [Register contents](05-register-content.md).

![How folders on the computer relate to the viewer's screen: add "Shared" as an allowed folder, then register "Teaching materials" as a folder and "EIKEN" as an archive. Only those two appear for viewers. Unregistered "Drafts" and "Household accounts" outside the allowed folder do not appear.](images/how-it-works.webp)

> **You can add folders only on the computer running WebLAV, when it is open at a `localhost` address.**
>
> - The “Add a folder...” button does not appear when you open the admin pages from a phone or another computer.
> - It also does not appear on the same computer when you use the QR code address, such as `http://my-pc.local:3000`.
> - You can rename and remove folders from the admin pages on any device.

## Open it

1. On the computer running WebLAV, choose “Open in browser” from the WebLAV icon in the task tray (menu bar on macOS, top right of the screen on Ubuntu). This opens `http://localhost:3000`.

   ![WebLAV icon menu in the task tray on Windows](images/quickstart-tray-menu.webp)

2. Sign in as an administrator, select your username at the top right, then choose “Admin”.

   ![The username menu at the top right](images/setup-admin-menu.webp)

3. Select the “Allowed folders” tab.

   ![The Allowed folders tab](images/roots-page.webp)

## Add one

1. Select “Add a folder...”.

   ![The Add a folder button](images/roots-page.webp)

2. In the folder selection window that opens, choose the folder you want to show (select “Select Folder” on Windows, “Open” on macOS, or “Select” on Ubuntu).

It is added to the list immediately.

Do not add a broad location such as `C:\` or your entire user folder. This would allow all files inside it to be registered.

## Add a folder on a NAS or an external disk

You can also add a shared folder on a NAS or a folder on an external disk, as long as the computer running WebLAV can open it.

1. On the computer running WebLAV, connect to the NAS share first. If you are asked for a password, save it (check “Remember my credentials” on Windows, or “Remember this password in my keychain” on macOS).
2. Select “Add a folder...”, then choose a folder in the share in the window that opens.

If you do not save the password, WebLAV cannot reconnect to the share, for example after the computer restarts.

When the NAS or disk is not connected, the list shows “Not connected” for that folder. People who open the folder are told that it cannot be reached. Once it is connected again, they can view it as before.

Only let people you trust write to the NAS share. Files they put in the share also appear to viewers.

## Rename one

The name appears instead of the path when choosing a folder for content and when showing the content location.

1. Select “⋮” on the row, then choose “Rename...”.

   ![The allowed folder row menu](images/roots-menu.webp)

2. Enter a name, then select “Save”.

   ![The Rename screen](images/roots-rename.webp)

## Delete one

> **Deleting it here does not delete folders or files on your computer.**
>
> It removes only the entry from WebLAV’s “Allowed folders” list. The folder stays where it is.

1. Select “⋮” on the folder row, then choose “Delete...”.

   ![The allowed folder row menu](images/roots-menu.webp)

2. Select “Delete”.

   ![The removal confirmation](images/roots-delete.webp)

Contents registered inside it stop being viewable, but their entries remain. Add the same folder again to make them viewable again. You can add it again only on the computer running WebLAV.

Next, go to [Register contents](05-register-content.md).
