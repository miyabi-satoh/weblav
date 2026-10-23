# Register contents

## Types

| What you want to show | Type |
|---|---|
| A folder on your computer. You can also organize it as a library filterable by year or session | Folder (for the library → [Set up a library](06-archive-setup.md)) |
| One file | File (upload, up to 100 MB by default) |
| An external website | Link |
| A collection of items | Group |

You can choose folders only from “Allowed folders” (→ [Add allowed folders](04-roots.md)).
Only items registered here appear for viewers.

![How folders on the computer relate to the viewer's screen: add "Shared" as an allowed folder, then register "Teaching materials" and "EIKEN" as folders. Only those two appear for viewers. Unregistered "Drafts" and "Household accounts" outside the allowed folder do not appear.](images/how-it-works.webp)

Titles are assigned automatically. To change one, use the edit page that opens after registration.

## Start

1. On the “Contents” tab in the admin pages, select “Add...”.

   ![The Contents tab and Add button](images/contents-page.webp)

2. Choose a type.

   ![Choosing a type when adding content](images/quickstart-add-folder.webp)

The remaining steps differ by type.

## Folder

1. Select “Choose...”.

   ![The Add folder screen](images/add-folder-form.webp)

2. Browse from an allowed folder to the folder you want to show, then select “Use this folder”.

   ![The folder selection screen](images/add-folder-picker.webp)

3. Select “Register”.

   ![The Add screen after choosing a folder](images/add-folder-ready.webp)

### Publish or unpublish

By default, everything in a folder is published. Unpublish files or folders you do not want to show.

1. Open the folder, then select “⋮” on the row of a file or folder.
2. Choose “Unpublish”. To publish it again, choose “Publish”.

- Only people who can change the folder see “⋮”. They also see unpublished items, marked “Unpublished” under the name.
- Publishing or unpublishing a folder applies to everything inside it. Earlier changes to items inside it are cleared.
- Unpublished files cannot be downloaded or found by search either.
- If you rename or move a file, your change to it no longer applies, and it is treated like a newly found file.

To publish only the items you choose, turn off “Publish new files” under “Location and display” on the “Settings” tab of the edit page, then select “Save” in that section. New files start unpublished, so choose “Publish” for the ones to show. Publishing a file also makes the folders above it browsable.

Your changes are listed under “Publishing changes” on the edit page, and “Undo” reverts one. For a folder organized as a library, only folders are listed there. Publish or unpublish files on the “Items” tab (→ [Set up a library](06-archive-setup.md)).

## File

1. Select the area to choose a file (or drop a file there), then select “Register”.

   ![The Add file screen](images/add-file.webp)

## Link

1. Enter the URL, then select “Register”.

   ![The Add link screen](images/add-link.webp)

To show several links together, write them in a text file whose name ends with `.links.toml`, and put it in a folder or register it as a file.
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

1. Enter a title, then select “Register”.

   ![The Add group screen](images/add-group.webp)

## Change visibility

| Visibility | Who can see it |
|---|---|
| Everyone | Everyone, including people who aren't logged in (default) |
| Logged-in users | Only logged-in users |
| Creator only | Only the person who registered it |
| Hidden | No one (you can open it from the admin pages) |

1. In the “Contents” list, select “⋮” on the row, then choose “Edit”.

   ![The content row menu](images/row-menu.webp)

2. Choose “Visibility”, then select “Save”.

   ![Parent group and Visibility on the edit page](images/edit-visibility.webp)

Items in a group are visible only to people who also meet the group’s visibility setting.

Set a group to “Creator only” to make it a space just for you. It can hold only your own “Creator only” content, and anything you add inside it becomes “Creator only”. A group that holds someone else’s content, or content that isn’t “Creator only”, can’t be set to “Creator only”.

## Put items in a group

To add a new item:

1. Select “⋮” on the group row, then choose “Add inside...”.

   ![The group row menu](images/register-group-menu.webp)

2. Continue from step 2 in “Start”.

To move an existing item:

1. Open its edit page with “⋮” → “Edit”.
2. Choose “Parent group”, then select “Save”.

   ![Parent group and Visibility on the edit page](images/edit-visibility.webp)

## Change contents

Open the edit page with “⋮” → “Edit”, make your changes, then select “Save”.

To see how it looks to viewers, select “View on site” next to the heading of the edit page (it is also in the row’s “⋮” in the list). For an image, PDF, audio, or video file, this opens the group that contains it.

![The Link edit page. Below the title and description are buttons to refetch each one.](images/edit-link.webp)

- Title and description
- Link: Use “Refetch title” and “Refetch description” to fetch them again from the linked site.
- File: Drop a new file or select one to replace it.
- Folder: “Publish new files” (→ “Publish or unpublish” above) and “Organize as a library” (→ [Set up a library](06-archive-setup.md)). Neither is asked when you add a folder, so change them here.
  - Changing the location with “Change...” under “Path” clears all publish changes and the list.

## Delete

1. Select “⋮” on the row, then choose “Delete...”.

   ![The content row menu](images/row-menu.webp)

2. Select “Delete”.

   ![The Delete content confirmation](images/content-delete.webp)

You can also delete from the edit page: select “⋮” next to the heading, then choose “Delete...”. After deleting, you return to the list.

- Deleting a folder does not delete files on your computer.
- Deleting a group moves its contents out of the group, to the top level.
