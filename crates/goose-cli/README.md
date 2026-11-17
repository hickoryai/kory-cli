# Kory CLI

The interactive session keeps its prompt above a live children dock. Up from an
empty prompt opens the session's tool calls, including calls streamed by background
router turns. Down from an empty prompt selects a running child; Enter opens it,
and Esc or Up past the first child returns to the prompt. Ctrl+P/Ctrl+N retain
command-history navigation.

`session/child_sessions/dock.rs` owns the footer and reserved scroll region.
While Rustyline edits a prompt, its resize callback owns layout changes; the
background painter only refreshes rows at the agreed dimensions. Terminal output
and footer painting take the stdout lock before the footer state lock. Do not
reverse that order or resize the region independently of an active line editor.

`session/unsolicited.rs` renders background output through Rustyline's external
printer and publishes tool availability as it arrives. Opening the tool list first
settles those messages into the session transcript, so the list matches the calls
visible above the prompt.

The workspace uses a documented [Rustyline patch](../../vendor/rustyline/KORY-PATCH.md)
for buffered input, terminal write coordination, prompt attributes and resizing.
`tests/support/dock_terminal.rs` runs the real editor in isolated PTYs and inspects
the resulting terminal screen without starting an agent or calling a model.
