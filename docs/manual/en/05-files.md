# Work with files in shares

Signed-in users can view, add, and move files in shares from a browser. This works the same from a phone.
What you can do depends on your permission for each share (→ [Add shares](04-shares.md)). Administrators can do everything.

## Open a share

1. Sign in, then select a share in the “Shares” section below the list on Home.

   The second line of each row shows what you can do in that share (“You: read only”, “You: add only”, or “You: full access”).

2. Select a folder to go into it. Select an item in “Home / Shares / share name / …” at the top to go back.

Select a file to open it in place, just like in folder content (→ [View files](08-viewing.md)).

When you open a folder listed on Home (→ [Register contents](06-register-content.md)), the same actions appear if it is a place in a share where you can make changes.

## Upload files

You can do this in shares with “Add only” or “Full access”.

1. Open the folder to put the files in, select “Add” to the right of the heading, then choose “Upload...”.
2. Select the area to choose files, or drop them there. You can choose any number of files. On a phone, you can also choose from your photos.
3. Select “Upload”.

Progress appears for each file. If you select “Stop” partway, files already sent are kept.

- If a file with the same name exists, the new file is not overwritten but saved with a number, such as `photo (1).jpg`. The saved name is shown.
- If a photo's contents do not match its extension (for example, an iPhone photo arrives as JPEG with a `.HEIC` name), the extension is corrected when it is saved.
- If an administrator has set an upload limit, larger files cannot be sent (→ [Maintain WebLAV](10-maintenance.md)).

## Create a new folder

1. Select “Add” to the right of the heading, then choose “New folder...”.
2. Enter a name, then select “Create”.

Names that start with a dot or contain characters not allowed on Windows cannot be used.

## Rename

You can do this in shares with “Full access”.

1. Select “⋮” on the row, then choose “Rename...”.
2. Enter a name, then select “Rename”.

A share itself, and folders that contain another share, cannot be renamed.

## Move or copy

You can move in shares with “Full access”, and copy from any share you can see. The destination must be a share with “Add only” or “Full access”.

1. Select “⋮” on the row, then choose “Move...” or “Copy...”.
2. Browse from a share to the destination folder, then select “Move here” or “Copy here”.
3. If an item with the same name exists at the destination, choose what to do.
   - “Overwrite”: Moves the existing item to the trash, then puts the new one in its place. Available only when the destination is a share with “Full access”.
   - “Add a number”: Saves it with a numbered name.
   - “Skip”: Does not put it there.

> **In places with no trash, overwritten items cannot be restored.**
>
> NAS and network drives, for example, have no trash. In that case, the confirmation screen says so.

## Delete

You can do this in shares with “Full access”.

1. Select “⋮” on the row, then choose “Delete...”.
2. Read the confirmation screen, then select “Delete”.

> **“Delete” in a share deletes the files on the computer.**
>
> Usually they are moved to the computer's trash, so you can restore them from there. In places with no trash, such as a NAS or a network drive, they are deleted permanently and cannot be restored. When the confirmation screen says it “can't be undone”, check carefully before you select it.

If a folder you are deleting is listed on Home, it appears on the confirmation screen. After deleting, that content can no longer be opened (restoring it from the trash makes it open again).

## Long-running tasks

When moving, copying, or deleting does not finish right away, its progress appears at the bottom right of the screen. It continues even if you go to another page. Select “Stop” to stop it. What was already done stays as it is.

Quitting WebLAV stops any task in progress.

## List on Home

This shows a folder in a share on Home as folder content. You can do this in any share you can see.

1. Select “⋮” on the folder row, then choose “List on Home...”.
2. Choose the visibility, then select “List”.

The title is the folder name. To change it, use the edit page in the admin pages (→ [Register contents](06-register-content.md)).

Folders listed on Home show “Listed”. When you open such a folder, “Listed on Home as “title”” appears below the heading.

## Publish settings and renaming in shares

Items you rename or move on the share screens keep their publish setting in folders listed on Home, and their place in the library list. Items you rename or move in File Explorer or Finder do not (→ [Register contents](06-register-content.md)).

## Activity records

Adding, moving, and deleting in shares are recorded with who did it and when, and administrators can see the records (→ [Maintain WebLAV](10-maintenance.md)).
