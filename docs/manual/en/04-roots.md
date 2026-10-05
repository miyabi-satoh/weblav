# Add shared folders

A “shared folder” is the area from which you can choose contents.
**Adding one alone does not show anything to viewers.**
Folders inside it appear only after you register them as “Folder” or “Archive” in [Register contents](05-register-content.md).

![How folders on the computer relate to the viewer's screen: add "Shared" as a shared folder, then register "Teaching materials" as a folder and "EIKEN" as an archive. Only those two appear for viewers. Unregistered "Drafts" and "Household accounts" outside the shared folder do not appear.](images/how-it-works.webp)

> **You can do this only on the computer running WebLAV, when it is open at a `localhost` address.**
>
> - “Shared folders” does not appear in the admin pages when you open them from a phone or another computer.
> - It also does not appear on the same computer when you use the QR code address, such as `http://my-pc.local:3000`.

## Open it

1. On the computer running WebLAV, choose “Open in browser” from the WebLAV icon in the task tray (menu bar on macOS). This opens `http://localhost:3000`.

   ![WebLAV icon menu in the task tray on Windows](images/quickstart-tray-menu.webp)

2. Sign in as an administrator, select your username at the top right, then choose “Admin”.

   ![The username menu at the top right](images/setup-admin-menu.webp)

3. Select the “Shared folders” tab.

   ![The Shared folders tab](images/roots-page.webp)

## Add one

1. Select “Add a folder...”.

   ![The Add a folder button](images/roots-page.webp)

2. In the folder selection window that opens, choose the folder you want to show (select “Select Folder” on Windows or “Open” on macOS).

It is added to the list immediately.

Do not add a broad location such as `C:\` or your entire user folder. This would allow all files inside it to be registered.

## Rename one

The name appears instead of the path when choosing a folder for content and when showing the content location.

1. Select “⋮” on the row, then choose “Rename...”.

   ![The shared folder row menu](images/roots-menu.webp)

2. Enter a name, then select “Save”.

   ![The Rename screen](images/roots-rename.webp)

## Delete one

> **Deleting it here does not delete folders or files on your computer.**
>
> It removes only the entry from WebLAV’s “Shared folders” list. The folder stays where it is.

1. Select “⋮” on the folder row, then choose “Delete...”.

   ![The shared folder row menu](images/roots-menu.webp)

2. Select “Delete”.

   ![The removal confirmation](images/roots-delete.webp)

Contents registered inside it stop being viewable, but their entries remain. Add the same folder again to make them viewable again.

Next, go to [Register contents](05-register-content.md).
