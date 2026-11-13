# Register content

## Types

| What you want to show | Type |
|---|---|
| A folder or file on your computer | Shortcut |
| A list filterable by year or session | Library (create a shortcut, then switch it → [Set up a library](07-archive-setup.md)) |
| The URL of an external site | Link |
| A collection of items | Group |

Shortcuts can only point inside “Shares” (→ [Add shares](04-shares.md)).
They also let you organize things differently in groups without moving the actual files. To show files you have at hand, upload them into a share first (→ [Work with files in shares](05-files.md)).
Only items registered here appear for viewers.

![How folders on the computer relate to the viewer's screen: add "学校" (School) as a share, then register "教材" (Teaching materials) and "英検" (EIKEN) inside it as shortcuts. Only those two appear for viewers. Unregistered "下書き" (Drafts) and "家計簿" (Household accounts) outside the share do not appear.](images/how-it-works.webp)

Titles are assigned automatically. To change one, use the edit page that opens after registration.

## Start

On Home or on a group's page, select “Add” to the right of the heading and choose a type. On a group's page, the new item goes inside that group.

![“Add” to the right of the Home heading, and the type menu](images/add-menu.webp)

Administrators can also add from the admin pages.

1. On the “Content” tab in the admin pages, select “Add content...”.

   ![The Content tab and Add button](images/contents-page.webp)

2. Choose a type.

   ![Choosing a type when adding content](images/quickstart-add-folder.webp)

The remaining steps differ by type.

## Shortcut

1. Select “Choose...”.

   ![The Add shortcut screen](images/add-folder-form.webp)

2. Browse from a share. For a folder, open it and select “Use this folder”. For a file, select it and then select “Use this file”.

   ![The screen for choosing a folder or file](images/add-folder-picker.webp)

3. Select “Add”. If you chose a folder, select “Register” in the confirmation.

   ![The Add screen after choosing a location](images/add-folder-ready.webp)

You can also create one with “List on Home...” in the “⋮” menu of a folder or file row on the share screen (→ [Work with files in shares](05-files.md)).

### Shortcuts to a file

These open the same way as files in a list (→ [View files](08-viewing.md)). Images, PDFs, videos, text, and Office documents open in the viewer on the page, audio plays in the row's player, and files whose names end with `.links.toml` open as a list of links. Other kinds of files download. You can step between the files in the same list, and audio can play continuously.

A shortcut follows its file when you rename or move the file on the share screens. If the file is renamed or moved in File Explorer or Finder, or deleted, the row stays, and opening it shows “The file was not found.” Choose the file again with “Change...” under “Path” on the edit page.

Shortcuts to a file have no pattern fields and cannot become libraries.

### Limit what you show

By default, everything in a folder that a shortcut points to is visible. To limit what you show, use “Include patterns” and “Exclude patterns” under “Location and display”. Select “Edit...”, write one pattern per line, select “Use these patterns”, then select “Save” in that section.

- `*.pdf`: files with a matching name at any level
- `backup/`: folders with that name at any level
- `docs/*.pdf`: files directly inside “docs” at the top (a pattern containing `/` starts at the top of the folder)

If “Include patterns” is empty, everything is included. “Exclude patterns” takes priority. Filtered-out items don't appear in the folder view, downloads, or search, for viewers or for people who can change the folder. They still appear on the share screen. When only “Include patterns” is set, subfolders appear even if no file inside them matches.

When saving patterns would make more or fewer files visible, the counts appear before the save.

## Link

1. Enter the URL, then select “Add”.

   ![The Add link screen](images/add-link.webp)

To show several links together, write them in a text file whose name ends with `.links.toml`, and put it in a folder.
When opened, its links are listed the same way as registered links.

```toml
title = "Today's picks"

[[links]]
url = "https://example.com/"
note = "A description shown under the link (optional)"
```

- Write one `[[links]]` per link. They are listed in the order written.
- If you leave out `title`, the file name is used as the title.
- Save the file as UTF-8. If it cannot be read, a message and a button to open the file in a new tab are shown.

## Group

1. Enter a title, then select “Add”.

   ![The Add group screen](images/add-group.webp)

## Change visibility

| Visibility | Who can see it |
|---|---|
| Everyone | Everyone, including people who aren't logged in (default) |
| Logged-in users | Only logged-in users |
| Creator only | Listed only for the person who registered it (administrators can open it from the admin pages) |
| Hidden | Not listed (the person who registered it and administrators can open it from “Hidden content” in the user name menu) |

1. In the list on Home or in a group, select the pen at the right end of a row of an item you registered (administrators: any item) (administrators can also select the row in the “Content” list in the admin pages).

   ![A row in the Home list, with the pen at the right end](images/row-edit-pen.webp)

2. Choose “Visibility”, then select “Save”.

   ![Parent group and Visibility on the edit page](images/edit-visibility.webp)

Items in a group are visible only to people who also meet the group’s visibility setting.

Visibility controls how an item appears as content. Files inside a folder can still be seen on the share screen by anyone who can view that share, whatever the visibility.

## Hide and show again

Set the visibility to “Hidden” on the edit page to remove an item from the lists on Home and in groups.

Hidden items are listed under “Hidden content” in the username menu at the top right (items you registered; administrators see all). To show one again, select the pen at the right end of its row there to open the edit page, then choose another visibility.

Set a group to “Creator only” to make it a space just for you. It can hold only your own “Creator only” content, and anything you add inside it becomes “Creator only”. A group that holds someone else’s content, or content that isn’t “Creator only”, can’t be set to “Creator only”.

## Put items in a group

To add a new item, use “Add” to the right of the heading on the group's page (→ “Start”).

Administrators can also add from the list in the admin pages.

1. Select “⋮” on the group row, then choose “Add inside...”.

   ![The group row menu](images/register-group-menu.webp)

2. Continue from step 2 of the admin pages steps in “Start”.

To move an existing item:

1. Open the edit page (→ “Change content” below).
2. Choose “Parent group”, then select “Save”.

   ![Parent group and Visibility on the edit page](images/edit-visibility.webp)

## Change content

Open the edit page, make your changes, then select “Save”. To open the edit page, select the pen at the right end of a row in the list on Home or in a group, or the pen next to the heading of a group or other page. Administrators can also select the row in the list in the admin pages.

To see how it looks to viewers, select “View on site” next to the heading of the edit page (it is also in the row’s “⋮” in the list in the admin pages). This opens the group the item is in (Home if it isn't in a group), where it's listed. Items set to “Hidden” and other people's “Creator only” items aren't listed there, so the item itself opens (shortcuts to files such as images still open the group).

![The Link edit page. Below the title and description are buttons to refetch each one.](images/edit-link.webp)

- Title and description
- Link: Use “Refetch title” and “Refetch description” to fetch them again from the linked site.
- Shortcut to a folder: “Include patterns”, “Exclude patterns” (→ “Limit what you show” above) and “Organize as a library” (→ [Set up a library](07-archive-setup.md)). Neither is asked when you add a folder, so change them here.
  - Changing the location with “Change...” under “Path” clears the list.

## Delete

1. In the “Delete” section at the bottom of the edit page, select “Delete...” (administrators can also choose it from a row's “⋮” in the list in the admin pages).

   ![The “Delete” section of the edit page](images/delete-section.webp)

2. Select “Delete”.

   ![The Delete content confirmation](images/content-delete.webp)

After deleting, administrators return to the list in the admin pages. Others return to the group the item was in (Home if it wasn't in a group).

- Deleting a shortcut or library does not delete files on your computer.
- Deleting a group moves its contents out of the group, to the top level.
