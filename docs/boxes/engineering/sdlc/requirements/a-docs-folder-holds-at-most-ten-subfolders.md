---
page-type: constraint
id: a-docs-folder-holds-at-most-ten-subfolders
---
# A docs folder holds at most ten subfolders

<Callout kind="note" title="The rule">

A folder of devpage's docs that is a page — it has a `README.md` — holds at
most ten subfolders that are pages themselves. A folder that would get an
eleventh is regrouped instead: its subjects are sorted under a few named
parts, each a folder with its own landing page.

Only what a reader sees as a subcategory is counted. A `requirements` folder
and its rule files, a deck's slides, a doc's chapters, a widgets folder and a
set of designs are lists by nature and are not counted.

</Callout>

## Why

A folder grows one subject at a time, and no single addition looks wrong. The
viewer's folder reached thirty-three subfolders that way, twenty-one of them
in one group of the sidebar: a reader looking for one subject had to read the
whole list, and nothing in it said which subjects belong together. A limit
makes the eleventh addition the moment the grouping is decided, while it is
one move and not thirty.

## Check

A test walks `docs/` and fails when a page folder holds more than ten page
subfolders, naming each such folder with its count. A second test gives a
folder ten subfolders, a `requirements` folder and a deck, which pass, and
then an eleventh, which is found.
