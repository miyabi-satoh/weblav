-- ブックマークは窓で選んだときにしか作らず、いつも書き込みの許可を持つので、許可の種類を持たない。
ALTER TABLE folder_bookmarks DROP COLUMN read_write;
