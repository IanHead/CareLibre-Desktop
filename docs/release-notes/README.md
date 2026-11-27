# Release notes

Each release has a hand-written note in this directory, named after its tag. For
example, the `v0.2.0` release uses `v0.2.0.md`. The release workflow checks for
that file and refuses to publish unless it contains `## New`, `## Changed` and
`## Fixed`, in that order.

Write every bullet for someone who uses CareLibre, not someone who builds it. Say
what a care coordinator, worker or administrator can now do, what works
differently, or what no longer goes wrong. Avoid commit hashes, file names,
function names, API details and words such as "refactor" unless the reader would
recognise them.

- **New** — capabilities that did not exist before.
- **Changed** — existing behaviour that works differently.
- **Fixed** — bugs and rough edges that no longer happen.

Keep all three sections. If nothing belongs in a section, write
`- Nothing in this release.` rather than deleting the heading.

Start from [`TEMPLATE.md`](TEMPLATE.md).

## Example

```markdown
## New

- The weekly roster now shows each worker's shifts side by side, with the
  client's name on every block.
- You can link a contact to an organisation directly from the relationship card.

## Changed

- Contact search keeps the last filter when you reopen the Contacts page.
- Sign-in now asks for confirmation before remembering a device.

## Fixed

- Intake drafts no longer lose the emergency contact when you close and reopen
  them.
- Long notes in a contact record no longer overlap the buttons below them.
```
