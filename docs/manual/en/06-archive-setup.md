# Set up an archive

An archive lets people filter files in a folder by “Year”, “Session”, or similar categories.

The person who registered an archive and administrators can configure it on this page.
You can only view archives registered by someone else. Ask an administrator to change one.

## Example: filter by year and session

Suppose your files are arranged like this:

```
Recordings
├─ 2024
│   ├─ Session 1
│   │   └─ main.mp3
│   └─ Session 2
└─ 2025
```

1. In “Contents”, select “Archive” from “Add...”, then select “Choose...”.

   ![The Add archive screen](images/add-archive-form.webp)

2. Open the `Recordings` folder, then select “Use this folder”.

   ![The folder picker with Recordings open](images/add-archive-picker.webp)

3. Select “Register”. The archive edit page opens.

   ![The Add screen after choosing Recordings](images/add-archive-ready.webp)

4. Select the “Axes” tab, then select “Add axis...”.

   ![The Axes tab and Add axis button](images/archive-axes-tab.webp)

5. Enter “Year” as the axis name, choose “Folder level” as the source and “Level 1” as the level, then select “Save”.

   ![The Add axis screen for folder level](images/axis-add-dir.webp)

6. Select “Add axis...” again. Save it with “Session” as the axis name and “Level 2” as the level.
7. Set “Title template” to `{Year} {Session} {fileName}`, then select “Save”.

   ![Title template on the Axes tab](images/archive-axes-template.webp)

8. Select the “Items” tab.

   ![The Items tab](images/archive-items-tab.webp)

9. Select the files to show, then select “Publish selected”.

   ![Two selected items and the Publish selected button](images/archive-items-publish.webp)

**Nothing appears if you forget step 9.**

## Separate by words in file names

Use this when separating files by words in their names, such as `main` and `notes`.

1. Select “Add axis...”, enter an axis name, choose “Word in file name” as the source, then select “Save”.

   ![The Add axis screen for a word in the file name](images/axis-add-word.webp)

2. Select the row for the axis you added.
3. Select “Add value”, enter the raw value (`main`) and display name (“Main program”). Repeat for each word, then select “Save”.

   ![The values table for a Word in file name axis](images/archive-values.webp)

Use “Match position” to choose where in the file name a word must occur.

- Anywhere (default): It matches if the word occurs anywhere in the file name.
- Start or after a separator: It matches only at the start of the file name or after a non-alphanumeric character, such as punctuation, a space, or Japanese text.
- End: It matches only at the end of the file name, excluding the extension.

## Ask an AI

If you are not sure how to set up the axes, you can have the AI service you use (ChatGPT, Claude, and so on) design them. WebLAV never sends anything to the AI; you paste the prompt and the answer yourself.

1. On the "Axes" tab, press "Ask an AI..."
2. Copy the prompt, then paste it into the AI service and send it. The prompt contains file paths, so check that it contains no names of people
3. Paste the AI's answer into "Paste the AI's answer" and press "Check"
4. Look at how many files get a value for each axis and at the examples of display titles. If they look right, press "Import"

Importing replaces the current axes and the display title. After importing, you can edit them like any other axes.

## Edit or delete axes

On the “Axes” tab, select “Edit...” or “Delete...” on the axis row.

![The axes list, with Edit and Delete buttons on each row](images/axis-row-actions.webp)

On the page opened by “Edit...”, you can also choose these settings:

- Show as a filter: Turn this off to hide it from viewer filters. It is still used for the title and sort order.
- Show the file name when this axis has no value: Turn this off to leave that part blank when building a title for a file without a value.

Changing the source or level and saving clears that axis’s values table.

## Change the order

- Filter order: On the “Axes” tab, drag the handle at the beginning of the axis row.
- Value order: Drag the handle at the beginning of the row in the values table.

## When you add or remove files

1. On the archive edit page, select “Rescan” on the “Items” tab.

   ![The Rescan button on the Items tab](images/archive-items-tab.webp)

2. Added files start unpublished. Select them, then select “Publish selected”.

   ![Two selected items and the Publish selected button](images/archive-items-publish.webp)

To stop publishing files, select them, then choose “Unpublish selected”.

## When files or folders are not found

- Files you deleted or moved no longer appear in viewers’ lists. On the “Items” tab of the admin page they show “Not found”, and rescanning removes them from the list.
- If you move or rename a file inside the archive, rescanning adds it as a new file, and it starts unpublished again. Publish it again.

If rescanning shows one of the following, the item list and the publish settings stay as they were.

- **“The folder was not found.”**: The archive folder was deleted, moved, or renamed, or an external drive was disconnected.
  - If you moved or renamed it, select “Change...” under “Path” on the “Details” tab and choose it. The publish settings start over.
  - If you disconnected a drive, connect it and rescan.
  - Viewers also see “The folder was not found.”
- **“Could not read "(folder)".” / “The folder could not be read.”**: WebLAV cannot read that folder (in the latter case, the archive folder itself). Check the folder’s permissions on the computer, make it readable, then rescan. For a network drive, check that it is connected.
