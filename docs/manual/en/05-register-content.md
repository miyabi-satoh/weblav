# Register contents

## Types

| What you want to show | Type |
|---|---|
| A folder on your computer, as it is | Folder |
| Many files, filterable by year or session | Archive (→ [Set up an archive](06-archive-setup.md)) |
| One file | File (upload, up to 100 MB by default) |
| An external website | Link |
| A collection of items | Group |

You can choose folders and archives only from “Shared folders” (→ [Add shared folders](04-roots.md)).
Only items registered here appear for viewers.

![How folders on the computer relate to the viewer's screen: add "Shared" as a shared folder, then register "Teaching materials" as a folder and "EIKEN" as an archive. Only those two appear for viewers. Unregistered "Drafts" and "Household accounts" outside the shared folder do not appear.](images/how-it-works.webp)

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

2. Browse from a shared folder to the folder you want to show, then select “Use this folder”.

   ![The folder selection screen](images/add-folder-picker.webp)

3. Select “Register”.

   ![The Add screen after choosing a folder](images/add-folder-ready.webp)

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
| Anyone | Everyone (default) |
| Sign-in required | Signed-in users |
| Private | The person who registered it |
| Hidden | No one (you can open it from the admin pages) |

1. In the “Contents” list, select “⋮” on the row, then choose “Edit”.

   ![The content row menu](images/row-menu.webp)

2. Choose “Visibility”, then select “Save”.

   ![Parent group and Visibility on the edit page](images/edit-visibility.webp)

Items in a group are visible only to people who also meet the group’s visibility setting.

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

![The Link edit page. Below the title and description are buttons to refetch each one.](images/edit-link.webp)

- Title and description
- Link: Use “Refetch title” and “Refetch description” to fetch them again from the linked site.
- File: Drop a new file or select one to replace it.
- Archive: Use “File extensions” (for example, `mp3, pdf`) to limit the files included. Leave it empty for all files. After changing it, select “Rescan” on the “Items” tab (→ [Set up an archive](06-archive-setup.md)).

## Delete

1. Select “⋮” on the row, then choose “Delete...”.

   ![The content row menu](images/row-menu.webp)

2. Select “Delete”.

   ![The Delete content confirmation](images/content-delete.webp)

- Deleting a folder or archive does not delete files on your computer.
- Deleting a group moves its contents out of the group, to the top level.
